# 03 - 编译流程（方案乙）

## 3.1 总览

方案乙将流程明确分成“桥接层 -> 应用层 -> 领域核心 -> 适配器”：

```text
Flutter action
  -> flutter_rust_bridge API (pl-studio-bridge)
  -> pl-studio-runtime StudioRuntime / StudioHost
  -> StudioConversationSink / StudioEventRuntime
  -> interfaces ports
  -> studio/config/tool/mcp adapters (sqlite/config/fs/event/tool)
  -> pl-core AgentRuntime registry / AgentLoop
  -> TurnEngine turn pipeline
  -> pl-trace AgentEvent / TraceEvent / TracePart
  -> StudioEventRuntime canonical message/part snapshot + live part delta
  -> FRB Stream<BridgeEventEnvelope>
  -> Riverpod Studio event reducer
  -> Material 3 UI rendering
```

Flutter bridge crate 不承载流程逻辑，只把 Dart 调用转发到 `pl-studio-runtime`，并把
`StudioEventEnvelope` 映射为 `BridgeEventEnvelope`。

agent 的输入队列、活动 turn、取消和恢复由 `pl-core::AgentRuntime` 唯一维护。Studio 只通过
`StudioHost` 准备 turn、提交 durable state、管理外部资源和广播已提交事件。

## 3.2 输入与策略

运行输入统一为新 DTO 契约（camelCase wire）。

- `compileMode`：`simple | task`
- `turnOptions.permissionMode`：默认固定 `request-approval`
- `prompt`、`sessionId`、`workspaceRoot` 等进入 `StudioRuntime`

`compileMode` 同时决定根模型角色和协作边界。`simple` 根 turn 使用 executor，直接对话和实施，只能创建只读 explorer；`task` 根 turn 始终使用 planner，由 planner 通过持久化 coordinator 管理 explorer、executor、reviewer、当前分支和审查闭环。确认实施后仍保持 `task`，不会切换到 `simple`。详细状态机、交付和 merge 契约见 `16-task-orchestration.md`。

`workspaceRoot` 是运行期有效工作区，而不是简单等于 UI 当前选中的目录。Studio 读取 project path 后先解析到规范化目录；如果该目录位于 Git 仓库中，则提升到最近的 Git 仓库根。这样用户从子 crate 或桌面壳层进入项目时，工具仍能访问完整仓库上下文。工作区记忆按 Codex 风格从 Git 根到当前工作目录读取层级文档，候选文件优先级为 `AGENTS.override.md`、`AGENTS.md`、`Agents.md`，并受配置的总字节预算限制。

提示词在核心层按三层组装：`base/system`、`developer`、`user context`。`developer` 承载 Simple/Task 与当前 execution profile 的差异；运行时工具 effect 和 coordinator phase 是最终权限来源，提示词不能扩大权限。

系统提示词必须把文档同步作为代码修改前后的稳定约束：涉及架构、接口、行为或项目约定的变更，代理应先阅读相关设计文档和项目记忆，确认计划后先更新 `design/` 下对应文档，再开始实现；实现完成后需要整体回看计划、文档、代码和测试结果，确认交付内容仍与文档和计划一致。若实现过程中发现现有文档与可行方案冲突，应暂停并让用户在遵循现有文档、采纳新方案并同步文档、继续补充需求之间选择。

平台工具规则属于 developer 层，不属于 base/system。`InstructionAssembler` 在 mode prompt 之后、配置 developer 之前注入平台 block：公共规则总是存在，具体系统规则通过编译期 `cfg(windows)` 或 `cfg(unix)` 选择。该 block 随 session instruction snapshot 保存，保证恢复会话或子代理继承时不因运行时提示文件变化而漂移；即使用户配置了 `base_override`，平台 developer block 仍照常注入。

当启用 skills 时，运行时从 `<workspaceRoot>/skills/`、`~/.pure/skills/`、`~/.pure/skills/.system/` 和配置外部目录发现 skills，并在 developer 层注入简短索引。项目 skills 优先于用户、系统和外部只读 skills；自学习写入始终落到 `<workspaceRoot>/skills/`。

策略约束：

