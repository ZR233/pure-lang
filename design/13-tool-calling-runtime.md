# 13 - 工具调用运行时

## 13.1 身份与执行

每个工具调用携带 typed `ToolCallIdentity`，由当前 TurnId、provider item/call identity 组成。工具目录、
参数 schema、审批策略和 `ToolBatchPolicy` 在 inference 开始时冻结为代际快照。未知工具、参数解析、
审批拒绝与执行失败都形成 typed tool result，不伪造成功。

运行时唯一执行容器是 `DynTool(Arc<dyn ToolExecutor>)`。`ToolExecutor` 是对象安全的动态边界，同时
提供冻结 definition、可信 policy、execution owner 与 boxed execute future；registry、`ToolBinding`
和 `ToolPlan` 不保存来源枚举或具体工具类型。静态 Rust 工具通过 `StaticTool` 的关联 `Input` 保留
typed 参数和 RPITIT future，再由 `From` adapter 生成 `DynTool`。`StaticTool::definition` 必须返回
已经持有 `ToolName` 的 `StaticToolDefinition`，`From` 不承担可失败校验。Schemars 从 `Input` 的类型、
rustdoc 和属性生成参数 schema；adapter 使用同一类型反序列化调用参数，复杂业务不变量仍由领域逻辑验证。

`pl-core` 在 crate 根公开 `StaticTool`、`static_tool` builder、`DynTool`、`ToolExecutor`、
`ToolInstallGroup` 以及可复用内置工具的实现类型和构造函数。下游按所需能力自由选择内置工具，逐个
`.into()` 后与宿主工具放入同一安装组；`lsp_tools`、`command_tool_pair` 等工具族构造器直接返回
`Vec<DynTool>`。默认工具安装器只是公共工具的预设组合，不拥有私有工具类型，也不是第二条注册路径。

工具注册以 `ToolInstallGroup` 为原子单位，组同时拥有 exposure、可选 developer instructions、
generation 和 `DynTool` 列表。冻结快照只向模型发送 Direct 和当前 session 已 reveal 的 Deferred
工具；若目录仍有 Deferred 工具，同时暴露普通 `tool_search`。搜索只返回当前冻结目录中的命中，
通过 typed `RevealTools` directive 令下一次 inference 重新冻结；当前 inference 的 executor 集合不会
原地改变。目录 fingerprint 包含 deferred definition 与 source generation，变化时旧 reveal 失效。

`Coexist` 工具可按普通 batch 执行；包含 `Solo` 工具的 response 必须恰好只有该一个调用，否则整批在
任何工具执行前拒绝。该规则通用，不依赖工具名；`workflow_state` 与 `complete` 都使用 `Solo`。

## 13.2 workflow_state

当活动 Mode Skill 选择工作流时，状态工具支持：

- `compile`：在无 workflow 或前一 run 已 terminal 时编译新定义并冻结 Mode Skill；
- `status`：读取 current、graph 或 history，不修改 revision；
- `transition`：以 run/revision/current stage CAS 沿一条直接边前进；
- `supersede`：先完整编译 replacement，再原子归档 active run 并创建同 lineage 新 run。

Runtime 校验 schema、图、CAS、合法直接边、大小限制与 operation identity；`when`、阶段完成标准和证据
真实性由 Agent 判断。拒绝结果总是返回 canonical snapshot、合法路径与 recovery actions。

稳定拒绝码为 `invalidDefinition`、`workflowNotCompiled`、`activeWorkflowExists`、
`terminalWorkflow`、`runMismatch`、`staleRevision`、`stageMismatch`、`unknownTargetStage`、
`transitionNotAllowed`、`operationIdentityConflict`。

幂等键是 `(turnId, callId, argumentHash)`：完全重放返回 `alreadyApplied`，同一 turn/call 使用不同参数
返回 identity conflict。成功 operation receipt 最近保留 32 条。

## 13.3 原子 checkpoint

状态调用先在 working-state clone 上运行。一个 checkpoint 同时持久化 assistant tool-call Item、tool
result Item 和更新后的 `AgentWorkingState`；提交失败不发布任何一个新事实，也不消费 revision。

transition result 内含最新 stage constraint，供同一 Turn 后续 inference 使用。下一 Turn 从 working
state 派生 `pl.workflow`；context compaction 后重新捕获最新 projection，不复用压缩前阶段。

## 13.4 普通能力

Workflow 阶段不能收缩文件、命令、Git、MCP、Agent 或最终回复工具目录。工具权限继续使用既有通用
approval/sandbox 策略。协作 `spawn_agent` 接受 `profileId` 并冻结 Profile；child 不注册 root 的
`workflow_state`，因此不能替 root 改写 run。

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
调用 `workflow_state`，但仍必须通过 `complete` 结束。child（包括 `worktree_executor`）保持直接结束，
不得要求其调用 root 专用的 `complete`。
