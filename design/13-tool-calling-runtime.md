# 13 - 工具调用运行时

会话级异步工具与下游消息唤醒的替换契约见
[26-session-tool-runtime.md](26-session-tool-runtime.md)；本章的工具目录与 executor 契约继续适用。

## 13.1 身份与执行

具体工具实现 `pl_core::tool::opaque::Tool`，Registration 转移实例所有权到 Thread。
定义和参数是带 format/version 的不透明载荷，tool 解释参数，model 解释模型声明；core 不解析 JSON。
每次请求冻结同一工具目录与 executor，稳定工具身份和原调用参数进入日志。

动态注册与撤销在 Thread owner 中完成，失败不发布部分目录。重连不改变声明身份，
已冻结调用持有旧执行租约；准入前、执行前和控制提交前仍校验权限未被撤销。
工具不能由名字、MCP annotations 或输出正文取得控制权。结束、扩展 CAS、交互、发现、
任务等待与取消均使用显式注册授权及类型化控制值。

完整 payload 与实际 delivered_context 分开保存。取消或失败保留观察结果，但不得提交迟到控制
或扩展更新。普通工具后台任务及交付见 [26](./26-session-tool-runtime.md)。
Plan/workflow 工具与状态机属于 Studio，不进入 core。

## 13.2 Thread workflow tools

当注册 Mode 携带预设图时，root 获得 `workflow_current`、`workflow_next`、`workflow_graph`、
`workflow_history`、`workflow_transition` 与 `workflow_restart`。查询不修改 revision；transition 以
run/revision/current stage CAS 沿直接边前进；restart 原子归档并按当前图建立新 run。模型没有定义、
编译、patch 或 supersede 图的入口。

Runtime 校验 schema、图、CAS、合法直接边、大小限制与 operation identity；`when`、阶段完成标准和证据
真实性由 Agent 判断。拒绝结果总是返回 canonical snapshot、合法路径与 recovery actions。

稳定拒绝码为 `workflowNotStarted`、`runMismatch`、`staleRevision`、`stateMismatch`、
`modeSnapshotMismatch`、`invalidCompletion`、`invalidReason`、`transitionNotAllowed` 与
`operationIdentityConflict`；成功和幂等重放分别返回 `transitioned | restarted` 与
`alreadyApplied`。

幂等键是 `(turnId, callId, argumentHash)`：完全重放返回 `alreadyApplied`，同一 turn/call 使用不同参数
返回 identity conflict。成功 operation receipt 最近保留 32 条。

## 13.3 原子 checkpoint

状态调用从只读扩展快照计算 CAS 候选。core 将扩展更新、工具完整结果和实际模型上下文在同一
commit 中提交；提交失败保留完成对象用于重试，不再次执行副作用。

transition result 内含最新 stage constraint，供同一 Turn 后续 inference 使用。下一 Turn 从 working
state 派生 `pl.workflow`；context compaction 后重新捕获最新 projection，不复用压缩前阶段。

## 13.4 普通能力

Workflow 阶段不能收缩文件、命令、Git、MCP、Agent 或最终回复工具目录。工具权限继续使用既有通用
approval/sandbox 策略。协作 `spawn_agent` 接受 `profileId` 并冻结 Profile；child 不注册 root 的
workflow 工具，因此不能替 root 改写 run。

`spawn_agent` 对 directory Profile 接受 `writablePaths`；省略、空数组和非空数组分别表示项目内全可写、
只读和目录前缀白名单。所有 Pure 内置文件 mutation 通过中央策略检查，读取与项目外访问不受该字段
扩张或收缩。该策略不是 OS 沙箱，exec、Git 和 MCP 的 schema/固定 prompt 必须明确其可绕过目录限制，
并要求 child 不借这些工具修改白名单外项目文件。

协作工具 schema 按当前启用 Profile 的冻结 `workspace_mode` 生成 `oneOf` 对象分支。每个分支固定
`profileId` 并拒绝额外字段；只有 directory 分支声明 `writablePaths`。schema 约束用于降低模型首次
调用错误，执行路径仍按 Profile snapshot 重做同一语义校验，不能把 schema 当作安全边界。

MCP resource façade 同样属于本轮冻结工具目录。Runtime 必须读取 server 在 initialize/discovery 中
声明的 `resources` capability，只把支持该能力的 server 写入 lease 的 resource assignment；没有任何
此类 server 时不向模型暴露 `list_mcp_resources`、`list_mcp_resource_templates` 或
`read_mcp_resource`。聚合查询只访问 assignment 中的 server，显式指定未声明该能力的 server 必须在
发送请求前返回稳定参数错误，不能用一次预期的 `Method not found` 探测能力，也不能因此把正常 MCP
transport 标记为 unavailable。模型可见 schema 与执行路径必须消费同一冻结 assignment，避免暴露必然
首次失败的工具。

MCP tool executor 必须捕获创建它的 `McpTurnLease`、server identity、raw tool name 与 generation；
旧 `ToolPlan` 即使在新 generation 发布后仍调用旧 lease，最后一个 executor/plan 释放后才能回收旧
连接。远端展示 metadata 不得提升 effect、并行、programmatic、cache 或权限策略。

## 13.5 统一完成

所有 root Mode 在完成请求后调用 `complete`，提交非空 `summary` 和可选、有界的 `evidence` 列表。
工具返回结构化完成事实并结束当前 turn；普通文本不能替代该调用。未选择 workflow 的 Mode 不需要
调用 workflow 工具，但仍必须通过 `complete` 结束。child（包括 `worktree_executor`）保持直接结束，
不得要求其调用 root 专用的 `complete`。

### 文件读取缓存与可见内容

`read_file` 的精确请求可复用同一工作区 epoch 内的结果，但命中仍返回所请求的文件内容，不能只返回
“此前已读取”的摘要。不同的行范围按独立请求处理；模型可能是在输出截断或上下文压缩后补读，
不能把旧范围被覆盖等同于正文仍在模型上下文中。文件 IO 的精确请求去重及变更失效规则保持有效。
协作控制调用可能触发或观察 child 的文件变更，因此使父代理的 workspace 缓存 epoch 前进；
不能跨 child 派发、续跑或交付边界复用旧文件视图。