- 方案乙不保留旧命令别名和旧字段兜底
- `PermissionMode::RequestApproval` 为默认且主路径；工具权限不再维护独立审批策略
- 手动审批接口保留在系统能力中，但不作为默认流程；`request-approval` 模式只在工具请求 workspace 外访问时弹出用户审批，workspace 内读写直接放行
- `auto-review` 模式使用 reviewer 角色模型审批 workspace 外访问。reviewer 只返回批准或拒绝，不执行工具；解析失败、provider 失败或非明确批准均按拒绝处理
- `full-access` 不能扩大 execution profile 的工具 effect；planner、explorer、reviewer 仍受角色白名单约束
- Task planner 的探索和协调只使用 profile 允许的读取与 harness 工具；明确写入类工具不能通过权限模式绕过
- 用户显式要求 `subagent`/子代理分工时，核心提示必须将异步 agent 调度作为强约束；普通 shell 或文件探索不能替代子代理调度
- 通用多 agent 协作由 `spawn_agent`、`report_progress`、`send_message`、
  `interrupt_agent`、`list_agents`、`wait_agents`、`read_agent_session` 和
  `close_agent` 组成；工具只持有非泛型 `AgentRuntimeHandle`，不接触 Studio host 或
  AgentLoop 内部状态。`wait_agents` 订阅单一 Agent Directory watch，随后重读 canonical
  snapshot，并只因真实 progress、interaction 或 terminal 变化返回；它没有 timeout、
  轮询或自动续轮。产品 harness 可以注册严格类型化的 spawn 工具并调用同一个 runtime；
  Task 根的通用 `spawn_agent` 只创建 explorer，executor 使用 `task_spawn_executor`；
  delivery reviewer 与 integrated reviewer 分别使用 `task_request_delivery_review` 和
  `task_request_integrated_review`
- `request_user_input` 是 Codex 风格的阻塞交互工具，root agent 与 subagent 都可用；工具通过统一 `Interaction` 域创建 `userInput` 请求，等待前端 resolution 后把答案作为工具结果返回，不作为普通用户聊天消息写入历史
- `simple` 根聊天使用 executor，`task` 根聊天使用 planner；Task child role 由产品工具固定，所有 child 禁止继续派生
- agent 状态正交拆为 lifecycle（`Active | Closing | Closed | Faulted`）、activity（`Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling`）和 last turn outcome（`Completed | Cancelled | Failed | BudgetLimited`）
- turn 完成或失败后 agent 回到 `Active + Idle`；未关闭 agent 可继续接收输入，不存在 `resume_agent`
- `send_message` 是唯一 follow-up 投递：目标运行时进入 steer channel，空闲时进入 durable
  FIFO 并启动新 turn；`interrupt_agent` 只中断当前 turn，不附带下一条 prompt。AgentLoop
  先持久化输入，再准备和执行 turn；取消先触发 token，最多等待一秒清理，超时才 abort
- child 的 durable commit 更新 Agent Directory snapshot 与 watch revision，但不自动抢占、
  唤醒或启动父 agent。Planner 没有其他工作时调用 `wait_agents`，从新 progress、interaction
  或 terminal snapshot 恢复同一工具调用；用户输入、中断或关闭会取消等待
- 关闭父 agent 时按产品策略级联关闭子树；终态 repository commit 失败才把 AgentLoop 置为 Faulted 并拒绝新输入
- `AgentRuntime` 只维护 registry、容量和 spawn/close saga；每个 `AgentLoop` 是自身 canonical session、pending input、active turn、cancel token 和 revision 的唯一状态机。durable session cursor 只由 `SessionEventHub` 的 canonical projection 分配，AgentLoop/repository 中的 sequence 仅是提交后可修复的 checkpoint 镜像；宿主只能通过 CAS `AgentCommit` 与 lifecycle saga 持久化/管理外部资源
- 协作输入先由 runtime resolver 把模型提供的 target 解析为 agent id；模型工具不接受
  `sessionId`，也不得回退到 caller session。目标 AgentLoop 在同一条 submit 命令内读取
  自己唯一的 canonical session 并复核 lifecycle；显式携带 session id 的产品入口只能与该
  session 精确相等，不解析 current pointer、不保留多 session 容器，也不隐式创建空 session

## 3.3 核心 turn 编排

`StudioRuntime` 只做 use case 编排；Studio UI 状态不由命令完成响应驱动，而由 `StudioEventRuntime` 持久事件流驱动。UI-facing runtime 在 `pl-studio-runtime` 内维护状态机：

```text
Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped
                         │                         │
                         └──────────────► Failed ◄──┘
```

`initializeRuntime()` 完成配置、store、projection 恢复和非终态 turn 收敛；`startRuntime()` 启动后台 health、事件桥接和 `pl-core::AgentRuntime`；`shutdownRuntime()` 通过 runtime handle 取消活动 turn、停止后台任务并返回最终 snapshot。active turn、FIFO、cancellation 与 canonical session 归对应 AgentLoop，interaction projection 和 Studio 产品资源归 `pl-studio-runtime`。

核心编排步骤：

