# 21 - LSP Runtime

## 21.1 目标与边界

anywork 的 LSP 为 agent 提供代码语义查询，并向 Flutter 展示当前 Project 的 last-known 语言
服务器状态。LSP runtime 只存在于本地进程，不通过 MCP 暴露。server 定义是数据驱动的
catalog：内置 catalog 收录已知 server（当前只有 rust-analyzer 一条），用户可在配置中声明
自定义 server（配置面见 [20](./20-config.md)）；新增语言支持只需新增 catalog 条目、driver
实现或用户配置，不需要修改 pl-core，也不存在语言名字面量或按语言的分支。

## 21.2 职责划分

pl-lsp 按变化原因组织为六个稳定公开域：catalog（server 定义、用户配置合并、workspace
匹配与内置条目）、driver（环境探测、repair、command 解析与 server 专项初始化）、host
（workspace 文件/进程宿主边界与本地进程树托管）、client（单个 server 连接及其配置、文档
同步、诊断、消息分发、RPC、状态、传输和 URI 边界）、query（查询、位置、诊断与查询结果
等纯领域合同）、runtime（错误、状态、capabilities 与唯一 registry owner）。依赖方向由
query/runtime 纯合同与 catalog/driver/host 边界指向 client，再由 runtime 组合；client
不反向依赖 registry 编排；实现子模块保持私有，旧 crate 根类型路径不保留别名。

## 21.3 Server catalog 与 driver

server 定义是纯数据（serde camelCase）：声明 server id、展示名、language ids、workspace
检测规则（相对 workspace root 的文件名或单段 glob，空列表表示总是匹配）、command 解析
（program + args 模板，占位符当前仅支持 `{workspaceRoot}`）与能力集（支持的 `lsp_query`
操作子集，用于 capabilities 报告与路由校验）。重复 server id 或 language id 冲突在配置
解析时以类型化错误 fail-loud。同一 language id 被多个 server 声明且都匹配 workspace 时，
路由以类型化的歧义错误拒绝并列出候选，不按注册顺序或名称猜测；零匹配返回列出可用语言
的 unknown language 错误。未显式提供 language id 的文件路径路由遵循同一原则：扩展名零
匹配返回 unavailable，唯一匹配返回对应 server，多个 server 声明同一扩展名时以类型化的
路径歧义错误拒绝。

driver 是 server 生命周期的唯一 adapter 边界：环境探测（类型化就绪/缺失原因）、修复
（repair）、进程启动参数解析与 server 特殊初始化（如 rust-analyzer 的 client watcher
配置）由具体 driver 提供；连接、传输与请求转发由通用 client 层实现。catalog 支持运行期
开放扩展（内置、用户配置与宿主自定义共存），driver 动态分发且 future 显式 Send，不使用
async-trait 宏。rust-analyzer 的 rustup 探测、`missingServerComponent` 判定与
`rustup component add` 修复全部封在 rust-analyzer driver 内；用户声明的自定义 server
绑定通用命令 driver（`<command> --version` 探测，无可修复组件语义）。registry 与路由层
不包含任何语言专项逻辑。

## 21.4 Registry 与 CQS

registry 是进程、连接、handler、diagnostics、activity 和 snapshot 的唯一 owner，提供六组
边界：

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
增删 server 定义并清理 stale client；不得执行 `--version`、rustup、网络请求或启动语言
服务器。read 只克隆 owner 已发布的 snapshot。

probe 才运行 driver 的环境探测（如 `rust-analyzer --version`）。rustup 组件缺失时 driver
发布类型化的 `missingServerComponent`（携带组件标签与修复说明），不自动安装。repair 只
接受该状态，委托对应 driver 修复（rust-analyzer 执行 `rustup component add
rust-analyzer`），成功后重新 probe；其他不可用状态拒绝 repair。

LSP query 可以按需启动已确认 available 的 client，但不能重新 probe。启动失败必须回写
registry availability/error 并发布 `LspStateChanged`。reset 对目标 client 执行 LSP
shutdown/exit，清理 diagnostics、activity 与 handlers；重置前已启动则立即重启，未启动则
回到 available/unstarted；reset 不关闭 registry。shutdown 是不可恢复终止态，拒绝后续
membership/probe/repair/reset/start，但允许读取 stopped snapshot。

