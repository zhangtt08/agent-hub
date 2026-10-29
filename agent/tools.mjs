#!/usr/bin/env node
// AgentHub Agent tools —— 对话管理项目的全部核心能力封装
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { DatabaseSync } from 'node:sqlite';
import { zstdDecompressSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { AgentError } from './server.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, '..');

export const project = {
  id: 'agenthub',
  name: 'agenthub',
  version: '0.1.0',
  summary: '本机 Agent 对话集中管理与空间清理：多 Agent 对话索引、全文检索、全量消息预览、星标、归档与清理。',
  port: 8792
};
export const projectId = 'agenthub';

function dataDir() {
  const base = process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming');
  return path.join(base, 'com.agenthub.desktop');
}

function loadState() {
  const file = path.join(dataDir(), 'state.json');
  try {
    if (fs.existsSync(file)) {
      return JSON.parse(fs.readFileSync(file, 'utf8'));
    }
  } catch {
    // ignore
  }
  return { version: 1, archived: {}, deleted: {}, starred: [] };
}

function saveState(state) {
  const dir = dataDir();
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, 'state.json');
  const tmp = path.join(dir, 'state.json.tmp');
  fs.writeFileSync(tmp, JSON.stringify(state, null, 2), 'utf8');
  fs.renameSync(tmp, file);
}

function openIndexDb(readOnly = true) {
  const dbPath = path.join(dataDir(), 'index.db');
  if (!fs.existsSync(dbPath)) return null;
  try {
    return new DatabaseSync(dbPath, { readOnly });
  } catch {
    return null;
  }
}

// 辅助哈希
function fnv1a(str) {
  let h = 0xcbf29ce484222325n;
  const prime = 0x100000001b3n;
  const buf = Buffer.from(str);
  for (let i = 0; i < buf.length; i++) {
    h ^= BigInt(buf[i]);
    h = (h * prime) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, '0');
}

// 已知 Agent 定义
const AGENTS = [
  { id: 'codex', name: 'Codex', vendor: 'OpenAI', roots: [path.join(os.homedir(), '.codex', 'sessions'), path.join(os.homedir(), '.codex', 'archived_sessions')] },
  { id: 'dsh', name: 'DeepSeek Harness', vendor: 'DeepSeek', roots: [path.join(os.homedir(), '.dsh', 'sessions')] },
  { id: 'antigravity', name: 'Antigravity', vendor: 'Google', roots: [path.join(os.homedir(), '.gemini', 'antigravity', 'conversations')] },
  { id: 'qoder', name: 'Qoder', vendor: 'Alibaba', roots: [path.join(process.env.APPDATA || '', 'com.qodercn.app.stable'), path.join(os.homedir(), '.qoder-cn', 'projects'), path.join(os.homedir(), '.qoder', 'projects')] },
  { id: 'claude', name: 'Claude Code', vendor: 'Anthropic', roots: [path.join(os.homedir(), '.claude', 'projects')] },
  { id: 'workbuddy', name: 'WorkBuddy', vendor: 'Tencent', roots: [path.join(os.homedir(), '.workbuddy')] },
  { id: 'opencode', name: 'OpenCode', vendor: 'OpenCode', roots: [path.join(os.homedir(), '.local', 'share', 'opencode')] },
  { id: 'trae', name: 'Trae', vendor: 'ByteDance', roots: [path.join(process.env.APPDATA || '', 'Trae CN', 'ModularData', 'ai-agent')] },
  { id: 'zcode', name: 'ZCode', vendor: 'ZCode', roots: [path.join(process.env.APPDATA || '', 'ZCode')] }
];

// 删除前必须确认目标确实位于已知 Agent 目录内，否则会误删用户其它文件
function insideAgentRoots(target) {
  const abs = path.resolve(target).toLowerCase();
  return AGENTS.some((a) =>
    a.roots.some((root) => {
      if (!root) return false;
      const r = path.resolve(root).toLowerCase();
      return abs === r || abs.startsWith(r + path.sep);
    })
  );
}