1. 读取 session/project/config
2. 解析或创建 session 级提示词快照
3. 构造 `TurnRequest` 与 `TurnOptions`
4. `StudioHost::prepare_turn` 组装 `AgentKernel`、`TurnEngine` 与 execution policy
5. `AgentRuntime` 注入 turn identity/cancellation/trace 后执行 turn
6. 运行中每个可见对话 lifecycle 先进入 `StudioEventRuntime`。用户、assistant、commentary、reasoning、plan 和 tool 均规范化为 opencode 式 `StudioMessage` / `StudioPart`。`messageUpdated` 与 `messagePartUpdated` 是 durable snapshot：history writer 先把事实批量写入历史库，再把 projection mutation 写入状态库；只有 history ack 后才能广播 durable envelope，terminal 还必须等待两个库的 watermark barrier。`messagePartDelta` 只进入实时通道，是 live overlay，不写入历史库、不推进 durable cursor。
7. turn 收尾只负责消息、最终 runtime snapshot 与生命周期终态校准；不得要求前端等待最终响应才能看到过程

前端提交普通 prompt 或 Plan 实施时调用同一套后台 turn 提交流程。Flutter `submitPrompt(sessionId, prompt, attachmentIds)` 只创建 turn、注册 cancellation token、写入 canonical `turnChanged`、必要的用户 `messageUpdated` 和用户 `messagePartUpdated`，随后在后台执行 run，并立即返回 `{ sessionId, turnId, cursor }`。turn 状态使用 tagged `SessionTurnState`：`queued | inProgress { activity } | completed | failed { reason } | cancelled { reason }`；`SessionTurnActivity` 固定为 `preparing | thinking | responding | planning | runningTool | waitingForApproval | waitingForUserInput | waitingForPlanConfirmation | persisting`。没有当前 turn 就表示 session 空闲，不再发送 `idle`、`waitingForModel`、`streaming` 或通用 `waitingForInteraction`。

Rust projection 必须按真实事件推进当前 turn：排队后进入 `preparing`，模型等待与 reasoning 都进入 `thinking`，可见正文、计划和工具分别进入 `responding`、`planning`、`runningTool`，typed interaction 直接映射到三个具体等待 activity，提交终态前进入 `persisting`，最后进入 terminal state。Flutter 不得从 repository 回调、Plan lifecycle、message/part 或 pending interaction 本地反推 turn 状态；`turnChanged` 是唯一事实源。

计划确认选择实施时，`resolve_interaction` 在当前 `sessionId` 内解决 interaction、写入
`accepted/implementing` lifecycle，并以稳定 input id 向同一 canonical agent queue 提交
显式实施输入；不得创建或切换 target child session，也不得依赖
`sessionHandoffChanged` 展示实施过程。mailbox 输入使用 typed
`MailboxPresentation::{User,SyntheticVisible,SyntheticHidden}` 决定 timeline 投影：普通
用户 prompt 使用 `User`；只有产品明确要求展示的合成输入使用 `SyntheticVisible`；计划实施、
spawn 初始任务和 `send_message` follow-up 使用 `SyntheticHidden`，不得生成用户 message/part。

`TurnEngine` 在每次模型请求前用 `InstructionAssembler` 解析当前提示词快照。base/system 写入 `CompletionRequest.instructions`；developer 块作为临时 system 消息置于历史消息之前；user context 块作为临时 user 消息置于 developer 块之后、真实历史之前。临时前置消息只用于本次 provider request，不写入 `AgentSession`，因此压缩和持久化只处理真实对话历史。

`TurnEngine` 接收新 turn 后，真实用户输入必须已经通过 `submit_prompt` 生成 durable user message/part，并在 `AgentSession` 中作为模型历史写入；随后才能记录 enabled tools、turn running、inference、工具和模型输出等内部运行事件。每个 turn 的用户输入只能对应一个 canonical user text part：message id 为 `{turnId}:user`，part id 为 `{turnId}:user-text`。若内部 trace 仍产生用户输入 snapshot，进入 Studio 协议前必须忽略，只保留为内部诊断，不能覆盖既有 part 或生成第二条用户消息。`StudioEventEnvelope.sequence` 是后端唯一 durable 游标，part 的 `order` 只用于 message 内展示顺序，前端 optimistic 提示不得改变后端游标。

