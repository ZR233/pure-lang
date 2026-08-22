# 14 - LSP Runtime 设计

## 目标

Pure Studio 的 LSP 为 agent 提供代码语义查询，并向 Flutter 展示当前 Project 的 last-known
语言服务器状态。LSP runtime 只存在于本地进程，不通过 MCP 暴露。server 定义是数据驱动的
catalog：内置 catalog 收录已知 server（当前只有 rust-analyzer 一条），用户可在配置中声明
自定义 server；新增语言支持只需新增 catalog 条目、driver 实现或用户配置，不需要修改
`pl-core`，也不存在语言名字面量或按语言的分支。

## Server catalog 与 driver

`LspServerDefinition` 是纯数据（serde camelCase）：声明 server id、展示名、language ids、
workspace 检测规则（相对 workspace root 的文件名或单段 glob，空列表表示总是匹配）、
command 解析（program + args 模板，占位符当前仅支持 `{workspaceRoot}`）与能力集（支持的
`lsp_query` 操作子集，用于 capabilities 报告与路由校验）。catalog 由内置定义与用户在
`~/.pure/config.toml` `[lsp.servers.<id>]` 段的声明合并而成（配置面见 `10-config.md`）；
重复 server id 或 language id 冲突在配置解析时以 typed 错误 fail-loud。同一 language id
被多个 server 声明且都匹配 workspace 时，路由以 typed `LspRoutingError::AmbiguousLanguage`
拒绝并列出候选，不按注册顺序或名称猜测；零匹配返回列出可用语言的 unknown language 错误。

`LspServerDriver` 是 server 生命周期的唯一 adapter 边界：环境探测（typed 就绪/缺失原因）、
修复（repair）、进程启动参数解析与 server 特殊初始化（如 rust-analyzer 的 client watcher
配置）由具体 driver 提供；连接、传输与请求转发由通用 client 层实现。catalog 需要运行期
开放扩展（内置、用户配置与宿主自定义共存），driver 以 `dyn` 分发，future 使用 boxed
形态，不使用 `#[async_trait]`。rust-analyzer 的 rustup probe、`missingServerComponent`
判定与 `rustup component add` 修复全部封在 `RustAnalyzerDriver` 内；用户声明的自定义
server 绑定通用 `CommandDriver`（`<command> --version` 探测，无可修复组件语义）。
`pl-lsp` 的 registry 与路由层不包含任何语言专项逻辑。

## Owner 与 CQS

`LspRuntimeRegistry` 是进程、连接、handler、diagnostics、activity 和 snapshot 的唯一 owner，
提供六组边界：

```text
reconcileWorkspaceMembership(project)
probeLspServer(scope)
repairLspServer(serverId)
resetLsp(scope)
readLspState()
shutdownLsp()
```

scope 为单 server、单 workspace 或 All。membership 只规范化 workspace root、按 catalog
检测规则静态判定 server 适用性（检测未命中的条目保留为 Disabled member 供 UI 展示原因）、
增删 server 定义并清理 stale client；不得执行 `--version`、rustup、网络请求
或启动语言服务器。read 只克隆 owner 已发布的 snapshot。

probe 才运行 driver 的环境探测（如 `rust-analyzer --version`）。rustup 组件缺失时 driver
发布 typed `missingServerComponent`（携带组件标签与修复说明），不自动安装。repair 只接受
该状态，委托对应 driver 修复（rust-analyzer 执行 `rustup component add rust-analyzer`），
成功后重新 probe；其他不可用状态拒绝 repair。

LSP query 可以按需启动已确认 available 的 client，但不能重新 probe。启动失败必须回写 registry
availability/error 并发布 `LspStateChanged`。reset 对目标 client 执行 LSP shutdown/exit，清理
diagnostics、activity 与 handlers；重置前已启动则立即重启，未启动则回到 available/unstarted。
reset 不关闭 registry。shutdown 是不可恢复终止态，拒绝后续 membership/probe/repair/reset/start，
但允许读取 stopped snapshot。

## 并发与进程

