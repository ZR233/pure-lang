# 14 - LSP Runtime 设计

## 目标

Pure Studio 的 LSP 支持用于给 agent 提供代码语义查询能力，并在 Flutter chat 状态栏展示当前项目可用的语言服务器。LSP runtime 只存在于本地进程内，不通过 MCP server 暴露，也不把语言服务器配置持久化到用户配置中。

v1 只内置 `rust-analyzer`：

- 项目工作区存在 `Cargo.toml` 时启用 Rust LSP 探测。
- 使用 `rust-analyzer` 命令和 stdio transport。
- 默认不安装任意语言服务器；缺失时记录不可用状态并给 UI/工具返回可读提示。`rust-analyzer` 作为内置 Rust LSP 例外：当 PATH 上存在 rustup，且探测明确返回 rustup 的 `Unknown binary 'rust-analyzer'` 缺失组件错误时，runtime 自动运行 `rustup component add rust-analyzer`，成功后重试探测；rustup 不可用、安装失败或其他启动失败仍只记录不可用状态。
- Windows 下探测和启动语言服务器必须作为后台子进程静默运行，不显示额外终端窗口。

## 架构边界

`pl-lsp` 负责 LSP 协议和运行时：

- 实现 stdio JSON-RPC `Content-Length` framing。
- 维护语言服务器进程、request id、pending response、notification handler。
- 维护 server 快照、打开文档版本、diagnostics 缓存。
- 提供 `LspRuntimeRegistry` 给 `pl-core` 和 Studio 复用。
- 关闭 runtime 时先走 LSP `shutdown` / `exit`，再显式等待子进程退出；超时后按进程树强制终止，Drop 只作为兜底清理。

`pl-core` 负责把 LSP 能力接入通用 turn engine，`pl-studio-runtime` 负责产品生命周期：

- `StudioRuntime` 持有共享 `LspRuntimeRegistry`，项目打开/选择时 reconcile 当前 workspace。
- `StudioAgentTurnFactory` 在准备 `TurnEngine` 时把共享 registry 注入 `TurnEngineBuilder`。
- LSP 查询工具按语言拆分为独立工具（如 `lsp_query_rust`），父 agent 和 subagent 共用同一 registry。工具列表在每轮准备 TurnEngine 并注册默认工具时，根据当前可用语言同步；对应 LSP 服务器不可用时不会暴露给 LLM。
- `lsp_query_*` 的 `filePath` 在 `pl-core` 中复用工具统一路径策略解析：相对路径按 `workspaceRoot` 解释，workspace-only 模式拒绝越界，交给 `pl-lsp` 前必须已经是规范化绝对路径。
- 文件写入、patch、move/delete 成功后通知 LSP runtime 同步已打开文档。

`pure-studio` 只负责展示和事件订阅：

- `ThreadRuntimeSnapshot.activeLspServers` 表示当前 Thread 可用的 LSP server 名称。
- 初始 `ThreadSnapshot` 与后续 `ThreadRuntimeUpdated` 都携带完整 active LSP 列表，避免启动、切换或重订阅时状态栏空白。
- `lspHealthChanged` 事件同步完整 LSP server snapshot 和 active 列表。
- 状态栏能力弹层展示 Skills、MCP、LSP 三组。

## 状态模型

LSP server 快照包含：

- `id`：稳定 server id，例如 `rust-analyzer`。
- `displayName`：UI 展示名称。
- `availabilityKind`：`checking`、`available`、`unavailable`、`missingCommand`、`disabled`。
- `availabilityMessage`：缺失命令、无 Rust 工作区、启动失败等说明。
- `extensions` / `languageIds`：路由和展示用。
- `diagnosticCount`：当前缓存诊断数量。
- `activityKind`：`idle`、`busy`、`indexing`，表示语言服务器当前是否有前台工作。
- `activityTitle` / `activityMessage` / `activityPercentage`：来自 LSP `window/workDoneProgress/create` 和 `$/progress` 的当前工作摘要，用于状态栏展示 `索引中` 等状态。
- `lastError` / `lastErrorAt`：最近一次 LSP server error 或语言服务器 stderr warn/error 行，仅用于诊断展示，不写入 timeline。

active LSP 只包括 `available` server。`missingCommand`、`unavailable`、`disabled` 仍可在 UI 中展示提示，但不计入 active 数。

状态栏能力弹层中的 LSP 列表展示所有 snapshot。不可用状态优先于运行状态；available server 若存在 `indexing` 或 `busy` activity，则显示对应活动状态，否则显示就绪和诊断数量。

## 工具能力

每种可用的 LSP 语言会自动注册为独立的工具，命名为 `lsp_query_{language_id}`（如 `lsp_query_rust`）。工具名直接告诉 LLM 当前可用的语言；不可用时工具不会出现在会话中。

语言工具执行时会把内部 `languageId` 注入到 `LspQuery`，`pl-lsp` 优先按该语言 ID 路由到可用服务器；没有语言 ID 的兼容查询才回退到文件扩展名或默认 active server 路由。

每个语言工具支持：

- `goToDefinition`
- `findReferences`
- `hover`
- `documentSymbol`
- `workspaceSymbol`
- `goToImplementation`
- `prepareCallHierarchy`
- `incomingCalls`
- `outgoingCalls`
- `diagnostics`

位置类输入使用 1-based `line` / `character`，内部转换为 LSP 0-based UTF-16 position。输出为结构化 JSON 文本，包含 `success`、`operation`、`serverId`、`result`、`resultCount`、`fileCount`。

当 `lsp_query_rust`（或其他语言对应的 LSP 工具）可用且 active LSP 支持目标文件时，agent 应优先用它处理代码语义查询，包括定义跳转、引用查找、hover 类型/签名/文档、实现跳转、符号查询、调用层级和 diagnostics。纯文本匹配或配置搜索回退到 `exec` + `rg`，文件名搜索回退到 `exec` + `rg --files`；非支持语言、LSP 未激活或 LSP 返回不可用错误时使用相同回退，当前平台没有 ripgrep 时再使用等价的平台命令。

## 文件同步

查询前，runtime 会读取目标文件并发送 `textDocument/didOpen` 或 `textDocument/didChange`。文件写入、patch、copy、move、delete 成功后，`pl-core` 会把受影响路径通知 runtime；runtime 只同步已打开且受支持的文件。

`rust-analyzer` 初始化时固定使用 client-side file watcher 配置，并声明支持 `workspace/didChangeWatchedFiles` 动态注册。runtime 不让 `rust-analyzer` 自行启动服务端文件监听；当 agent 写入、移动或删除文件时，已启动的 LSP client 会收到对应 watched-files 通知，同时对已打开且受支持的源码文件继续发送 text document 同步。

`pl-lsp` 只接受已解析的绝对路径来生成 LSP file URI，不依赖 `std::env::current_dir()` 兜底。Windows 下 `std::fs::canonicalize` 可能返回 `\\?\` 或 `\\?\UNC\` verbatim path；runtime 生成 LSP file URI 前必须转回普通 drive/UNC 路径，避免向语言服务器发送包含 `%3F` 的无效 URI。

`rust-analyzer` 可能在索引期间返回 `ContentModified` 错误 `-32801`，runtime 对该错误做最多 3 次指数退避重试。

## 非目标

v1 不实现 Claude Code 的插件市场 LSP 配置、LSP 推荐 UI、终端 Ink 展示、IDE 虚拟 URI 或 MCP 诊断基线；除内置 `rust-analyzer` 的 rustup component 自愈外，不自动安装任意语言服务器。