assistant 的 text、commentary、reasoning 和 plan part identity 不得直接使用 provider 局部 `item_id`。provider id 只在单个 inference 内定位当前打开的 stream block；进入 Studio part 前必须生成包含 `{turnId}`、`{inferenceId}` 和语义段序号的稳定 id，例如 `{inferenceId}-reasoning-1`、`{inferenceId}-text-final-1`。同一个 provider item 若出现 text channel/phase 变化，必须先关闭旧 block 再打开新 block，不能把旧 block id 继续用于另一种可见 channel。工具调用是语义边界：遇到 tool start/ready 或 step finish 时，当前打开的 text/reasoning/plan 必须先完成并清理 active provider 映射；工具后的模型输出必须创建新的 part，展示顺序排在工具之后。工具 part 使用 runtime tool call id 保持稳定，使 provider tool snapshot 与 core tool execution snapshot 更新同一个 part。

Reasoning part 使用 Codex 式双通道内容：`summary: Vec<String>` 保存 provider reasoning summary，`content: Vec<String>` 保存 raw reasoning content。live delta 分别使用 `reasoning.summary` 与 `reasoning.content`；raw content 首次到达就创建并更新当前 reasoning part，后续 summary 更新同一 part，不复制第二条思考 item。authoritative snapshot 同时保留两组内容，消费方总是优先展示非空 summary，其次 raw content。两组均为空时才允许显示无内容占位。

message/part 是有序 item，而不是按 turn 聚合后的展示块。`partId` 首次插入时固定
`messageId/sessionId/turnId/type/order/createdAt/textChannel`；后续 streaming/terminal
snapshot 只能替换内容、revision 和状态，不能移动首次位置或改变身份。批内同一 part 的多次
更新只发布最终 snapshot。工具 part 不携带分组 id；展示层只合并排序后相邻的可见 tool part，任何 text、
commentary、final、reasoning、plan、agent row 或 message 边界都会结束当前工具组。

模型输出的 `commentary` 只进入 timeline，用于让用户看到阶段性进展，不写入 `AgentSession`。只有 `final` 输出会作为 assistant response 写入会话历史；带工具调用的中间轮次如果只输出 commentary，也不得把 commentary 当作 assistant tool-call content 写回 provider 历史。

`TurnEngine` 在每次模型请求前执行自动上下文压缩检查。首次请求使用完整 assembled request 的估算 token；工具调用后的后续请求优先使用 provider 上一响应报告的实际上下文 token。同一 session revision 与上下文项长度没有变化时不得重复压缩。压缩阈值来自当前模型的 `autoCompactTokenLimit`，未配置时使用有效上下文窗口的 90%；模型没有上下文窗口信息时不触发自动压缩。`TurnEngine::compact_session` 和 trace 版本提供忽略阈值的 standalone 手动入口；空上下文或只有既有 checkpoint 时不修改会话。

压缩实现由 wire protocol 和配置共同决定。Chat Completions 始终本地压缩；Responses provider 可读取 `runtime.openai_compaction_mode = "remote_v2" | "local"`，默认 `remote_v2`。远程压缩始终使用独立 Responses HTTP 请求，不复用常规 WS 连接，也不改变 provider 实例选择的连接模式。远程失败不得自动回退本地，也不得安装局部结果；只有完整校验成功后才原子替换 session，并使 transport session 的旧 continuation 失效。

本地压缩使用当前 turn 的 canonical instructions，把 compact prompt 作为最后一条 synthetic user 输入，请求不携带工具；遇到不支持 max output 参数或 context pressure 时按压缩规则重试。替换历史过滤旧摘要，按 20k token 预算保留最近真实用户消息，边界消息按 token 截断，最后追加新摘要。OpenAI v2 按 64k token 预算保留最近真实用户消息及图片，最后追加唯一的加密 compaction item。instruction prelude 参与压缩请求但不进入替换历史，下一次模型调用重新生成 canonical prelude。含加密 checkpoint 的会话若切换到非 OpenAI provider，当前轮必须在请求前失败并提示继续使用 OpenAI 或新建会话。

子代理没有独立的压缩实现。`AgentRuntime` 为每个 agent 保存独立 `AgentSession` 集合，child turn 复用同一个 `TurnEngine` pipeline，因此每个子代理独立维护自己的压缩历史与 transport session；父会话不会替子代理压缩，也不会因为子代理压缩而改写父历史。

子代理继承稳定 instruction context，但 execution profile 由角色决定，不直接继承父 turn
权限。explorer/reviewer 只读，executor 只写自己的 worktree；task child depth 固定为 1。
Task executor 和 reviewer 使用 fresh session：executor 的自包含任务由
`task_spawn_executor.message` 提供，reviewer 的自包含审查上下文由 harness 构造，二者都不
复制 planner 完整历史。

