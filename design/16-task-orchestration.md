# Thread Mode 与预设工作流

## 16.1 单一会话框架

Studio 不再拥有 Simple/Task 两套运行时。所有 root Thread 使用相同的 Thread、Turn、model、tool、
store 与 collaboration 框架；模式是 root Thread 的注册配置。`ThreadModeId` 是动态字符串，内置值为
`mode.simple` 与 `mode.task`，未来自定义模式仍使用 `mode.<id>`。

工作流只影响模型提示和合法状态边，不关闭文件、命令、Git、Agent 或最终回复能力；是否使用工作流
由注册 Mode 是否携带图决定。所有 root Mode 统一通过 `complete` 工具提交完成事实并结束 turn。产品不再拥有
TaskRun、WorkUnit、Completion、ReviewRound、MergeRecord、TaskIssue、worktree、completion gate
或 recovery 状态。

## 16.2 Thread Mode 注册

Mode ID 必须使用 `mode.` 前缀。注册输入显式携带展示名称、描述、排序、Mode Prompt 与可选预设图；
它不经过 Skill frontmatter、调用策略或 Provider。

`mode.simple`、`mode.task` 由 Studio 内置来源以 Rust 常量注册，其他来源不能覆盖。其他来源之间也不设
winner precedence：重复 ID 使整个注册批次失败。Thread Mode 不进入普通 `skills_list`/`skill_view`。

每个 root Turn 捕获一份不可变 Mode 快照。Prompt 始终使用该快照的最新正文；run 只保存 Mode ID 与图
hash。图 hash 变化时下一个 Turn 自动归档并建立 replacement；Prompt-only 更新不重建 run。模式切换
要求 root idle 且没有 pending interaction，runtime 负责归档旧活动 run。

## 16.3 持久化状态

`AgentWorkingState.workflow` 保存 `WorkflowSessionState`：单调 revision、当前 run、最近 16 个归档摘要、
滚动摘要以及最近 32 个成功 operation receipt。当前 run 保存 lineage/run id、Mode ID、图 hash、
active/terminal、当前 state、时间和最近 64 条 transition，不保存完整 definition 或 Prompt。

完整 typed state 最大 256 KiB。图由本 Turn 的 Mode 快照提供，旧细节由 Thread timeline/trace 审计，热状态只保存有界尾部。
模型上下文不直接暴露完整 JSON；`AgentSession::working_context_snapshot` 派生保留 id `pl.workflow`，
只包含当前 state 指令、完成标准、允许边、最近摘要和下一次 CAS 参数。

## 16.4 注册时图编译

定义包含 `title`、`goal`、`initialStateId`、`states` 与 `transitions`。最多 32 个 state、96 条边，
规范化 JSON 最大 64 KiB。state ID 只允许小写 ASCII、数字、`-`、`_`，长度 1–64。非终态必须有
指令、完成标准与出边；终态禁止出边。每个 state 显式携带穷尽的 `kind: atomic | final`；每条边携带
`sourceStateId`、`targetStateId` 与声明性自然语言 `guard`。允许自环、循环和返工；同一 source/target
只能出现一次。

Mode Manager 在发布注册批次前调用纯编译器。编译器先做 schema/大小/唯一性检查，再校验端点与初态；从初态正向遍历拒绝不可达节点，从所有
终态反向遍历拒绝无法终结的非终态。成功后保留声明顺序用于 UI，以 canonical JSON 计算稳定 hash；
数组顺序是 definition 的一部分，相同规范化定义必然得到相同 hash。编译复杂度为 `O(V + E)`。

## 16.5 Workflow 工具

root-only 工具拆为 `workflow_current`、`workflow_next`、`workflow_graph`、`workflow_history`、
`workflow_transition` 和 `workflow_restart`。前四者只读；`workflow_transition` 表示完成当前阶段并原子
进入直接相邻阶段，`workflow_restart` 归档当前 run 并按同一预设图开始新 run。任何工具都不接受
`definition`、`compile` 或 `supersede` 输入。