// 扫描所有已纳管的会话记录
function scanAllRows() {
  const state = loadState();
  const deletedSet = new Set(Object.keys(state.deleted || {}));
  const archivedSet = new Set(Object.keys(state.archived || {}));
  const starredSet = new Set(state.starred || []);
  const rows = [];

  // 1. Antigravity 会话
  const agDir = path.join(os.homedir(), '.gemini', 'antigravity');
  const agConv = path.join(agDir, 'conversations');
  const agSumDb = path.join(agDir, 'conversation_summaries.db');
  const agSummaries = new Map();
  if (fs.existsSync(agSumDb)) {
    try {
      const db = new DatabaseSync(agSumDb, { readOnly: true });
      const items = db.prepare('SELECT conversation_id, COALESCE(title,\'\') as title, COALESCE(step_count,0) as steps, COALESCE(preview,\'\') as preview FROM conversation_summaries').all();
      for (const it of items) {
        agSummaries.set(it.conversation_id, it);
      }
    } catch {
      // ignore
    }
  }
  if (fs.existsSync(agConv)) {
    try {
      const files = fs.readdirSync(agConv);
      for (const f of files) {
        if (!f.endsWith('.db')) continue;
        const full = path.join(agConv, f);
        const st = fs.statSync(full);
        const key = f.slice(0, -3);
        const sum = agSummaries.get(key);
        const id = fnv1a(`antigravity:${key}`.toLowerCase());
        if (deletedSet.has(id)) continue;
        const title = (sum?.title && sum.title.trim()) || (sum?.preview && sum.preview.trim()) || `Antigravity 会话 · ${new Date(st.mtimeMs).toLocaleDateString()}`;
        rows.push({
          id,
          agent: 'antigravity',
          title,
          project: 'Antigravity',
          project_path: agDir,
          path: full,
          size: st.size,
          mtime_ms: Math.floor(st.mtimeMs),
          mtime: new Date(st.mtimeMs).toISOString(),
          messages: sum ? Number(sum.steps) : 1,
          starred: starredSet.has(id),
          archived: archivedSet.has(id),
          readonly: false,
          previewable: true
        });
      }
    } catch {
      // ignore
    }
  }

  // 2. Codex 会话
  const codexDir = path.join(os.homedir(), '.codex', 'sessions');
  if (fs.existsSync(codexDir)) {
    function walkCodex(dir) {
      try {
        const ents = fs.readdirSync(dir, { withFileTypes: true });
        for (const ent of ents) {
          const full = path.join(dir, ent.name);
          if (ent.isDirectory()) {
            walkCodex(full);
          } else if (ent.isFile() && (ent.name.endsWith('.jsonl') || ent.name.endsWith('.zst'))) {
            const st = fs.statSync(full);
            const id = fnv1a(full.toLowerCase());
            if (deletedSet.has(id)) continue;
            rows.push({
              id,
              agent: 'codex',
              title: ent.name,
              project: path.basename(dir),
              project_path: dir,
              path: full,
              size: st.size,
              mtime_ms: Math.floor(st.mtimeMs),
              mtime: new Date(st.mtimeMs).toISOString(),
              messages: 10,
              starred: starredSet.has(id),
              archived: archivedSet.has(id),
              readonly: false,
              previewable: true
            });
          }
        }
      } catch {
        // ignore
      }
    }
    walkCodex(codexDir);
  }

  // 3. DSH 压缩会话（*.jsonl.zstd）
  const dshDir = path.join(os.homedir(), '.dsh', 'sessions');
  if (fs.existsSync(dshDir)) {
    for (const full of walkFiles(dshDir, (n) => n.endsWith('.jsonl.zstd'))) {
      const st = fs.statSync(full);
      const id = fnv1a(`dsh:${path.basename(full, '.jsonl.zstd')}`.toLowerCase());
      if (deletedSet.has(id)) continue;
      rows.push({
        id,
        agent: 'dsh',
        title: path.basename(full, '.jsonl.zstd'),
        project: path.basename(path.dirname(full)).replace(/^--|--$/g, '').slice(0, 60) || '未识别项目',
        project_path: path.dirname(full),
        path: full,
        size: st.size,
        mtime_ms: Math.floor(st.mtimeMs),
        mtime: new Date(st.mtimeMs).toISOString(),
        messages: countDshMessages(full),
        starred: starredSet.has(id),
        archived: archivedSet.has(id),
        readonly: false,
        previewable: true
      });
    }
  }

  // 4. Claude Code / Qoder CLI：Claude 风格的 JSONL 会话
  for (const [agent, bases] of [
    ['claude', [path.join(os.homedir(), '.claude')]],
    ['qoder', [path.join(os.homedir(), '.qoder-cn'), path.join(os.homedir(), '.qoder')]]
  ]) {
    for (const base of bases) {
      const projects = path.join(base, 'projects');
      if (!fs.existsSync(projects)) continue;
      for (const full of walkFiles(projects, (n) => n.endsWith('.jsonl'))) {
        const parsed = parseCliJsonl(full);
        if (!parsed || parsed.messages === 0) continue;
        const id = fnv1a(`${agent}:${path.basename(full, '.jsonl')}`.toLowerCase());
        if (deletedSet.has(id)) continue;
        rows.push({
          id,
          agent,
          title: parsed.title || `会话 · ${new Date().toLocaleDateString()}`,
          project: parsed.project || '未识别项目',
          project_path: parsed.cwd || '',
          path: full,
          size: fs.statSync(full).size,
          mtime_ms: Math.floor(fs.statSync(full).mtimeMs),
          mtime: new Date(fs.statSync(full).mtimeMs).toISOString(),
          messages: parsed.messages,
          starred: starredSet.has(id),
          archived: archivedSet.has(id),
          readonly: false,
          previewable: true
        });
      }
    }
  }

  return rows;
}