子代理同样继承父 turn 的交互运行时。`request_user_input`、工具审批和计划确认统一表达为 `InteractionKind::{userInput, toolApproval, planConfirmation}`。每个 interaction 都带 `sessionId`、`turnId`、可选 `itemId/toolId/agentPath`，由 `InteractionRuntime` 创建、持久化、广播并等待 resolution。Studio 只渲染当前最高优先级 pending interaction；回答或审批只解除对应等待，不触发新 turn，也不写入普通聊天消息。UI 交互形态对齐 opencode dock prompt：pending question/permission/plan confirmation 在底部 dock 处理，timeline 只渲染 message/part 投影；`request_user_input` 的 completed tool part 可以显示 redacted 问题答案摘要。

进程重启会丢失 interaction 的进程内 waiter，但不会丢失 durable `userInput`。恢复时
`userInput` 保持 pending；`toolApproval` 因外部副作用是否已经发生无法可靠判断，必须拒绝并
取消。若用户回答时原 turn 已以 `runtime_restarted` 终止，Studio 以
`mail_id = interaction:<id>` 把完整问题与答案作为 typed `SyntheticHidden` 输入提交到
canonical agent queue。queue 对稳定 id 的幂等接受与 interaction resolved receipt 使用同一
durable transaction；崩溃恢复只对账 input receipt 和 interaction projection，不使用
continuation outbox、wake receipt 或 detached 自动续轮。没有 pending 显式输入的活动 Task
显示 paused，由用户继续；attach 不启动模型。

Simple 模式主 turn 保存完成后，如果 `[skills].auto_learn = true` 且本轮达到自学习触发条件，`StudioRuntime` 启动后台 reviewer。reviewer 只开放 skills 工具，复盘结果只写项目 skills 目录；失败只记录日志，不改变本轮响应。Task 模式从规划开始由 coordinator 独占工作区写入，既不启动后台自学习 reviewer，也不允许只读的 `skill_view` 更新项目使用统计，避免计划确认前出现绕过 TaskRun 的 workspace 修改。

文件、shell 和 LSP 查询工具都以有效 `workspaceRoot` 为默认边界。工具输入不要求全部使用绝对路径；相对路径一律按 `workspaceRoot` 解析，而不是按 Pure Studio 进程 cwd。执行前，核心层用统一路径策略把输入解析为规范化绝对路径，并用同一结果做权限预判和实际执行。`exec` 默认在 workspace root 下执行，`cwd` 也按 workspace root 解析并拒绝逃逸；文件工具默认只允许访问 workspace root 内的路径；`lsp_query_*` 的 `filePath` 解析后才交给 `pl-lsp` 生成 file URI。`full-access` 模式会放宽本地 backend 的该边界，允许文件路径和 `exec.cwd` 指向 workspace 外，但仍要求 existing 或 existing-parent 可解析，不绕过工具自身校验、写锁、超时、输出截断和 timeline 记录。宿主 backend 可以施加更严格的隔离边界。

Skills 管理工具同样以 `workspaceRoot` 为边界，但写入面收窄到 `<workspaceRoot>/<skills.project_dir>/`。用户级、系统和外部 skills 只读参与发现，不允许被工具原地修改。Task 模式仍可发现、读取和激活 skills，但所有项目 skill 写入及使用统计更新都必须停用；Task 实施所需的设计和源码修改只能通过 coordinator 管理的边界发生。

工具预算与收尾原则：