`guard` 与 completion criteria 是 Agent 的提示约束，Runtime 不推断外部工作是否完成。所有语义拒绝
返回 `accepted: false`、稳定 code、最新 canonical snapshot、constraint prompt 与 recovery actions，
且无副作用。

成功操作以 `(turnId, callId, argumentHash)` 记录。相同身份与参数重放返回 `alreadyApplied`；相同身份
不同参数返回 `operationIdentityConflict`。run ID、revision 与 current state ID 同时参与 CAS。

两个写工具与 `complete` 使用 `ToolBatchPolicy::Solo`；四个查询工具使用 `Coexist`。
工具只在 working-set clone 上计算；tool call、result 与新 working state 由随后同一个 Thread checkpoint
共同提交，失败则保持旧 canonical revision。

同一 Turn 的 working context 为 prompt cache 保持冻结，切换结果由 tool result 立即提供；下一 Turn
由 `pl.workflow` 注入。若发生 compaction，则在 rebase 边界重新捕获最新 workflow projection。

## 16.6 内置 Mode

`mode.simple` 不带 workflow，也不要求阶段转换；它直接工作、按风险验证，并在完成时调用 `complete`。
它不增加 Git、固定审查轮次或交付门禁。

`mode.task` 注册预设图 `planning -> editing_documents -> working -> integrating -> reviewing -> completed`，
并提供 stopped 终态。代码 finding 回到 working，设计
finding 回到 editing_documents；两条返工路径都必须重新经过 integrating 和 reviewing。完整计划必须
在 planning 中通过 `plan_current`、`plan_next` 和 `plan_submit { expectedRevision, plan }` 请求批准或修订；
用户要求补充时 Plan 固定状态机进入 `revisionRequested`，重新提交与批准期间 workflow 仍保持 planning。
只有 `plan_current` 返回 `approved` 后才可 transition 到 editing_documents。`request_user_input` 只用于
计划形成前的缺失信息与澄清；Plan 确认也复用通用 `UserInput` continuation，但生命周期只属于当前
AgentSession。批准后的完整 Plan 作为 GUI 隐藏的用户输入进入该 session；进入 `completed` 后调用
`complete`。完整合同见 [24-agent-session-plan.md](24-agent-session-plan.md)。
`completed` 与 `stopped` 都是无任何 outgoing transition 的 final state；停止边只从非终态 state
进入 `stopped`。

planning 中 root 优先把相互独立的探索并行交给 fresh-context `explorer`，自己综合依赖图、文件所有权与
验证边界。editing_documents 中 root 亲自更新设计。working 中普通实现必须交给 `executor` 或
`worktree_executor`：单任务和互斥目录并行优先 directory，可能交叉影响共同接口、清单、生成边界或 Git
状态时使用 worktree；真实前后依赖始终顺序执行。每个 child 消息必须详细描述目的、基线、所有权、
禁止范围、有序步骤、完成/失败条件、证据和 workspace/Git 合同。

所有 child 使用同一成果传递顺序；非 reviewer child 完成探索/实现/验证后先调用 `report_progress`，以
`readyForCompletion` 提交含 `CHILD_DELIVERY_READY` 的 durable detail，再发送内容一致的 final reply；
reviewer 使用既有的 durable verdict marker。
root 保存成功 spawn receipt 中的 `agentId`，循环 `wait_agents` 直到 terminal，然后对该 id 调用
`read_agent_submissions`；progress 事件只能触发继续等待。canonical page 必须非空，空页只允许进入
诊断和收窄重派，`read_agent_session` 不能替代正常交付。reviewer 使用既有 finding/approval marker，
并继续保持比通用 child delivery 更严格的最终授权语义。
批量 `wait_agents` 只为返回 message 中明确绑定的 agent 提供证据，不能推断未返回目标的状态。root
维护 pending agentId 集合，且只在同一次 receipt 中观察到 `reason=terminal`、匹配 agentId、
`state.agent.kind=idle|closed` 与 completed `lastTurnOutcome` 后移除；progress 中即使已有
`CHILD_DELIVERY_READY` 也必须继续等待。所有 pending 目标清空前禁止调用任何一个目标的
`read_agent_submissions`，从而让每份 durable delivery 都有先行的 receipt-bound terminal 证据。