function walkFiles(root, match) {
  const out = [];
  const stack = [root];
  let guard = 4000;
  while (stack.length && guard-- > 0) {
    const dir = stack.pop();
    let ents;
    try {
      ents = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const ent of ents) {
      const full = path.join(dir, ent.name);
      if (ent.isDirectory()) stack.push(full);
      else if (ent.isFile() && match(ent.name)) out.push(full);
    }
  }
  return out;
}

// DSH 用 zstd 压缩，只解开头一段统计消息条数即可
function countDshMessages(file) {
  try {
    const raw = fs.readFileSync(file);
    const text = zstdDecompressSync(raw).toString('utf8');
    const user = (text.match(/"type":"user\/message"/g) || []).length;
    const ai = (text.match(/"type":"assistant\/message"/g) || []).length;
    return user + ai;
  } catch {
    return 0;
  }
}

// Claude Code / Qoder CLI 记录形如 {"type":"user","message":{"role","content"}}
function parseCliJsonl(file) {
  let messages = 0;
  let title = '';
  let cwd = '';
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
  for (const line of text.split('\n')) {
    if (!line.startsWith('{')) continue;
    let v;
    try {
      v = JSON.parse(line);
    } catch {
      continue;
    }
    if (!cwd && typeof v.cwd === 'string' && v.cwd) cwd = v.cwd;
    if (!cwd && v.type === 'workspace-directories' && Array.isArray(v.directories)) {
      cwd = v.directories.find((d) => typeof d === 'string' && d) || '';
    }
    if (v.type === 'user') {
      messages += 1;
      if (!title) {
        title = contentText(v.message && v.message.content).slice(0, 68);
      }
    } else if (v.type === 'assistant') {
      messages += 1;
    }
  }
  const project = cwd ? path.basename(cwd) : '';
  return { messages, title, cwd, project };
}

function contentText(content) {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    return content
      .map((i) => (i && (i.text || i.input_text || i.output_text)) || '')
      .filter(Boolean)
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim();
  }
  return '';
}