- 工具调用或 provider 返回 `end_turn = false` 只表示 `needsFollowUp`，不是完成条件
- root turn 和 child agent 默认只强制 `wallClockMs = 1800000`
- 模型采样和普通工具调用只记录 `modelSteps`、`toolCalls` 观测计数，不触发 step/tool 限制
- pending interaction 等待期间仍受当前 turn 的 cancellation token 和 wall-clock 预算约束；用户停止时 pending interaction 被标记为 `cancelled`，wall-clock 到期时标记为 `expired`
- agent tree 默认限制为 `maxAgents = 16`、`maxDepth = 3`
- 预算耗尽属于 `TurnAborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace，不得伪装为 `failed` 或 `completed`
- wall-clock 预算耗尽时核心层按 `budgetLimited` 收尾，并在 trace 中保留预算用量
- 无工具总结或普通 assistant 文本中若出现未执行的工具调用标记，必须按 `budgetLimited` 收尾并写入 `TurnBudgetLimited`；不能把原始 tool-call 文本作为最终回答
- 用户显式要求子代理分工时，turn 完成前必须验证本轮实际创建了 agent；否则按 `failed` 收尾，不写入伪完成 assistant 消息

Agent 协作 timeline 与状态分层：

- 每个 agent session 的 timeline 只记录该 owner 的 message/part/tool/interaction，以及它主动执行的 spawn、message/followup、close 等协作事实
- child 的内部模型输出、工具、Todo、skill 和 context 只进入 child session；root Planner
  timeline 只保留 Planner 自己的 spawn、send、interrupt、list、read 和 wait 工具事实
- agent directory 是大会话级 latest snapshot，只保存身份、父子关系、状态、最近活动和 attention，供顶部切换入口与 `list_agents` 使用
- 前端不得用 latest snapshot 渲染 timeline；同一个 agent 的多次状态变化必须在 timeline 中保留为多条独立事件
- `AgentStateChanged` 只更新大会话目录，不进入单 agent session stream。`update_todo_list` 是 Codex `update_plan` 风格的完整 checklist replacement，不是 Plan Mode plan part；当前 agent workspace 只展示 snapshot 中最新 Todo，不在 timeline 中保留 Todo 卡片历史。实时事件与历史 snapshot 都必须携带 canonical typed 语义，Flutter 不得读取 raw `AgentEvent`。

spawn 使用 runtime 原生 child `SessionId` 创建 Studio agent session，先持久化 owner/session
目录，再启动 runtime。Studio commit observer 不得把 child `session_events` 或 trace 重绑到
root Studio session。恢复顺序固定为先恢复 session owner 和 agent directory，再挂载 runtime；
owner 冲突必须拒绝并输出诊断。大会话归档级联归档其 agent sessions，但不删除历史。

所有面向 session 的宿主操作都必须从 `sessions.owner_agent_id` 解析 canonical owner，包括
prompt 提交、停止、外部 session fact 和重启后的 transient interaction 取消；不得根据
`sessionId` 临时派生 `studio:{sessionId}` 并把 child session rootify。启动恢复可在事务中
清除严格可证明的 ghost runtime registration：该 runtime agent 没有任何 canonical owned
session，且其全部 session claim 均已由其他 owner 持有。只要同一 agent 同时存在有效与冲突
claim，恢复必须拒绝并保留诊断，不能猜测或删除共享 session projection。

持久化原则（详细合同见 `19-studio-storage-and-diagnostics.md`）：

- 消息和内部 `pl-trace` 诊断事件采用事务批量写入，避免逐条写放大；旧 `timeline_events` 表的 entity、运行期写入、读取和清理路径均已删除，迁移历史按 append-only 保留（不再有运行期代码读写该表）
- `session_history_items` 是 Studio UI 与模型恢复的唯一完整 durable 事实流。每个 durable item 带 `sessionId`、会话内单调 `sequence`、`createdAt` 和类型化 payload；广播 payload 必须与持久化 payload完全一致。高频 `messagePartDelta` 是实时 overlay，不写入 durable history，必须能被后续 `messagePartUpdated` 完全覆盖。
- `SessionEventHub` canonical snapshot 的 `throughSequence` 是下一 durable sequence 的唯一依据。observation、trace、runtime event 和外部 session fact 走同一 owner validation，但物理提交按“历史库事实 -> 状态库 projection”排序，不声称跨库原子。`agent_runtime_sessions.session_event_sequence` 只保存提交后可重建 checkpoint，恢复时按历史 watermark 修复，绝不能据此分配 sequence。
- `turns` 表保存当前与历史 turn 的 tagged `SessionTurnState`。`queued` 与 `inProgress { activity }` 属于非终态；`completed`、`failed { reason }`、`cancelled { reason }` 属于终态。启动时所有非终态 turn 必须收敛为带明确原因的 `cancelled`
- `studio_messages`、`message_parts`、`agent_events`、`interactions`、`session_skills` 是 `StudioEventRuntime` 的 projection 表。message/part projection 保存 latest snapshot，live delta 只作为前端 overlay；除一次性迁移和启动恢复外，运行期不得由前端推断直接写入。Plan lifecycle 也必须先写 `StudioEventKind::PlanLifecycleChanged`，再由 projection 更新查询表。旧 `session_handoffs` projection 已通过后续迁移从当前 schema 清理，不再参与运行期读写。
- `message_parts.part_order` 在 part 首次 durable snapshot 时由 `StudioEventEnvelope.sequence` 固化；后续同 part snapshot 即使携带旧 order，也必须保留既有 order，禁止终态 snapshot 或 backfill 改变首次展示位置。
- session 的 `mode` 表示下一轮默认协作模式，由 Studio 模式切换命令持久化；运行时按 session 当前 `mode` 构造 `TurnRequest`
- session 的 `instruction_snapshot_json` 保存稳定 base/user/project context 和非 mode-specific developer context；Simple/Task 与 execution profile overlay 每个 turn 重新注入。
- Task 计划与实施状态通过 durable coordinator 和 plan lifecycle 事件表达；确认实施后在同一 session 进入 `designUpdating`，由明确实施输入推进，不切换会话模式。
- `interactions` 表保存所有 pending/resolved/cancelled/expired 交互，是刷新与 session 切换恢复 pending UI 的事实来源。`InteractionChanged` 通过 `StudioEventKind::InteractionChanged` 广播当前 interaction 最新状态；旧 `studio-user-input-*`、`studio-tool-approval-*`、`studio-interaction-changed` sideband 事件不再作为 Studio 协议入口
- `userInput` 的 durable record 与进程内 waiter 是两层状态：同进程回答直接唤醒 waiter；
  重启后回答通过稳定 mail id 进入 canonical queue。`toolApproval` 不做跨进程恢复，避免把
  不确定的外部副作用重复执行
- framework attach 时会检查活动 Task 根会话最新的完整 Plan trace；如果对应确认 interaction 缺失、没有活动 TaskRun，且同一计划尚未进入实施或终态，则幂等补写 `PendingConfirmation` lifecycle 与 `planConfirmation`。该恢复只修复已有完整计划证据的投影缺口，不从局部 delta、普通文本或旧计划猜测确认内容
- `skill_view` 成功激活 skill 时，后端写入结构化 `SkillActivated` 事件并 upsert 会话级 skill runtime fact。Studio 当前会话的 `activeSkills` 只从 `session_skills` 等结构化持久层读取，不能再从 tool result JSON 文本反解析。
- 如果 turn 内发生上下文压缩，`AgentSession` revision 会变化，Studio 以事务重写当前 session 的有序模型上下文项并追加本轮 trace；未发生压缩时继续使用追加写入。`messages.item_type` 区分普通 `message` 与加密 `compaction`。恢复时加载两类上下文项，Studio/Flutter 消息查询与 projection 只返回普通消息，绝不暴露 checkpoint 加密内容
- StudioEvent 读取以 `sequence` 为 durable 单调游标；message/part snapshot projection 的 `sequence` 必须等于来源 `StudioEventEnvelope.sequence`。`messagePartDelta` 没有 durable sequence 语义，前端不得用它推进 cursor。
- agent tree、agent events、agent messages 与 turn snapshot 分表持久化；`agents` 为 latest snapshot，`agent_events` 为 append-only event log

## 3.4 事件管线

Flutter/FRB 端使用两类订阅：

- `subscribeSessionEvents(sessionId)`：只转发当前会话的 timeline、turn、interaction、session runtime、agent 与高频 `messagePartDelta`。
- `subscribeGlobalEvents()`：只转发项目、配置、Provider usage、MCP/LSP health 等低频全局变化。

Studio runtime 在启动、保存 Provider 设置和保存 MCP 设置后都必须刷新同一份 effective MCP server 列表并广播 `McpHealthChanged`。Provider 保存包含 Zhipu Coding Plan token 时，内置 Zhipu MCP server 立即进入后台 health 检查，不依赖下一轮 turn 或下一次应用启动触发。

`subscribe_session(session_id)` 和 `subscribe_global()` 必须在 `pl-core` 内过滤；Flutter 切换会话时取消旧 session stream，只保留当前打开会话的高频监听。

`StudioRuntime::drain_agent_events` 在 `pl-core` 内使用显式分支处理内部 broadcast 通道状态：

- `Ok(event)`：交给 `StudioEventRuntime` 持久化并广播 `studio-runtime-event`
- `Err(Lagged(n))`：广播 `StudioEventKind::Stale { laggedEvents: n }`，Flutter 按 cursor 调用 `load_studio_events`
- `Err(Closed)`：结束循环

这保证高频 delta 下 UI 不会因为 lagged 直接断流。Flutter bridge 检测到 lagged 时必须为 active session 发 live-only `stale`，驱动前端用 durable cursor 补拉 snapshot。前端按 opencode 的事件批处理方式在 16ms frame 内合并事件：如果同一 part 的 durable snapshot 到达，跳过该 frame 中同 part 尚未应用的旧 live delta，并清除该 part 的 delta overlay；若 snapshot 被 coalescing 覆盖，也必须把同 part pending delta 标成 stale。terminal snapshot 到达后，低序或等序 live delta 不得再修改该 part；带 `chunkIndex` 的 delta 需要按 part 去重。

`StudioEventRuntime` 的运行时职责与映射职责分层维护：运行时入口负责订阅、持久化广播、live/durable 分流和 timeline actor 状态协作；trace 到 Studio 协议的纯映射（message/part id、part 类型/状态、agent timeline、delta field 与文本提取）放在独立 mapper 子模块。mapper 只做确定性结构转换，不访问 store、不广播事件、不分配 durable sequence；所有持久化游标和 projection 更新仍由 `StudioEventRuntime` 与 `StudioStore` 负责。

`StudioRuntime` 保持 use case 门面边界：公开入口仍由 `StudioRuntime` 暴露；runtime 初始化/启动/关闭放入 lifecycle 子模块；项目、会话、配置角色、provider usage 与 skill catalog 查询放入 session-service 子模块；agent/runtime usage 的展示快照映射放入 projection 子模块；skills 自学习触发、阈值统计和后台 reviewer turn 放入 self-learning 子模块。子模块不得直接替代 runtime 发事件或写 store，只返回确定性 projection、执行门面方法对应的持久化动作，或启动明确的后台 review 任务。

`messagePartDelta` 只用于 live overlay，不得写入历史库。`stale` 也是 live-only 补拉提示，不占用 durable sequence，不参与历史重放。`load_session_state` 从状态库 projection 恢复当前终态；`load_session_history_page` 从历史库按 turn keyset 加载完整旧历史。两条读取路径按 sequence 合并，历史恢复不得依赖 delta。

`Done`、turn final、agent final 属于 lossless 事件：转发层必须确保它们不会因为普通 delta 的背压被丢弃。

## 3.5 Turn 收尾语义

turn 生命周期持久化语义固定：

- `started`
- `completed`
- `aborted`（具体原因见 `turnAbortReason`）
- `errored`

用户停止属于 `aborted(reason=interrupted)`，不可被延迟完成覆盖。
工具、模型或 agent 基座错误属于 `errored`，必须写入 `TurnFailed` trace。
工具、等待、模型采样或 agent tree 预算耗尽属于 `aborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace。

