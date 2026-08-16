# 14 - LSP Runtime 设计

## 目标

Pure Studio 的 LSP 为 agent 提供代码语义查询，并向 Flutter 展示当前 Project 的 last-known
语言服务器状态。LSP runtime 只存在于本地进程，不通过 MCP 暴露。server 定义是数据驱动的
catalog：内置 catalog 收录已知 server（当前只有 rust-analyzer 一条），用户可在配置中声明
自定义 server；新增语言支持只需新增 catalog 条目、driver 实现或用户配置，不需要修改
`pl-core`，也不存在语言名字面量或按语言的分支。

## Server catalog 与 driver

`LspServerDefinition` 是纯数据：声明 server id、展示名、language ids、workspace 检测规则
（文件名/glob）、command 解析策略与能力集。catalog 由内置定义与用户配置声明合并而成；
同一 language id 被多个 server 声明且都匹配 workspace 时，路由以 typed 歧义错误拒绝，
不按注册顺序或名称猜测。

`LspServerDriver` 是 server 生命周期的唯一 adapter 边界：环境探测与修复、进程启动/关闭、
请求转发都由具体 driver 实现。rust-analyzer 的 rustup probe 与 `MissingRustupComponent`
修复等内容全部封在 `RustAnalyzerDriver` 内；`pl-lsp` 的 registry 与路由层不包含任何语言
专项逻辑。

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

scope 为单 server、单 workspace 或 All。membership 只规范化 workspace root、检查静态
`Cargo.toml` 特征、增删 server 定义并清理 stale client；不得执行 `--version`、rustup、网络请求
或启动语言服务器。read 只克隆 owner 已发布的 snapshot。

probe 才运行 `rust-analyzer --version`。探测到 rustup `Unknown binary 'rust-analyzer'` 时发布 typed
`MissingRustupComponent`，不自动安装。repair 只接受该状态，执行
`rustup component add rust-analyzer`，成功后重新 probe；其他不可用状态拒绝 repair。

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

`LspStateSnapshot` 包含公共 `ObservedStateMeta`、project memberships 和完整 server snapshots。
server 记录稳定 id/display name、project id、definition fingerprint、availability、是否已启动、
extensions/language ids、diagnostic count、activity、last error 与 checked time。availability 至少区分
uninitialized、checking、available、missing command、missing rustup component、unavailable、
disabled 和 stopped。

失败保留最后一次成功 payload 并标 stale；首次失败使用 authoritative empty。active LSP 只包括
available server，unavailable/disabled/stopped 仍可在 UI 显示但不计入 active 数。

## 工具能力

LSP 以能力 seam 模式接入工具注册表：workspace 存在可用 server 时，LSP 来源发布两个
deferred 工具——`lsp_capabilities` 与 `lsp_query`；不再存在按语言命名的
`lsp_query_{language_id}` 工具。`lsp_capabilities` 动态返回当前 workspace 可用的 server、
language id、支持的操作与就绪状态；`lsp_query` 接收 `languageId`、operation（definition、
references、hover、document/workspace symbol、implementation、call hierarchy、diagnostics）
与查询参数，运行期按 catalog 路由到对应 server。父 agent 与 subagent 共用 registry。
输入路径先经过 workspace-only 绝对路径解析；位置使用 1-based line/character，内部转换为
LSP 0-based UTF-16。

查询前 runtime 发送 didOpen/didChange；文件工具写入、move/delete 后通知已启动 client，并发送
watched-files 通知。Windows verbatim path 在生成 URI 前转回普通 drive/UNC。ContentModified 和
启动期空结果只做有界重试，不伪造 didChange。

`ThreadRuntimeSnapshot.activeLspServers` 表示当前 Turn 实际冻结的 server；产品级完整状态只通过
`readLspState`/`LspStateChanged`。页面刷新、Studio snapshot、Turn 创建和工具查询都不得隐式
probe 或 repair。

Flutter 的 LSP 设置页只投影产品级完整状态。页面进入和“刷新”调用 `readLspState`；Project
probe、仅在 `missingRustupComponent` 时可用的 repair，以及 workspace/server reset 分别调用
对应 typed command。Widget 不从错误字符串推断 availability，也不把 shutdown 当作 reset。

## 非目标

不实现插件市场 LSP 推荐 UI、终端展示或 IDE 虚拟 URI；除用户在配置中显式声明的自定义
server 外，不自动安装任何语言服务器。rust-analyzer 的 rustup 安装必须由用户明确 repair
command 触发。
