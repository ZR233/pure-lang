# Pure Studio LSP Runtime 设计

## 目标

Pure Studio 的 LSP 支持用于给 agent 提供代码语义查询能力，并在 chat 状态栏展示当前项目可用的语言服务器。LSP runtime 只存在于本地进程内，不通过 MCP server 暴露，也不把语言服务器配置持久化到用户配置中。

v1 只内置 `rust-analyzer`：

- 项目工作区存在 `Cargo.toml` 时启用 Rust LSP 探测。
- 使用 `rust-analyzer` 命令和 stdio transport。
- 不自动安装缺失命令；缺失时记录不可用状态并给 UI/工具返回可读提示。

## 架构边界

`pl-lsp` 负责 LSP 协议和运行时：

- 实现 stdio JSON-RPC `Content-Length` framing。
- 维护语言服务器进程、request id、pending response、notification handler。
- 维护 server 快照、打开文档版本、diagnostics 缓存。
- 提供 `LspRuntimeRegistry` 给 `pl-core` 和 Studio 复用。

`pl-core` 负责把 LSP 能力接入 agent：

- `StudioRuntime` 持有共享 `LspRuntimeRegistry`，项目打开/选择时 reconcile 当前 workspace。
- `PureCore` 通过 `with_lsp_runtime` 接收共享 registry。
- `lsp_query` 是只读工具，父 agent 和 subagent 共用同一 runtime。
- 文件写入、patch、move/delete 成功后通知 LSP runtime 同步已打开文档。

`pure-studio` 只负责展示和事件订阅：

- `SessionRuntimeDto.activeLspServers` 表示当前项目 active 的 LSP server 名称。
- `BootstrapDto` / `ProjectSelectionDto` 携带一次性 LSP health 快照，避免启动或切换项目时错过首个探测事件。
- `studio-lsp-health-updated` 事件同步 LSP 快照和 active 列表。
- 状态栏能力弹层展示 Skills、MCP、LSP 三组。

## 状态模型

LSP server 快照包含：

- `id`：稳定 server id，例如 `rust-analyzer`。
- `displayName`：UI 展示名称。
- `availabilityKind`：`checking`、`available`、`unavailable`、`missingCommand`、`disabled`。
- `availabilityMessage`：缺失命令、无 Rust 工作区、启动失败等说明。
- `extensions` / `languageIds`：路由和展示用。
- `diagnosticCount`：当前缓存诊断数量。

active LSP 只包括 `available` server。`missingCommand`、`unavailable`、`disabled` 仍可在 UI 中展示提示，但不计入 active 数。

## 工具能力

`lsp_query` 支持：

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

## 文件同步

查询前，runtime 会读取目标文件并发送 `textDocument/didOpen` 或 `textDocument/didChange`。文件写入、patch、copy、move、delete 成功后，`pl-core` 会把受影响路径通知 runtime；runtime 只同步已打开且受支持的文件。

`rust-analyzer` 可能在索引期间返回 `ContentModified` 错误 `-32801`，runtime 对该错误做最多 3 次指数退避重试。

## 非目标

v1 不实现 Claude Code 的插件市场 LSP 配置、LSP 推荐 UI、终端 Ink 展示、IDE 虚拟 URI、MCP 诊断基线或自动安装语言服务器。