## 3.6 输出模型

命令输出统一采用新 DTO：

- `bootstrapResponse`
- `projectSelectionResponse`
- `sessionSelectionResponse`
- `submitPromptResponse`
- `studioEventsResponse`
- `sessionStateResponse`
- `settingsDraftResponse`

Flutter/FRB 输出使用同一语义，桥接层统一包成：

- `RuntimeSnapshot`：runtime 状态、活动会话/turn、更新时间与可展示错误
- `BridgeEventEnvelope`：`eventId/sessionId/turnId/sequence/createdAt/payload`
- `BridgeStudioEventsResponse`：`sessionId/events/nextSequence`，其中 `events[]` 与实时 stream 共用 typed `BridgeEventEnvelope`
- `BridgeSessionHistoryPageResponse`：`sessionId/turns/nextBeforeTurnSequence/hasMore`，用于重启恢复和向上分页
- `BridgeStudioSnapshotResponse`、`BridgeSessionStateResponse`、`ProviderUsagesResponse`、`SkillsResponse`、`SubmitPromptResponse`、`StopPromptResponse`、`ResolveInteractionResponse`、`SettingsDraftResponse`、`ConfigSavedResponse`：FRB typed DTO。Studio snapshot 通过单一 `BridgeStudioSettingsDto` 携带 provider、role、runtime、instructions、skills、MCP、General 与 Web Search 的 canonical typed view；不得携带 `configJson`、`generalSettingsJson` 或 raw map。

