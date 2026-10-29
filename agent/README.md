# AgentHub 对话管理 · Agent API & MCP 服务

将 Agent 对话管理（AgentHub）的完整多 Agent 纳管、全文搜索、预览、星标、归档、导出与清理能力暴露为统一的 Agent API 与标准 MCP (Model Context Protocol) 工具，供任何智能体直接调用。

服务默认监听 `127.0.0.1:8792`。

## 启动方式

```bash
# 启动本地服务
node agent/server.mjs

# 启动 MCP stdio 桥（服务未启动时会自动拉起）
node agent/mcp-server.mjs
```

## HTTP 契约

```
GET  /api/health          -> { ok: true, data: { project, version, agent_api: 1, tools, ... } }
GET  /api/agent/tools     -> { ok: true, data: [ { name, description, risk, input_schema } ] }
GET  /api/agent/manifest  -> { ok: true, data: { project, version, base_url, tools } }
POST /api/agent/tool      -> body { tool, input } -> { ok: true, tool, ms, data }
```

## 工具清单（前缀 `agenthub.`）

| 工具 | 风险 | 说明 |
|---|---|---|
| `agenthub.overview` | read | 获取全机 Agent 对话概览统计（总数、总占用、总消息数、已装 Agent、5 类清理候选） |
| `agenthub.list_agents` | read | 列出支持和已识别的所有 Agent（Codex、DSH、Antigravity、Qoder 等）及占用与路径 |
| `agenthub.list_sessions` | read | 多维检索会话列表（按 Agent、关键词、星标、归档、排序） |
| `agenthub.get_transcript` | read | 读取指定会话的全量时间线消息内容 |
| `agenthub.search` | read | 基于 SQLite 索引对全机所有 Agent 对话进行全文搜索 |
| `agenthub.star_session` | write | 会话加星收藏 / 取消收藏 |
| `agenthub.archive_session` | write | 会话归档 / 取消归档 |
| `agenthub.export_markdown` | write | 将会话完整导出为 Markdown 文档 |
| `agenthub.cleanup_candidates` | read | 列出清理中心的 5 类候选会话（超期、超大、极短、归档） |
| `agenthub.delete_sessions` | **exec** | 删除会话释放空间（移入回收站或永久删除）；**必须 `confirm: true`** |

## MCP 配置示例

```json
{
  "mcpServers": {
    "agenthub": {
      "command": "node",
      "args": ["C:/path/to/AgentHub/agent/mcp-server.mjs"]
    }
  }
}
```

## 统一入口（一个 MCP / 一个 HTTP API）

五个软件（AI Flow、FrameBoost、Agent 对话管理、清影下载器、声析）可以由
`agent-bus` 一个入口统一调用：本项目的 `agenthub.*` 工具原样暴露，
服务没起时会自动拉起。见 `agent-bus/README.md`。本项目自己的 `agent/mcp-server.mjs` 仍可单独使用。
