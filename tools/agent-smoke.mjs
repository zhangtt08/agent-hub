import { spawn } from 'node:child_process';
import assert from 'node:assert';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');
const TEST_PORT = 8992;
const BASE_URL = `http://127.0.0.1:${TEST_PORT}`;

console.log('--- Testing AgentHub Agent API & MCP ---');

const server = spawn('node', ['agent/server.mjs'], {
  cwd: ROOT,
  env: { ...process.env, AGENT_PORT: String(TEST_PORT) },
  stdio: 'pipe'
});

server.stderr.on('data', (d) => process.stderr.write(d));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitForHealth() {
  for (let i = 0; i < 40; i++) {
    try {
      const res = await fetch(`${BASE_URL}/api/health`, { signal: AbortSignal.timeout(1000) });
      if (res.ok) {
        return await res.json();
      }
    } catch {
      await sleep(250);
    }
  }
  throw new Error('Server did not become healthy in time');
}

try {
  const health = await waitForHealth();
  console.log('✓ GET /api/health returned healthy');
  assert.strictEqual(health.ok, true);
  assert.strictEqual(health.data.project, 'agenthub');
  assert.strictEqual(health.data.agent_api, 1);
  assert(health.data.tools >= 9);

  let token = '';
  if (health.data.guard?.token_file) {
    try {
      const { readFileSync } = await import('node:fs');
      token = readFileSync(health.data.guard.token_file, 'utf8').trim();
    } catch {
      // ignore
    }
  }

  const postHeaders = {
    'content-type': 'application/json',
    ...(token ? { 'x-agenthub-token': token } : {})
  };

  // Tools
  const toolsRes = await fetch(`${BASE_URL}/api/agent/tools`);
  const toolsData = await toolsRes.json();
  console.log(`✓ GET /api/agent/tools returned ${toolsData.data.length} tools`);
  assert.strictEqual(toolsData.ok, true);
  const toolNames = toolsData.data.map((t) => t.name);
  assert(toolNames.includes('agenthub.overview'));
  assert(toolNames.includes('agenthub.list_agents'));
  assert(toolNames.includes('agenthub.list_sessions'));
  assert(toolNames.includes('agenthub.get_transcript'));
  assert(toolNames.includes('agenthub.search'));
  assert(toolNames.includes('agenthub.delete_sessions'));

  // Manifest
  const manifestRes = await fetch(`${BASE_URL}/api/agent/manifest`);
  const manifestData = await manifestRes.json();
  console.log('✓ GET /api/agent/manifest valid');
  assert.strictEqual(manifestData.data.project, 'agenthub');

  // Test tool call: agenthub.overview
  const callOverview = await fetch(`${BASE_URL}/api/agent/tool`, {
    method: 'POST',
    headers: postHeaders,
    body: JSON.stringify({ tool: 'agenthub.overview', input: {} })
  });
  const overviewRes = await callOverview.json();
  assert.strictEqual(overviewRes.ok, true);
  assert(overviewRes.data.totals.conversations >= 0);
  // 清理中心现在就是三类候选（超过30天未更新 / 已归档 / 几乎没有内容），界面按接口返回渲染
  const bucketKeys = overviewRes.data.buckets.map((b) => b.key);
  for (const key of ['old', 'archived', 'tiny']) assert(bucketKeys.includes(key), `缺少清理分类 ${key}`);
  console.log(`✓ agenthub.overview: ${overviewRes.data.totals.conversations} conversations, ${overviewRes.data.totals.mb} MB`);

  // Test tool call: agenthub.list_agents
  const callAgents = await fetch(`${BASE_URL}/api/agent/tool`, {
    method: 'POST',
    headers: postHeaders,
    body: JSON.stringify({ tool: 'agenthub.list_agents', input: {} })
  });
  const agentsRes = await callAgents.json();
  assert.strictEqual(agentsRes.ok, true);
  assert(agentsRes.data.agents.length >= 8);
  console.log(`✓ agenthub.list_agents: ${agentsRes.data.agents.length} agents detected`);

  // Test tool call: agenthub.list_sessions
  const callSessions = await fetch(`${BASE_URL}/api/agent/tool`, {
    method: 'POST',
    headers: postHeaders,
    body: JSON.stringify({ tool: 'agenthub.list_sessions', input: { limit: 10 } })
  });
  const sessionsRes = await callSessions.json();
  assert.strictEqual(sessionsRes.ok, true);
  console.log(`✓ agenthub.list_sessions returned ${sessionsRes.data.sessions.length} sessions`);

  // Test tool call: search
  const callSearch = await fetch(`${BASE_URL}/api/agent/tool`, {
    method: 'POST',
    headers: postHeaders,
    body: JSON.stringify({ tool: 'agenthub.search', input: { query: 'agent' } })
  });
  const searchRes = await callSearch.json();
  assert.strictEqual(searchRes.ok, true);
  console.log(`✓ agenthub.search found ${searchRes.data.groups.length} matching session groups`);

  // Test delete without confirm is refused
  const callDel = await fetch(`${BASE_URL}/api/agent/tool`, {
    method: 'POST',
    headers: postHeaders,
    body: JSON.stringify({ tool: 'agenthub.delete_sessions', input: { ids: ['test-id'], confirm: false } })
  });
  const delRes = await callDel.json();
  assert.strictEqual(delRes.ok, false);
  assert.strictEqual(delRes.error.code, 'confirm_required');
  console.log('✓ agenthub.delete_sessions correctly requires confirm: true');

  // Test MCP JSON-RPC
  const mcp = spawn('node', [path.join(ROOT, 'agent', 'mcp-server.mjs')], {
    cwd: ROOT,
    env: { ...process.env, AGENT_BASE_URL: BASE_URL },
    stdio: ['pipe', 'pipe', 'inherit']
  });

  const mcpPromise = new Promise((resolve, reject) => {
    let out = '';
    mcp.stdout.on('data', (d) => {
      out += d.toString();
      if (out.includes('"jsonrpc":"2.0"') && out.includes('"tools"')) {
        resolve(out);
      }
    });
    mcp.on('error', reject);
  });

  mcp.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }) + '\n');
  mcp.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} }) + '\n');

  const mcpOut = await mcpPromise;
  console.log('✓ MCP stdio bridge responded with initialize and tools/list');
  mcp.kill();

  console.log('=== ALL AGENTHUB AGENT API & MCP CHECKS PASSED ===');
} finally {
  server.kill('SIGTERM');
}