export const tools = [
  {
    name: 'agenthub.overview',
    description: '获取全机 Agent 对话概览统计：纳管总会话数、占用磁盘总量、总消息数、已安装 Agent 数量以及清理中心的低价值候选分类。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {},
      additionalProperties: false
    },
    handler: async () => {
      const rows = scanAllRows();
      // 只读索引来源不可删除，不能进入清理候选
      const cleanable = rows.filter((r) => !(r.readonly && !(typeof r.path === 'string' && r.path.includes('#'))));
      const now = Date.now();
      const totalBytes = rows.reduce((s, r) => s + r.size, 0);
      const totalMessages = rows.reduce((s, r) => s + r.messages, 0);

      const buckets = [
        {
          key: 'old',
          label: '超过 30 天未更新',
          count: cleanable.filter((r) => now - r.mtime_ms > 30 * 86400000).length,
          bytes: cleanable.filter((r) => now - r.mtime_ms > 30 * 86400000).reduce((s, r) => s + r.size, 0)
        },
        {
          key: 'archived',
          label: '已归档的会话',
          count: cleanable.filter((r) => r.archived).length,
          bytes: cleanable.filter((r) => r.archived).reduce((s, r) => s + r.size, 0)
        },
        {
          key: 'tiny',
          label: '几乎没有内容（≤ 2 条消息）',
          count: cleanable.filter((r) => r.messages <= 2).length,
          bytes: cleanable.filter((r) => r.messages <= 2).reduce((s, r) => s + r.size, 0)
        }
      ];

      return {
        totals: {
          conversations: rows.length,
          bytes: totalBytes,
          mb: +(totalBytes / 1048576).toFixed(2),
          messages: totalMessages,
          installedAgents: AGENTS.filter((a) => a.roots.some((r) => fs.existsSync(r))).length
        },
        buckets,
        dataDir: dataDir()
      };
    }
  },
  {
    name: 'agenthub.list_agents',
    description: '列出系统支持和识别的所有 Agent（Codex、DSH、Antigravity、Qoder、WorkBuddy、OpenCode、Trae、ZCode），包含安装态、会话数与存储目录。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {},
      additionalProperties: false
    },
    handler: async () => {
      const rows = scanAllRows();
      const agents = AGENTS.map((a) => {
        const mine = rows.filter((r) => r.agent === a.id);
        const installed = a.roots.some((r) => fs.existsSync(r));
        return {
          id: a.id,
          name: a.name,
          vendor: a.vendor,
          installed,
          roots: a.roots.filter((r) => fs.existsSync(r)),
          count: mine.length,
          bytes: mine.reduce((s, r) => s + r.size, 0),
          messages: mine.reduce((s, r) => s + r.messages, 0)
        };
      });
      return { agents, count: agents.length };
    }
  },
  {
    name: 'agenthub.list_sessions',
    description: '多维筛选并列出会话列表。支持按 Agent 名称、关键词搜索、星标/归档筛选，支持按更新时间、体积或消息数排序。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {
        agent: { type: 'string', description: 'Agent 标识，如 antigravity, codex, dsh 等' },
        keyword: { type: 'string', description: '按标题或项目名称模糊搜索' },
        starred: { type: 'boolean', description: '是否只看已星标会话' },
        archived: { type: 'boolean', description: '是否包含或仅看归档会话' },
        sort: { type: 'string', enum: ['mtime', 'size', 'messages'], description: '排序字段，默认 mtime 倒序' },
        limit: { type: 'integer', description: '返回上限，默认 50，最大 500' }
      },
      additionalProperties: false
    },
    handler: async (input) => {
      let rows = scanAllRows();
      if (input.agent) {
        rows = rows.filter((r) => r.agent === input.agent.toLowerCase());
      }
      if (typeof input.starred === 'boolean') {
        rows = rows.filter((r) => r.starred === input.starred);
      }
      if (typeof input.archived === 'boolean') {
        rows = rows.filter((r) => r.archived === input.archived);
      }
      if (input.keyword && typeof input.keyword === 'string') {
        const kw = input.keyword.trim().toLowerCase();
        rows = rows.filter((r) => (r.title && r.title.toLowerCase().includes(kw)) || (r.project && r.project.toLowerCase().includes(kw)));
      }
      const sort = input.sort || 'mtime';
      if (sort === 'size') {
        rows.sort((a, b) => b.size - a.size);
      } else if (sort === 'messages') {
        rows.sort((a, b) => b.messages - a.messages);
      } else {
        rows.sort((a, b) => b.mtime_ms - a.mtime_ms);
      }
      const limit = Math.min(500, Math.max(1, Number(input.limit) || 50));
      return {
        count: rows.length,
        limit,
        sessions: rows.slice(0, limit)
      };
    }
  },
  {
    name: 'agenthub.get_transcript',
    description: '获取指定会话的完整对话消息记录（角色、时间戳与正文），供 Agent 分析历史或提取内容。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {
        id: { type: 'string', description: '会话 ID' }
      },
      required: ['id'],
      additionalProperties: false
    },
    handler: async (input) => {
      const rows = scanAllRows();
      const session = rows.find((r) => r.id === input.id);
      if (!session) throw new AgentError('not_found', `没有找到会话：${input.id}`);

      // 先查 index.db
      const db = openIndexDb(true);
      let messages = [];
      if (db) {
        try {
          const rowsInDb = db.prepare('SELECT seq, role, text FROM messages WHERE session_id = ? ORDER BY seq ASC').all(session.id);
          if (rowsInDb && rowsInDb.length > 0) {
            messages = rowsInDb.map((m) => ({ seq: m.seq, role: m.role, text: m.text }));
          }
        } catch {
          // ignore
        }
      }

      // 如果 index.db 里没有，且是 Antigravity SQLite 会话库，直接从 sqlite steps 表提取
      if (!messages.length && session.path && session.path.endsWith('.db') && fs.existsSync(session.path)) {
        try {
          const convDb = new DatabaseSync(session.path, { readOnly: true });
          const steps = convDb.prepare('SELECT step_index, source, type, content FROM steps ORDER BY step_index ASC').all();
          let seq = 0;
          for (const s of steps) {
            if (s.content && typeof s.content === 'string') {
              messages.push({
                seq: ++seq,
                role: s.type === 'USER_INPUT' ? 'user' : 'assistant',
                text: s.content.slice(0, 10000)
              });
            }
          }
        } catch {
          // ignore
        }
      }

      return {
        session,
        messagesCount: messages.length,
        messages
      };
    }
  },
  {
    name: 'agenthub.search',
    description: '对全机已索引的所有 Agent 对话正文进行全文搜索，返回高亮命中文本片段与所属会话。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: '搜索关键词（至少2个字）' },
        limit: { type: 'integer', description: '返回结果数量限制，默认 30' }
      },
      required: ['query'],
      additionalProperties: false
    },
    handler: async (input) => {
      const q = String(input.query || '').trim();
      if (q.length < 2) throw new AgentError('bad_input', '搜索关键词至少需要 2 个字符');
      const db = openIndexDb(true);
      if (!db) return { ready: false, groups: [] };

      const limit = Math.min(100, Math.max(1, Number(input.limit) || 30));
      const stmt = db.prepare('SELECT session_id, seq, role, text FROM messages WHERE text LIKE ? LIMIT ?');
      const hits = stmt.all(`%${q}%`, limit * 4);

      const rows = scanAllRows();
      const groupsMap = new Map();
      for (const h of hits) {
        const sess = rows.find((r) => r.id === h.session_id);
        if (!sess) continue;
        if (!groupsMap.has(h.session_id)) {
          if (groupsMap.size >= limit) break;
          groupsMap.set(h.session_id, { conversation: sess, hits: [] });
        }
        const g = groupsMap.get(h.session_id);
        if (g.hits.length < 3) {
          const idx = h.text.indexOf(q);
          const start = Math.max(0, idx - 40);
          const end = Math.min(h.text.length, idx + q.length + 40);
          const snippet = (start > 0 ? '…' : '') + h.text.slice(start, end).replace(/[\r\n]+/g, ' ') + (end < h.text.length ? '…' : '');
          g.hits.push({ seq: h.seq, role: h.role, snippet });
        }
      }

      return {
        query: q,
        ready: true,
        count: groupsMap.size,
        groups: Array.from(groupsMap.values())
      };
    }
  },
  {
    name: 'agenthub.star_session',
    description: '将会话标记为加星收藏或取消收藏。',
    risk: 'write',
    input_schema: {
      type: 'object',
      properties: {
        id: { type: 'string', description: '会话 ID' },
        starred: { type: 'boolean', description: 'true 为加星，false 为取消' }
      },
      required: ['id', 'starred'],
      additionalProperties: false
    },
    handler: async (input) => {
      const state = loadState();
      const set = new Set(state.starred || []);
      if (input.starred) {
        set.add(input.id);
      } else {
        set.delete(input.id);
      }
      state.starred = Array.from(set);
      saveState(state);
      return { id: input.id, starred: Boolean(input.starred) };
    }
  },
  {
    name: 'agenthub.archive_session',
    description: '将会话标记为归档或取消归档。',
    risk: 'write',
    input_schema: {
      type: 'object',
      properties: {
        id: { type: 'string', description: '会话 ID' },
        archived: { type: 'boolean', description: 'true 为归档，false 为移出归档' }
      },
      required: ['id', 'archived'],
      additionalProperties: false
    },
    handler: async (input) => {
      const state = loadState();
      state.archived = state.archived || {};
      if (input.archived) {
        state.archived[input.id] = new Date().toISOString();
      } else {
        delete state.archived[input.id];
      }
      saveState(state);
      return { id: input.id, archived: Boolean(input.archived) };
    }
  },
  {
    name: 'agenthub.export_markdown',
    description: '将指定会话的完整对话内容导出为格式整齐的 Markdown 文件到下载目录或指定路径。',
    risk: 'write',
    input_schema: {
      type: 'object',
      properties: {
        id: { type: 'string', description: '会话 ID' },
        outputPath: { type: 'string', description: '导出路径（可选，默认导出到系统 Downloads 目录）' }
      },
      required: ['id'],
      additionalProperties: false
    },
    handler: async (input) => {
      const rows = scanAllRows();
      const session = rows.find((r) => r.id === input.id);
      if (!session) throw new AgentError('not_found', `没有找到会话：${input.id}`);

      // 获取消息
      let messages = [];
      const db = openIndexDb(true);
      if (db) {
        try {
          const items = db.prepare('SELECT seq, role, text FROM messages WHERE session_id = ? ORDER BY seq ASC').all(session.id);
          if (items.length) messages = items;
        } catch {
          // ignore
        }
      }

      let content = `# ${session.title}\n\n`;
      content += `- **Agent**: ${session.agent}\n`;
      content += `- **项目**: ${session.project}\n`;
      content += `- **更新时间**: ${session.mtime}\n`;
      content += `- **原始路径**: \`${session.path}\`\n\n---\n\n`;

      if (messages.length) {
        for (const m of messages) {
          const roleLabel = m.role === 'user' ? '👤 **用户**' : '🤖 **助手**';
          content += `### ${roleLabel}\n\n${m.text}\n\n`;
        }
      } else {
        content += `*（会话保存在数据库中，共 ${session.messages} 条记录）*\n`;
      }

      const cleanName = (session.title || '会话').replace(/[\\/:*?"<>|]/g, '-').slice(0, 40);
      const downloadDir = path.join(os.homedir(), 'Downloads');
      const outPath = input.outputPath ? path.resolve(input.outputPath) : path.join(downloadDir, `${cleanName}.md`);
      fs.mkdirSync(path.dirname(outPath), { recursive: true });
      fs.writeFileSync(outPath, content, 'utf8');

      return {
        id: session.id,
        exportedPath: outPath,
        bytes: Buffer.byteLength(content)
      };
    }
  },
  {
    name: 'agenthub.cleanup_candidates',
    description: '列出清理中心的候选清理会话名单（超过30天未更新、已归档、极少内容）。只按低价值筛选，不按体积。',
    risk: 'read',
    input_schema: {
      type: 'object',
      properties: {
        bucket: { type: 'string', enum: ['old', 'archived', 'tiny', 'all'], description: '候选分类，默认 all' },
        limit: { type: 'integer', description: '最多返回条数，默认 50' }
      },
      additionalProperties: false
    },
    handler: async (input) => {
      const rows = scanAllRows();
      const now = Date.now();
      const bucket = input.bucket || 'all';
      const limit = Math.min(200, Math.max(1, Number(input.limit) || 50));

      let matched = [];
      if (bucket === 'old') matched = rows.filter((r) => now - r.mtime_ms > 30 * 86400000);
      else if (bucket === 'archived') matched = rows.filter((r) => r.archived);
      else if (bucket === 'tiny') matched = rows.filter((r) => r.messages <= 2);
      else {
        matched = rows.filter((r) => now - r.mtime_ms > 30 * 86400000 || r.archived || r.messages <= 2);
      }

      return {
        bucket,
        count: matched.length,
        candidates: matched.slice(0, limit).map((r) => ({
          id: r.id,
          agent: r.agent,
          title: r.title,
          sizeMb: +(r.size / 1048576).toFixed(2),
          messages: r.messages,
          mtime: r.mtime,
          path: r.path
        }))
      };
    }
  },
  {
    name: 'agenthub.delete_sessions',
    description: '删除指定的会话释放磁盘空间。purge 为 true 时永久删除，false 时移入回收站。必须显式带 confirm: true。',
    risk: 'exec',
    input_schema: {
      type: 'object',
      properties: {
        ids: { type: 'array', items: { type: 'string' }, description: '待删除的会话 ID 列表' },
        purge: { type: 'boolean', description: '是否永久删除，默认 false（移入回收站）' },
        confirm: { type: 'boolean', description: '必须为 true 确认删除' }
      },
      required: ['ids', 'confirm'],
      additionalProperties: false
    },
    handler: async (input) => {
      if (input.confirm !== true) {
        throw new AgentError('confirm_required', 'agenthub.delete_sessions 属于写/删除操作，必须显式带 confirm: true');
      }
      const ids = Array.isArray(input.ids) ? input.ids : [];
      if (!ids.length) throw new AgentError('bad_input', '请提供待删除的会话 ID 列表');

      const state = loadState();
      state.deleted = state.deleted || {};
      const deleted = [];
      const skipped = [];
      let freedBytes = 0;

      const rows = scanAllRows();
      for (const id of ids) {
        const sess = rows.find((r) => r.id === id);
        if (!sess) {
          skipped.push({ id, reason: 'not_found' });
          continue;
        }
        // 只读索引、或路径不在已知 Agent 目录内的条目，一律拒绝物理删除
        const dbBacked = typeof sess.path === 'string' && sess.path.includes('#');
        if (sess.readonly && !dbBacked) {
          skipped.push({ id, reason: 'read_only_source' });
          continue;
        }
        let removed = false;
        if (sess.path && fs.existsSync(sess.path) && input.purge) {
          if (!insideAgentRoots(sess.path)) {
            skipped.push({ id, reason: 'outside_agent_roots' });
            continue;
          }
          try {
            fs.unlinkSync(sess.path);
            removed = true;
          } catch (err) {
            skipped.push({ id, reason: 'unlink_failed', detail: String(err && err.message) });
            continue;
          }
        }
        // Only real file removal frees disk space.
        if (removed) freedBytes += sess.size || 0;
        state.deleted[id] = new Date().toISOString();
        deleted.push(id);
      }
      saveState(state);

      return {
        success: true,
        deletedCount: deleted.length,
        deleted,
        skipped,
        freedBytes,
        freedMb: +(freedBytes / 1048576).toFixed(2),
        purge: Boolean(input.purge)
      };
    }
  }
];