integrating 中 root 审查 directory 组合 diff、显式采纳 worktree commit、cleanup 并处理冲突。并行
worktree 批次必须先把全部接受的 commit 整合进主 workspace，再开始逐个 cleanup；不得在同批次仍有
未整合 commit 时提前清理其中任一 child。冲突处理中
允许完成相邻必要实现或测试修复，但不能展开无关工作。child 持续不可用时，root 收窄重派一次后可以
最小兜底，并显式记录 `ROOT_IMPLEMENTATION_FALLBACK`。reviewing 必须由新创建的只读 `reviewer` 检查
整合后的主 workspace；root 自审不能替代它，阻塞 finding 修复后必须创建新的 reviewer 复审。
工具参数错误必须先对照 schema 修正再重试，禁止原样重复失败调用；模式专属字段必须使用 camelCase，
`writablePaths` 只传给 directory child，`workspaceDisposition:cleanup` 只在 worktree commit 已整合后使用。
验收用的 directory 越界拒绝是明确的 expected rejection，不得重试或借 shell 绕过。
reviewer 只读完成后以 `report_progress` 提交 durable finding/approval，root 按冻结 reviewer `agentId`
调用 `read_agent_submissions` 获取结构化 verdict；只有该证据到达后才能执行最终门禁。该提交不改变
workspace 或 Git，不能扩张 reviewer 的修复权限，也不能由 root 自述或自由文本 session 摘要替代。

上述职责由本 Turn 的 Mode/Profile 指令约束，不新增专用 executor/reviewer runtime 生命周期，也不按阶段裁剪
普通工具能力。

## 16.7 GUI 与 live 验收

GUI 动态展示 `ThreadModeCatalogSnapshot`，状态栏只显示 Mode 与当前 state，不展示允许边、完整图或
transition history。Driver 从 canonical snapshot、工具回执和 wire 读取完整历史。

wire replay 对 provider 请求历史中的查询和转换分别按各自严格 typed schema 验证。转换要求完整
run/revision/current-state CAS、目标、理由和 completion；查询不接受多余字段。任何 provider 请求中出现旧
`workflow_state`、definition、compile 或 supersede 输入都直接使验收失败。

最终入口是 `cargo xtask verify-workflow --live --headless` 与 `cargo xtask verify-workflow --live --gui`。
它们必须使用真实非本地 provider 和真实 Prompt，不得 fallback 到 scripted/demo provider。workflow
harness 在隔离 Rust 项目中分别选择 `mode.simple` 与 `mode.task`：简洁模式直接完成且不产生 workflow
调用，任务模式在 planning 内走完 Plan clarification/revision/approval，再经过
editing_documents/working/integrating/reviewing/completed 并调用
`complete`。真实 Prompt 的 wire 验收必须拒绝任何工具执行失败或 `accepted:false`；后续成功重试不能
掩盖首次错误。最终临时 GUI Task 验收还必须先提出真实澄清，接收固定计划修改意见，再重新提交计划并
获得批准；批准前不得开始实现。随后并行运行至少两个互斥 directory child 与至少两个独立 worktree
child，验证不同 workspace/branch、整合前隔离、显式 commit 采纳、只读 reviewer 与全部 cleanup。
两份 worktree commit 必须都先完成显式采纳，之后才允许第一次 cleanup。
两种模式都运行 `cargo test` 与 verifier；GUI 额外重启任务模式验证 run id/revision/history 持久化。
失败 artifacts 保存到 `target/workflow-live-artifacts/` 或
`target/subagents-live-artifacts/`，同时回收 GUI、DTD 与 Driver 进程树。
子代理 artifact 必须保留未去重的协作调用 attempt、outcome 与 error class；每个 child 都需要绑定成功
spawn receipt 的 nonempty durable submission。非预期首次调用失败不能因后续成功 receipt 被静默忽略，
expected directory rejection、正常 progress polling 与空页诊断分别分类。