## 21.5 并发与进程

membership、probe、repair、reset 与 shutdown 通过 registry lifecycle 锁串行化；状态锁内
不等待 probe、rustup、client 初始化或子进程退出。异步操作捕获 operation id、desired
revision 与 fingerprint，过期结果不得覆盖新状态。workspace/server 删除时先从状态中原子
移除 owner，再在生命周期锁外等待子进程关闭。

所有 probe、repair 和 server process 统一经过 pl-remote-helper 的后台进程工厂（见
[05](./05-conventions.md)）：Windows 不弹出命令行窗口并把进程树放入 Job Object；Unix 使用
独立 process group。关闭时先走 LSP `shutdown` / `exit`，再等待完整子进程树，超时后强制
终止并等待；Drop 只作兜底。

## 21.6 状态模型

状态快照使用观察资源语义：Loading/Failed 没有可用 payload，Refreshing/Degraded 明确保留
last-known health，Stale 表示 desired membership 已变化。每个 server 记录稳定
id/display name 与 extensions/language ids，并以穷尽的服务器状态表达 Checking、
Available、Unavailable、Disabled。Missing command 与 missing server component 是
Unavailable 内的类型化错误码，不形成平行 availability/message 字段。

只有 Available 承载 checked time、diagnostic count 与 `Idle | Busy | Indexing` activity
union；title/message/percentage 只存在于 Busy/Indexing。Unavailable 承载 checked time 与
类型化错误，Checking/Disabled 只承载说明。active LSP 只包括 Available server，其余状态
仍可展示但不计入 active。

## 21.7 工具能力

LSP 以能力 seam 模式接入目标 agent 的工具集：workspace 存在 available server 时，原子
注册普通 eager 工具 `lsp_capabilities` 与 `lsp_query`；不存在按语言命名的
`lsp_query_{language_id}`，也不存在 deferred namespace。两个工具在构造时捕获同一个
registry、workspace 与路径 resolver，不通过通用调用上下文查找 LSP。`lsp_capabilities`
由 catalog × workspace 检测 × 运行态动态产出当前 workspace 的 server、language id、
支持的操作与就绪状态；`lsp_query` 接收 languageId、operation（definition、references、
hover、document/workspace symbol、implementation、call hierarchy、diagnostics）与查询
参数，运行期按 catalog 路由到对应 server，能力集外的操作被路由层拒绝。父 agent 与
subagent 可以共用 registry owner，但必须分别在自己的工具集中注册 seam；一个 agent 的
工具更新不得改变另一个 agent 已冻结的工具计划。输入路径先经过 workspace-only 绝对路径
解析；位置使用 1-based line/character，内部转换为 LSP 0-based UTF-16。

查询前 runtime 发送 didOpen/didChange；文件 backend 在构造时捕获 registry，写入、
move/delete 后通知已启动 client，并发送 watched-files 通知。Windows verbatim path 在生成
URI 前转回普通 drive/UNC。ContentModified 和启动期空结果只做有界重试，不伪造 didChange。

runtime snapshot 的 activeLspServers 表示当前 Turn 实际冻结的 server；产品级完整状态只
通过 readLspState 与 `LspStateChanged` 事件流。页面刷新、Studio snapshot、Turn 创建和
工具查询都不得隐式 probe 或 repair。

Flutter 的 LSP 设置页只投影产品级完整 sealed state：页面进入和"刷新"调用 readLspState；
Project probe、仅在 Unavailable 的 `lspComponentMissing` 类型化错误时可用的 repair，以及
workspace/server reset 分别调用对应 typed command。Widget 不从错误字符串推断
availability，也不把 shutdown 当作 reset。server activity（idle/busy/indexing 及
title/message/percentage）随同一 snapshot 与事件流投影到 Flutter：设置页 LSP 行是权威
展示，activity 非 idle 时显示活动状态与进度；主状态栏在任一 server 非 idle 时显示轻量
活动指示。两者都是纯投影，不隐式触发 probe、repair 或 server 启动。

## 21.8 非目标

不实现插件市场 LSP 推荐 UI、终端展示或 IDE 虚拟 URI；除用户在配置中显式声明的自定义
server 外，不自动安装任何语言服务器。rust-analyzer 的 rustup 安装必须由用户明确 repair
command 触发。