membership、probe、repair、reset 与 shutdown 通过 registry lifecycle 锁串行化；状态锁内不等待
probe、rustup、client 初始化或子进程退出。异步操作捕获 operation id、desired revision 与
fingerprint，过期结果不得覆盖新状态。workspace/server 删除时先从状态中原子移除 owner，再在
生命周期锁外等待子进程关闭。

所有 probe、repair 和 server process 统一经过 `pl-lsp` 后台进程工厂。Windows 必须使用
`CREATE_NO_WINDOW` 并把进程树放入 Job Object；Unix 使用独立 process group。关闭时先走 LSP
`shutdown` / `exit`，再等待完整子进程树，超时后强制终止并等待。Drop 只作兜底。

## 状态模型

`LspStateSnapshot` 使用 `ObservedResource<StudioLspHealth>`：Loading/Failed 没有可用 payload，
Refreshing/Degraded 明确保留 last-known health，Stale 表示 desired membership 已变化。每个 server
记录稳定 id/display name 与 extensions/language ids，并以精确 `StudioLspServerState` 表达
Checking、Available、Unavailable、Disabled。Missing command 与 missing server component 是
Unavailable 内的 typed error code，不再形成平行 availability/message 字段。

只有 Available 承载 checked time、diagnostic count 与 `Idle | Busy | Indexing` activity union；
title/message/percentage 只存在于 Busy/Indexing。Unavailable 承载 checked time 与 typed error，
Checking/Disabled 只承载说明。active LSP 只包括 Available server，其余状态仍可展示但不计入 active。

## 工具能力

LSP 以能力 seam 模式接入工具注册表：workspace 存在可用 server 时，LSP 来源发布两个
deferred 工具——`lsp_capabilities` 与 `lsp_query`；不再存在按语言命名的
`lsp_query_{language_id}` 工具。`lsp_capabilities` 由 catalog × workspace 检测 × 运行态动态产出当前 workspace 的
server、language id、支持的操作与就绪状态；`lsp_query` 接收 `languageId`、operation
（definition、references、hover、document/workspace symbol、implementation、call hierarchy、
diagnostics）与查询参数，运行期按 catalog 路由到对应 server，能力集外的操作被路由层拒绝。父 agent 与 subagent 共用 registry。
输入路径先经过 workspace-only 绝对路径解析；位置使用 1-based line/character，内部转换为
LSP 0-based UTF-16。

查询前 runtime 发送 didOpen/didChange；文件工具写入、move/delete 后通知已启动 client，并发送
watched-files 通知。Windows verbatim path 在生成 URI 前转回普通 drive/UNC。ContentModified 和
启动期空结果只做有界重试，不伪造 didChange。

`ThreadRuntimeSnapshot.activeLspServers` 表示当前 Turn 实际冻结的 server；产品级完整状态只通过
`readLspState`/`LspStateChanged`。页面刷新、Studio snapshot、Turn 创建和工具查询都不得隐式
probe 或 repair。

Flutter 的 LSP 设置页只投影产品级完整 sealed state。页面进入和“刷新”调用 `readLspState`；Project
probe、仅在 Unavailable 的 `lspComponentMissing` typed code 时可用的 repair，以及 workspace/server
reset 分别调用对应 typed command。Widget 不从错误字符串推断 availability，也不把 shutdown 当作 reset。

server activity（idle/busy/indexing 及 title/message/percentage）随同一 snapshot 与
`LspStateChanged` 事件流投影到 Flutter：设置页 LSP 行是权威展示，activity 非 idle 时显示
活动状态与进度；主状态栏在任一 server 非 idle 时显示轻量活动指示，数据同样取自
`readLspState`/事件流。两者都是纯投影，不隐式触发 probe、repair 或 server 启动。

## 非目标

不实现插件市场 LSP 推荐 UI、终端展示或 IDE 虚拟 URI；除用户在配置中显式声明的自定义
server 外，不自动安装任何语言服务器。rust-analyzer 的 rustup 安装必须由用户明确 repair
command 触发。