Dart FRB adapter 从 `BridgeEventPayload` sealed union 归一出 app 内部 typed `StudioBridgeEventPayload`；Riverpod reducer 只按 payload 类型更新 store，不再读取 `event.payload[...]` Map。实时 stream 与 `loadStudioEvents` backfill 必须共用这套 typed envelope。命令、snapshot 与配置返回不使用 `JsonResponse` 外壳，也不在 Dart adapter 解配置 JSON；只有工具参数、开放 provider payload 等本来就动态的协议标量可以保留 JSON。agent timeline 在 FRB 边界使用 typed payload union；Flutter 不解析历史 `payloadJson` agent event 记录，持久层必须在进入 Flutter 前投影为 typed `BridgeAgentTimelineEventDto`。

Flutter 桥接动作按同一 runtime 边界命名：`bootstrapStudio`、`openProject`、`selectProject`、`createSession` 和 `archiveSession` 返回新的 Studio 快照，`setSessionMode` 持久化当前 session 的下一轮协作模式，`setModelRole` 写回 provider/role 配置并返回 canonical config view，`submitPrompt`/`stopPrompt`/`resolveInteraction` 只表示请求已提交，`loadSessionState`/`loadStudioEvents` 用于会话恢复与 stale backfill，`saveProviderSettings` 与 `saveMcpSettings` 写回配置后必须同步刷新 MCP runtime health，`saveRuntimePermissionMode` 写回 runtime config，`saveStudioSettingsDraft` 持久化尚未 typed 化的设置页草稿。
