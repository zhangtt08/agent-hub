# Agent 对话管理

把本机各个 Agent 的对话集中到一个桌面软件里：自动识别、统一浏览、预览、归档，并直接删除源文件释放空间。

- **Tauri 2 + WebView2**，无边框一体化窗口，单文件 exe，不依赖 Node.js
- 纯本地：不联网、不上传，只读取本机 Agent 的数据目录
- 删除前二次确认，默认移入回收站，可选永久删除

## 下载

到 [Releases](https://github.com/zhangtt08/agent-hub/releases) 下载：

- `AgentHub-Setup-0.1.0.exe`：安装版，按当前用户安装到 `%LOCALAPPDATA%\AgentHub`，开始菜单生成快捷方式
- `AgentHub.exe`：绿色版，双击即用，不写注册表

系统要求：Windows 10/11 x64，需要 Microsoft Edge WebView2 Runtime（Win11 自带）。

## 运行

两种方式，任选其一：

- **安装版**：双击 `AgentHub-Setup-0.1.0.exe`，按当前用户安装到 `%LOCALAPPDATA%\AgentHub`，
  开始菜单会出现快捷方式；卸载用同目录的 `uninstall.exe`，或「设置 → 应用」里卸载。
- **绿色版**：直接双击 `AgentHub.exe`，不写注册表、不需要安装。

从源码构建：双击 `build.cmd`（需要 Rust 与 Visual Studio C++ 生成工具），
产物在 `src-tauri\target\release\agent-hub.exe`；再执行
`npx @tauri-apps/cli@2 build` 可生成 NSIS 安装包。

## 支持的 Agent

| Agent | 数据位置 | 读取 | 删除 |
|---|---|---|---|
| Codex | `~/.codex/sessions`、`archived_sessions` | 标题 / 项目 / 消息数 / 预览 | 可删除源文件 |
| DeepSeek Harness | `~/.dsh/sessions/**/session.v4.jsonl.zstd` | 同上（zstd 多帧） | 可删除源文件 |
| Antigravity | `~/.gemini/antigravity/conversations/*.db` | 标题 / 步骤数（无标题时用首条提问） | 可删除整个会话库文件 |
| Qoder | `%APPDATA%\com.qodercn.app.stable\main.sqlite`（国际版 `com.qoder.app.stable`） | 标题 / 项目 / 消息数 / 归档状态 | 只读索引 |
| WorkBuddy | `~/.workbuddy/workbuddy.db` | 会话列表 / 标题 / 时间 | 只读索引 |
| OpenCode | `~/.local/share/opencode/opencode.db` | 会话列表 / 消息数 | 只读索引 |
| Trae | `%APPDATA%\Trae CN\ModularData\ai-agent` | 已识别数据目录 | 待适配 |
| ZCode | `%APPDATA%\ZCode` | 已识别数据目录 | 待适配 |

> 数据库型 Agent（Qoder / WorkBuddy / OpenCode）现在**支持删除**：按会话 ID 在自己的库里逐表移除
> 记录，父会话的子消息、索引片段、用量统计一并清理，永久删除时再 `VACUUM` 回收空间。
> 界面上标「数据库可删」。建议删除前先退出对应 Agent，避免它持有数据库锁。

### 为什么 Trae / ZCode 还没有会话列表

这不是漏做，而是本机数据不可读，应用会如实标注「已识别 · 待适配」：

- **Trae**：`ModularData/ai-agent/database.db` 文件头不是 SQLite（`54 EA 10 56 …`），
  字节熵 ≈ 8.0，全文没有任何 JSON / SQLite 页 / 明文键名 —— 内容被加密，需要密钥才能解。
  连 `database.db-wal` 也是加密页。
- **ZCode**：`%APPDATA%\ZCode` 只有 Electron 的 session/cache 目录，没有会话数据库或 JSONL。

等这些应用公开本地格式（或提供导出）后，按上面的「新增 Agent 适配」步骤接入即可。

## 功能

- **一体化窗口**：无系统边框（DWM 边框染成页面底色）、透明标题栏，最小化 / 最大化 / 关闭都是自绘
- **概览**：Agent 数量、会话数、占用空间、可清理体积，外加最近会话与预览
- **会话页**：左列表 + 右对话的双栏浏览，按 Agent / 关键词筛选，按最近更新 / 占用 / 消息数排序
- **内容搜索**：输入 2 个字以上会同时搜对话正文，命中处显示上下文片段，点进去直接跳到那条消息；索引只对变化的文件重建
- **收藏与只看提问**：会话可加星并在「★ 收藏」里筛出；对话里可一键只看你发过的提问
- **完整对话**：读取整份会话文件（Codex JSONL、DSH zstd 多帧），按角色分列、Markdown 渲染、代码块高亮样式，超过 120 条分页加载
- **项目页**：按项目目录汇总会话数、消息数、占用与最近活动，点击即可下钻到该项目的会话
- **统计页**：按 Agent / 项目的占用分布、最近 6 个月会话趋势、可清理空间估算
- **设置页**：数据来源与模式、删除方式（回收站 / 永久）、本机数据目录与重新扫描
- **导出**：把当前会话导出为 Markdown 到系统下载目录
- **清理中心**：超过 30 天 / 已归档 / 几乎没有内容 / 大于 20 MB / 大于 5 MB 五类，勾选后批量删除
- **删除**：默认移入回收站；勾选「永久删除」立即释放磁盘空间；数据库型会话按记录删除
- **强制删除**：绕过回收站，清只读属性、重试被占用的文件、清理 sqlite 边车文件，并在结束后对数据库 `VACUUM`
- **删除不卡界面**：删除任务在后台线程执行，主线程只负责最后的索引更新；批量删除 12 条约 260ms，大库 `VACUUM` 约 2.9s 也不会冻结窗口
- **安全**：只允许删除已知 Agent 目录内的文件，路径校验失败直接拒绝

## 新增 Agent 自动接入

不用改代码：应用每次扫描时会在 `%APPDATA%`、`%LOCALAPPDATA%` 和用户主目录下的点号目录里找**像会话仓库的文件夹**
（里面有 `sessions/`、`conversations/`、`history/` 这类子目录），并读取其中含 `role`/`content` 的 JSONL/JSON 文件。
命中就会作为一个新 Agent 出现在总览里，会话进入列表、搜索索引和清理名单，可预览、可删除。

- 名称取自文件夹（`com.acme.copilot.stable` → `Copilot`），来源标记为「自动发现」
- 单个文件夹最多取 60 个会话文件，整次扫描最多 120 个，避免拖慢启动
- 明确排除 `Documents`、`Desktop`、`Downloads`、`Program Files` 等用户数据目录，避免误把文档当会话删掉
- 已有专用适配器的 Agent（Codex / DSH / Antigravity / Qoder / WorkBuddy / OpenCode）不会被重复计入
- 如果某个 Agent 用的是数据库或加密格式，仍需要按下面的步骤写专用适配器

## 删除安全（重要）

这一版重新设计了删除路径，因为此前的清理逻辑误删过正常对话：

- **默认一个都不勾选**：打开清理中心时所有分类都是未选中，必须亲手勾选才会进入删除清单
- **不再按体积选人**：已移除「占用超过 5 MB」「大于 20 MB」两个分类。长对话往往最有价值，按大小推荐删除是错的
- 只保留三条低价值规则：超过 30 天未更新、已归档、几乎没有内容（≤ 2 条消息）
- **默认移入回收站**：可以恢复。「永久删除」不再记住上次选择，每次都要重新勾选
- **只读来源不可删**：自动发现但无法完整解析的来源，只做索引展示，不提供删除入口，也不进入清理候选
- **路径护栏**：任何删除都必须落在已知 Agent 目录内，否则直接拒绝

Agent API（`agent/`）的 `delete_sessions` 也已同步加固：强制 `confirm: true`、校验路径在 Agent 目录内、只读来源拒绝删除，并返回 `skipped` 说明每个被拒绝的原因。

## 界面参考

产品结构参考了这些开源项目，功能按本机场景重做，没有直接复用它们的代码：

- [jhlee0409/claude-code-history-viewer](https://github.com/jhlee0409/claude-code-history-viewer)：
  多 Agent 统一浏览、按项目/会话组织、全局搜索、分析面板的布局思路
- [HizTam/codex-history-viewer](https://github.com/HizTam/codex-history-viewer)：
  会话标签、导入导出、恢复命令等功能清单

## 快捷键

- `Ctrl/⌘ + K`：跳到会话搜索
- `1`–`5`：切换 概览 / 会话 / 项目 / 统计 / 设置
- `Esc`：关闭弹窗

## 索引

- 位置：`%APPDATA%\com.agenthub.desktop\index.db`（SQLite，只存可读会话的正文摘要，每条截断 2000 字）
- 时机：每次扫描后在后台线程增量更新，未变化的文件按 (大小, mtime) 跳过
- 大小：本机 107 个会话约 4.8 MB
- 删除这个文件不会丢数据，下次扫描会重建

## 数据与安全

- 归档标记与删除记录：`%APPDATA%\com.agenthub.desktop\state.json`
- 应用不联网，也不写入 Agent 的会话内容；删除是唯一会改动磁盘的操作
- 删除后请不要再依赖原 Agent 恢复该会话

## 打不开 / 双击没反应

- 启动日志：`%LOCALAPPDATA%\com.agenthub.desktop\startup.log`，每次启动写入 `start`、
  `window visible=… minimized=…`，异常时写入 `fatal: …`。
- 应用是**单实例**的：如果已有一个窗口（可能被最小化或压在别的窗口后面），
  再次双击会把已有窗口恢复到前台，而不是开第二个实例。
- 本次修复了「启动即最小化」的问题：窗口创建后会强制 `ShowWindow(SW_SHOWNORMAL)`，
  不再沿用启动器传入的显示状态。
- 若日志出现 `fatal:`，把那一行发回来即可定位；若完全没有日志，说明进程没起来（杀软拦截等）。

## 后续新增 Agent 适配

1. `src-tauri/src/scan.rs`
   - 加常量与 `agent_home()` / `agent_roots()` / `meta()` 分支
   - 写一个 `xxx_rows() -> Vec<Conversation>`：
     - 文件型：填 `path`、`size`、`messages`、`readonly: false`
     - 数据库型：`path` 用 `db#rowid`，`readonly: true`
   - 在 `scan()` 里调用并把行推进 `rows`
2. `web/index.html`：在 sprite 里加一个 `logo-xxx` symbol
3. `web/app.js`：补 `AGENT_LOGO`、`AGENT_TINT`、`AGENT_TINT_SOFT`
4. 重新 `build.cmd`，用 `tools/tauri-e2e.mjs` 跑一遍增删流程

## 自检

- `tools/tauri-check.mjs`：通过 WebView2 调试端口读取界面状态并截图
- `tools/tauri-e2e.mjs`：在真实窗口里跑「扫描 → 预览 → 归档 → 删除 → 索引更新」

两者需要窗口带调试端口启动（默认配置未开启）：

```
set WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223
agent-hub.exe
```

或临时在 `src-tauri/tauri.conf.json` 的窗口配置里加
`"additionalBrowserArgs": "--remote-debugging-port=9223"` 后重新构建。

## 已知限制

- Trae / Qoder / ZCode 的会话格式是专有的（非 SQLite / 非公开 JSONL），当前只识别目录
- 数据库型 Agent 只读，无法在应用内删除
- 预览只读会话尾部，超长会话只显示最近几条
- 永久删除不可恢复
