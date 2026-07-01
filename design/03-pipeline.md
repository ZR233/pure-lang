# 03 - 编译流程（方案乙）

## 3.1 总览

方案乙将流程明确分成“桥接层 -> 应用层 -> 领域核心 -> 适配器”：

```text
Flutter action
  -> flutter_rust_bridge API (pl-studio-bridge)
  -> pl-core StudioRuntime
  -> StudioConversationSink / StudioEventRuntime
  -> interfaces ports
  -> studio/config/tool/mcp adapters (sqlite/config/fs/event/tool)
  -> PureCore turn pipeline
  -> pl-trace AgentEvent / TraceEvent / TracePart
  -> StudioEventRuntime canonical message/part snapshot + live part delta
  -> FRB Stream<BridgeEventEnvelope>
  -> Riverpod Studio event reducer
  -> Material 3 UI rendering
```

Flutter bridge crate 不承载流程逻辑，只把 Dart 调用转发到 `pl-core`，并把 `StudioEventEnvelope` 映射为 `BridgeEventEnvelope`。

## 3.2 输入与策略

运行输入统一为新 DTO 契约（camelCase wire）。

- `compileMode`：`plan | auto`
- `turnOptions.permissionMode`：默认固定 `request-approval`
- `prompt`、`sessionId`、`workspaceRoot` 等进入 `StudioRuntime`

`compileMode` 是会话级协作模式，不是模型角色路由。Studio 根聊天 turn 始终使用 `planner` 角色模型；`auto` 表示执行型协作模式，允许模型在审批策略约束内主动修改工作区；`plan` 是 Codex 风格规划模式，允许读取、搜索、运行经审批的探索命令和调度探索型子代理，但最终交付物应是一段可执行计划，而不是直接修改文件。前端切换 Auto/Plan 调用 `setSessionMode(sessionId, mode)` 持久化 session 默认模式；前端切换根聊天模型调用 `setModelRole(roleKey=planner, providerId, model, effort)` 持久化 planner role，下一轮 turn 按新的 planner role 解析 provider/model。模型可见输出在协议层分流：OpenAI Responses 等 native phase provider 使用原生 `commentary` / `final_answer` phase，Chat tagged provider 使用 `<commentary>...</commentary>` 表示运行中的短进展更新、`<final>...</final>` 表示最终答复。streaming 层把这些 provider 输出统一投影为 text part；plan part 只能由 `plan_exit.content` 或后续明确的 plan lifecycle 事件生成。普通 assistant 正文不显示 Chat 标签，Responses native phase 不解析标签，`<proposed_plan>` 按普通未标记文本处理。

`workspaceRoot` 是运行期有效工作区，而不是简单等于 UI 当前选中的目录。Studio 读取 project path 后先解析到规范化目录；如果该目录位于 Git 仓库中，则提升到最近的 Git 仓库根。这样用户从子 crate 或桌面壳层进入项目时，工具仍能访问完整仓库上下文。工作区记忆按 Codex 风格从 Git 根到当前工作目录读取层级文档，候选文件优先级为 `AGENTS.override.md`、`AGENTS.md`、`Agents.md`，并受配置的总字节预算限制。

提示词在核心层按三层组装：`base/system`、`developer`、`user context`。`base/system` 是模型请求的顶层系统提示词，承载跨 Auto/Plan 共用的身份、工作原则、通用工具协作、子代理调度约定和文档同步工作流；`developer` 承载 Auto/Plan 模式差异、平台工具规则、skills 索引和运行约束；`user context` 承载用户配置的上下文偏好和 AGENTS 项目记忆。这些临时前置内容参与模型请求和 token 估算，但不写入普通会话消息历史。session snapshot 只冻结稳定上下文；Auto/Plan mode overlay 是 per-turn 注入，Plan session 后续切到 Auto 实施计划时不得继续携带 Plan-only developer 约束。

系统提示词必须把文档同步作为代码修改前后的稳定约束：涉及架构、接口、行为或项目约定的变更，代理应先阅读相关设计文档和项目记忆，确认计划后先更新 `design/` 下对应文档，再开始实现；实现完成后需要整体回看计划、文档、代码和测试结果，确认交付内容仍与文档和计划一致。若实现过程中发现现有文档与可行方案冲突，应暂停并让用户在遵循现有文档、采纳新方案并同步文档、继续补充需求之间选择。

平台工具规则属于 developer 层，不属于 base/system。`InstructionAssembler` 在 mode prompt 之后、配置 developer 之前注入平台 block：公共规则总是存在，具体系统规则通过编译期 `cfg(windows)` 或 `cfg(unix)` 选择。该 block 随 session instruction snapshot 保存，保证恢复会话或子代理继承时不因运行时提示文件变化而漂移；即使用户配置了 `base_override`，平台 developer block 仍照常注入。

当启用 skills 时，运行时从 `<workspaceRoot>/skills/`、`~/.pure/skills/`、`~/.pure/skills/.system/` 和配置外部目录发现 skills，并在 developer 层注入简短索引。项目 skills 优先于用户、系统和外部只读 skills；自学习写入始终落到 `<workspaceRoot>/skills/`。

策略约束：

- 方案乙不保留旧命令别名和旧字段兜底
- `PermissionMode::RequestApproval` 为默认且主路径；旧 `ToolApprovalPolicy` 仅作为兼容构造
- 手动审批接口保留在系统能力中，但不作为默认流程；`request-approval` 模式只在工具请求 workspace 外访问时弹出用户审批，workspace 内读写直接放行
- `auto-review` 模式使用 reviewer 角色模型审批 workspace 外访问。reviewer 只返回批准或拒绝，不执行工具；解析失败、provider 失败或非明确批准均按拒绝处理
- `full-access` 放宽 Pure 文件工具和 `bash.workingDirectory` 的 workspace 边界，并在策略层直接放行 Plan Mode 中已暴露的 `bash`；该模式仍只执行已注册工具，不提供 OS 沙箱或系统级提权
- Studio 中的 Plan Mode 会保留 `bash` 探索能力；`request-approval` 与 `auto-review` 下 bash 必须走手动审批，`full-access` 下直接放行。明确写入类工具不会暴露给模型，也不能执行模型幻觉出的写入工具调用
- 用户显式要求 `subagent`/子代理分工时，核心提示必须将异步 agent 调度作为强约束；普通 shell 或文件探索不能替代子代理调度
- 显式子代理分工允许最多两轮只读定位；若仍未创建 agent，后续推理只暴露 `spawn_agent` 并保持 `auto` tool choice，避免触发不支持 required tool choice 的 provider 限制
- 多 agent 协作只通过 `spawn_agent`、`wait_agent`、`list_agents`、`send_message`、`followup_task`、`close_agent` 组成，不提供同步等待到最终摘要的 `subagent` 入口；这些工具只提交协作操作，实际 agent 生命周期由 `AgentSupervisor` 统一管理
- `request_user_input` 是 Codex 风格的阻塞交互工具，root agent 与 subagent 都可用；工具通过统一 `Interaction` 域创建 `userInput` 请求，等待前端 resolution 后把答案作为工具结果返回，不作为普通用户聊天消息写入历史
- `planner` 是所有 Studio 根聊天 turn 的唯一模型角色，包括 Auto、Plan 和 Plan 实施；`executor` 只用于 planner 通过子代理工具创建或继续的 child agent turn
- agent 运行时状态对齐 Codex：`queued | running | waiting | completed | errored | interrupted | shutdown | notFound`
- `budgetLimited` 不是 agent 状态，而是 turn abort reason；子 agent 预算耗尽时状态为 `interrupted`，并携带 `reason`、`budgetLimitKind` 和 `budgetUsage`
- `interrupted` 是可恢复的非终局状态；`completed | errored | shutdown | notFound` 是终局状态
- `send_message` / `followup_task` 不能重新激活终局 agent；`followup_task` 不抢占已经 `running` 或已 `queued` 的 agent，调用方应等待该 agent 进入可接收新 turn 的状态；`interrupted` agent 是可恢复状态，可以通过 followup 进入新 turn
- `send_message` 只把消息放入目标 agent 队列，不启动 turn，也不把 `running` / `queued` agent 降级为 `waiting`；queued message 会在下一次 `followup_task` 触发的新 turn 中按入队顺序并入 prompt
- `wait_agent` 等待 supervisor 活动流，只返回 `{ message, timedOut }`；状态明细通过 `list_agents` 和实时 `agentChanged`/`agentTimelineChanged` 获取，不能把完整 agent snapshot 塞回 wait 工具结果
- 父 agent 因中断、错误、预算限制或关闭而停止时，必须级联关闭仍在运行的子树，避免后台子 agent 残留为 `running`
- `AgentSupervisor` 是 agent latest snapshot、运行 handle、取消 token、父子路径和执行容量的唯一状态机入口；内部状态迁移必须集中处理 `status`、`updatedAt`、错误原因与预算详情。进入 `queued`、`running` 或 `completed` 时清理旧的 `error/reason/budgetLimitKind/budgetUsage`，避免从 `interrupted` 恢复后残留旧预算限制；进入 `errored/interrupted/shutdown` 时按当前 turn 或关闭原因写入详情。状态机必须拒绝从终局状态重新激活 agent，`send_message` 对 `queued/running` 保持原状态，`followup_task` 只把可恢复非运行状态推进到 `queued`。

## 3.3 核心 turn 编排

`StudioRuntime` 只做 use case 编排；Studio UI 状态不由命令完成响应驱动，而由 `StudioEventRuntime` 持久事件流驱动。UI-facing runtime 在 `pl-core` 内维护状态机：

```text
Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped
                         │                         │
                         └──────────────► Failed ◄──┘
```

`initializeRuntime()` 完成配置、store、projection 恢复和非终态 turn 收敛；`startRuntime()` 启动后台 health、事件桥接和可取消 turn 执行能力；`shutdownRuntime()` 取消活动 turn、停止后台任务并返回最终 snapshot。所有后台 turn 的 active handle、cancellation token、pending interaction wakeup 和收尾状态都属于 `pl-core::studio`，Flutter 只通过 runtime API 触发。

核心编排步骤：

1. 读取 session/project/config
2. 解析或创建 session 级提示词快照
3. 构造 `TurnRequest` 与 `TurnOptions`
4. 组装 `PureCore`（含工具注册）
5. 执行 `run_turn_with_trace`
6. 运行中每个可见对话 lifecycle 先进入 `StudioEventRuntime`。用户、assistant、commentary、reasoning、plan 和 tool 均规范化为 opencode 式 `StudioMessage` / `StudioPart`。`messageUpdated` 与 `messagePartUpdated` 是 durable snapshot，由 store 在同一个事务中分配 `StudioEventEnvelope.sequence`、写入 `studio_events` 与 `studio_messages/message_parts` projection，再广播同一份 envelope 给 Studio；`messagePartDelta` 只进入实时通道，是 live overlay，不写入 `studio_events`、不推进 durable cursor。
7. turn 收尾只负责消息、最终 runtime snapshot 与生命周期终态校准；不得要求前端等待最终响应才能看到过程

前端提交普通 prompt 或 Plan 实施时调用同一套后台 turn 提交流程。Flutter `submitPrompt(sessionId, prompt, attachmentIds)` 只创建 turn、注册 cancellation token、写入 `turnChanged(queued/contextLoading/waitingForModel)`、用户 `messageUpdated` 和用户 `messagePartUpdated`，随后在后台执行 run，并立即返回 `{ sessionId, turnId, cursor }`。计划确认选择实施时，`resolve_interaction` 在当前 `sessionId` 内解决 interaction、写入 `accepted/implementing` lifecycle，并启动同会话实施 turn；不得创建或切换 target child session，也不得依赖 `sessionHandoffChanged` 展示实施过程。内部实施 prompt 可以标记为 `synthetic/ignored`，避免 timeline 出现一条巨大的重复用户消息。

`run_turn_with_trace` 在每次模型请求前用 `InstructionAssembler` 解析当前提示词快照。base/system 写入 `CompletionRequest.instructions`；developer 块作为临时 system 消息置于历史消息之前；user context 块作为临时 user 消息置于 developer 块之后、真实历史之前。临时前置消息只用于本次 provider request，不写入 `CoreSession`，因此压缩和持久化只处理真实对话历史。

`run_turn_with_trace` 接收新 turn 后，真实用户输入必须已经通过 `submit_prompt` 生成 durable user message/part，并在 `CoreSession` 中作为模型历史写入；随后才能记录 enabled tools、turn running、inference、工具和模型输出等内部运行事件。每个 turn 的用户输入只能对应一个 canonical user text part：message id 为 `{turnId}:user`，part id 为 `{turnId}:user-text`。若内部 trace 仍产生用户输入 snapshot，进入 Studio 协议前必须忽略，只保留为内部诊断，不能覆盖既有 part 或生成第二条用户消息。`StudioEventEnvelope.sequence` 是后端唯一 durable 游标，part 的 `order` 只用于 message 内展示顺序，前端 optimistic 提示不得改变后端游标。

assistant 的 text、commentary、reasoning 和 plan part identity 不得直接使用 provider 局部 `item_id`。provider id 只在单个 inference 内定位当前打开的 stream block；进入 Studio part 前必须生成包含 `{turnId}`、`{inferenceId}` 和语义段序号的稳定 id，例如 `{inferenceId}-reasoning-1`、`{inferenceId}-text-final-1`。同一个 provider item 若出现 text channel/phase 变化，必须先关闭旧 block 再打开新 block，不能把旧 block id 继续用于另一种可见 channel。工具调用是语义边界：遇到 tool start/ready 或 step finish 时，当前打开的 text/reasoning/plan 必须先完成并清理 active provider 映射；工具后的模型输出必须创建新的 part，展示顺序排在工具之后。工具 part 使用 runtime tool call id 保持稳定，使 provider tool snapshot 与 core tool execution snapshot 更新同一个 part。

模型输出的 `commentary` 只进入 timeline，用于让用户看到阶段性进展，不写入 `CoreSession`。只有 `final` 输出会作为 assistant response 写入会话历史；带工具调用的中间轮次如果只输出 commentary，也不得把 commentary 当作 assistant tool-call content 写回 provider 历史。

`run_turn_with_trace` 在每次模型请求前执行自动上下文压缩检查。压缩阈值来自当前模型的 `autoCompactTokenLimit`，未配置时使用有效上下文窗口的 90%；模型没有上下文窗口信息时不触发自动压缩。压缩估算包含 base/system、developer、user context 和真实消息历史。压缩由 `pl-core` 本地摘要完成：用当前模型和固定 compact prompt 生成 handoff summary，再用一条带 metadata 的用户摘要消息加最近真实用户消息替换原始历史。工具调用、工具结果和 assistant 中间过程不以原始片段保留，避免压缩后出现破碎的 tool-call 配对。

子代理没有独立的压缩实现。`AgentSupervisor` 为每个 agent 保存独立 `CoreSession`，`spawn_agent` 和 `followup_task` 提交的 child turn 复用同一个 `PureCore` turn pipeline，因此每个子代理独立维护自己的压缩历史；父会话不会替子代理压缩，也不会因为子代理压缩而改写父历史。

子代理继承父 turn 的 `compileMode` 和稳定 instruction context，但不继承 root 的模型角色。child agent 的模型角色由 `agentType` 决定，默认 executor；只有子代理路径可以使用 executor 角色。child turn 同样按当前 mode 注入 Auto/Plan overlay，并复用同一套工具边界和 proposed-plan 输出约定。

子代理同样继承父 turn 的交互运行时。`request_user_input`、工具审批和计划确认统一表达为 `InteractionKind::{userInput, toolApproval, planConfirmation}`。每个 interaction 都带 `sessionId`、`turnId`、可选 `itemId/toolId/agentPath`，由 `InteractionRuntime` 创建、持久化、广播并等待 resolution。Studio 只渲染当前最高优先级 pending interaction；回答或审批只解除对应等待，不触发新 turn，也不写入普通聊天消息。UI 交互形态对齐 opencode dock prompt：pending question/permission/plan confirmation 在底部 dock 处理，timeline 只渲染 message/part 投影；`request_user_input` 的 completed tool part 可以显示 redacted 问题答案摘要。

主 turn 保存完成后，如果 `[skills].auto_learn = true` 且本轮达到自学习触发条件，`StudioRuntime` 启动后台 reviewer。reviewer 只开放 skills 工具，复盘结果只写项目 skills 目录；失败只记录日志，不改变本轮响应。

文件、shell 和 LSP 查询工具都以有效 `workspaceRoot` 为默认边界。工具输入不要求全部使用绝对路径；相对路径一律按 `workspaceRoot` 解析，而不是按 Pure Studio 进程 cwd。执行前，核心层用统一路径策略把输入解析为规范化绝对路径，并用同一结果做权限预判和实际执行。`bash` 默认在 workspace root 下执行，`workingDirectory` 也按 workspace root 解析并拒绝逃逸；文件工具默认只允许访问 workspace root 内的路径；`lsp_query_*` 的 `filePath` 解析后才交给 `pl-lsp` 生成 file URI。`full-access` 模式会放宽该边界，允许文件路径和 `bash.workingDirectory` 指向 workspace 外，但仍要求 existing 或 existing-parent 可解析，不绕过工具自身校验、写锁、超时、输出截断和 timeline 记录。

Skills 管理工具同样以 `workspaceRoot` 为边界，但写入面收窄到 `<workspaceRoot>/<skills.project_dir>/`。用户级、系统和外部 skills 只读参与发现，不允许被工具原地修改。

工具预算与收尾原则：

- 工具调用或 provider 返回 `end_turn = false` 只表示 `needsFollowUp`，不是完成条件
- root turn 和 child agent 默认只强制 `wallClockMs = 1800000`
- 模型采样、普通工具调用和 `wait_agent` 调用只记录 `modelSteps`、`toolCalls`、`waitCalls` 观测计数，不触发 step/tool/wait 限制
- pending interaction 等待期间仍受当前 turn 的 cancellation token 和 wall-clock 预算约束；用户停止时 pending interaction 被标记为 `cancelled`，wall-clock 到期时标记为 `expired`
- agent tree 默认限制为 `maxAgents = 16`、`maxDepth = 3`
- 预算耗尽属于 `TurnAborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace，不得伪装为 `failed` 或 `completed`
- wall-clock 预算耗尽时核心层按 `budgetLimited` 收尾，并在 trace 中保留预算用量
- 无工具总结或普通 assistant 文本中若出现未执行的工具调用标记，必须按 `budgetLimited` 收尾并写入 `TurnBudgetLimited`；不能把原始 tool-call 文本作为最终回答
- 用户显式要求子代理分工时，turn 完成前必须验证本轮实际创建了 agent；否则按 `failed` 收尾，不写入伪完成 assistant 消息

Agent 协作 timeline 与状态分层：

- agent timeline 是 append-only 协作事件流，只记录 spawn、wait、message、followup、close、final status 等事实事件
- agent tree 是 latest snapshot，只按 `agent_id/path` 覆盖最新状态，供状态栏、树视图和 `list_agents` 使用
- 前端不得用 latest snapshot 渲染 timeline；同一个 agent 的多次状态变化必须在 timeline 中保留为多条独立事件
- `AgentStateChanged` 只用于更新 latest snapshot；UI timeline 消费 `agentEvents` 中的 append-only `SubAgentActivity` event。实时 `agentTimelineChanged` 与历史 `agentEvents` 都必须携带 canonical `StudioAgentTimelineEvent` 语义，Flutter 只解析规范化 payload，不得读取 raw `AgentEvent`。旧 spawn/interaction/wait/close begin/end payload 不再作为运行期协议保留。

持久化原则：

- 消息和内部 `pl-trace` 诊断事件采用事务批量写入，避免逐条写放大；旧 `timeline_events` 表的 entity、运行期写入、读取和清理路径均已删除，迁移历史按 append-only 保留（不再有运行期代码读写该表）
- `studio_events` 是 Studio UI 的唯一 durable 重放事实流。每个 durable 事件带 `sessionId`、会话内单调 `sequence`、`createdAt` 和类型化 `kind`；前端通过 cursor 补拉缺失事件，而不是依赖命令最终响应补状态。广播 payload 必须与持久化 payload 完全一致，禁止 projection 重写一份、实时广播另一份。高频 `messagePartDelta` 是实时 overlay，不写入 durable log，必须能被后续 `messagePartUpdated` 完全覆盖。
- `turns` 表保存当前与历史 turn 状态：`queued | contextLoading | waitingForModel | streaming | waitingForInteraction | runningTool | persisting | completed | failed | cancelled`。启动时所有非终态 turn 必须收敛为 `cancelled`
- `studio_messages`、`message_parts`、`agent_events`、`interactions`、`session_skills` 是 `StudioEventRuntime` 的 projection 表。message/part projection 保存 latest snapshot，live delta 只作为前端 overlay；除一次性迁移和启动恢复外，运行期不得由前端推断直接写入。Plan lifecycle 也必须先写 `StudioEventKind::PlanLifecycleChanged`，再由 projection 更新查询表。旧 `session_handoffs` projection 已通过后续迁移从当前 schema 清理，不再参与运行期读写。
- `message_parts.part_order` 在 part 首次 durable snapshot 时由 `StudioEventEnvelope.sequence` 固化；后续同 part snapshot 即使携带旧 order，也必须保留既有 order，禁止终态 snapshot 或 backfill 改变首次展示位置。
- session 的 `mode` 表示下一轮默认协作模式，由 Studio 模式切换命令持久化；运行时按 session 当前 `mode` 构造 `TurnRequest`
- session 的 `instruction_snapshot_json` 保存首轮解析出的稳定 base/user/project context 和非 mode-specific developer context。已有 session 缺少快照时，在下一轮运行前按当前配置补建。后续配置、模型默认提示词或 AGENTS 文件变化不 retroactively 改写既有 session；新 session 才使用新配置。Auto/Plan mode overlay 每个 turn 重新注入，不能被 snapshot 永久冻结。
- Plan Mode 生成的计划有独立生命周期事件：`pendingConfirmation | accepted | implementing | implemented | implementationFailed | continuedPlanning | dismissed | cancelled`。这些事件由 `StudioEventKind::PlanLifecycleChanged` 广播，并随 durable `studio_events` 重放；前端按 `planId` 折叠最新状态。计划实施确认不是前端从 timeline 自行推断的临时状态，而是后端在当前 live Plan turn 终态后创建的 `planConfirmation` interaction。确认实施沿用 wire 枚举名 `implementFreshContext`，但语义固定为当前 session 内实施：后端把 plan markdown 作为实施 prompt 的唯一意图来源，在同一 `sessionId` 启动新 turn，root turn 继续使用 planner 角色；旧 `session_handoffs` handoff/child session 路径不再作为 Plan 实施入口，也不作为当前 Studio projection。
- `interactions` 表保存所有 pending/resolved/cancelled/expired 交互，是刷新与 session 切换恢复 pending UI 的事实来源。`InteractionChanged` 通过 `StudioEventKind::InteractionChanged` 广播当前 interaction 最新状态；旧 `studio-user-input-*`、`studio-tool-approval-*`、`studio-interaction-changed` sideband 事件不再作为 Studio 协议入口
- `skill_view` 成功激活 skill 时，后端写入结构化 `SkillActivated` 事件并 upsert 会话级 skill runtime fact。Studio 当前会话的 `activeSkills` 只从 `session_skills` 等结构化持久层读取，不能再从 tool result JSON 文本反解析。
- 如果 turn 内发生上下文压缩，`CoreSession` revision 会变化，Studio 以事务重写当前 session 的消息历史并追加本轮 trace；未发生压缩时继续使用追加写入
- StudioEvent 读取以 `sequence` 为 durable 单调游标；message/part snapshot projection 的 `sequence` 必须等于来源 `StudioEventEnvelope.sequence`。`messagePartDelta` 没有 durable sequence 语义，前端不得用它推进 cursor。
- agent tree、agent events、agent messages 与 turn snapshot 分表持久化；`agents` 为 latest snapshot，`agent_events` 为 append-only event log

## 3.4 事件管线

Flutter/FRB 端使用两类订阅：

- `subscribeSessionEvents(sessionId)`：只转发当前会话的 timeline、turn、interaction、session runtime、agent 与高频 `messagePartDelta`。
- `subscribeGlobalEvents()`：只转发项目、配置、Provider usage、MCP/LSP health 等低频全局变化。

`subscribe_session(session_id)` 和 `subscribe_global()` 必须在 `pl-core` 内过滤；Flutter 切换会话时取消旧 session stream，只保留当前打开会话的高频监听。

`StudioRuntime::drain_agent_events` 在 `pl-core` 内使用显式分支处理内部 broadcast 通道状态：

- `Ok(event)`：交给 `StudioEventRuntime` 持久化并广播 `studio-runtime-event`
- `Err(Lagged(n))`：广播 `StudioEventKind::Stale { laggedEvents: n }`，Flutter 按 cursor 调用 `load_studio_events`
- `Err(Closed)`：结束循环

这保证高频 delta 下 UI 不会因为 lagged 直接断流。Flutter bridge 检测到 lagged 时必须为 active session 发 live-only `stale`，驱动前端用 durable cursor 补拉 snapshot。前端按 opencode 的事件批处理方式在 16ms frame 内合并事件：如果同一 part 的 durable snapshot 到达，跳过该 frame 中同 part 尚未应用的旧 live delta，并清除该 part 的 delta overlay；若 snapshot 被 coalescing 覆盖，也必须把同 part pending delta 标成 stale。terminal snapshot 到达后，低序或等序 live delta 不得再修改该 part；带 `chunkIndex` 的 delta 需要按 part 去重。

`StudioEventRuntime` 的运行时职责与映射职责分层维护：运行时入口负责订阅、持久化广播、live/durable 分流和 timeline actor 状态协作；trace 到 Studio 协议的纯映射（message/part id、part 类型/状态、agent timeline、delta field 与文本提取）放在独立 mapper 子模块。mapper 只做确定性结构转换，不访问 store、不广播事件、不分配 durable sequence；所有持久化游标和 projection 更新仍由 `StudioEventRuntime` 与 `StudioStore` 负责。

`StudioRuntime` 保持 use case 门面边界：公开入口仍由 `StudioRuntime` 暴露；runtime 初始化/启动/关闭放入 lifecycle 子模块；项目、会话、配置角色、provider usage 与 skill catalog 查询放入 session-service 子模块；agent/runtime usage 的展示快照映射放入 projection 子模块；skills 自学习触发、阈值统计和后台 reviewer turn 放入 self-learning 子模块。子模块不得直接替代 runtime 发事件或写 store，只返回确定性 projection、执行门面方法对应的持久化动作，或启动明确的后台 review 任务。

`messagePartDelta` 只用于 live overlay。即使底层为了诊断保留了 delta 事件，也不得写入 `studio_events`。`stale` 也是 live-only 补拉提示，不占用 durable sequence，不参与历史重放。`load_session_state` 从 `studio_messages/message_parts` projection record 恢复终态，每条 record 必须携带来源 event sequence 供前端建立新旧 guard，并附带非 message/part durable 状态事件；`load_studio_events` 只回放 durable snapshot 与状态事件，历史恢复不得依赖 delta。旧 `timeline_events` 表的实体、写入、读取与 cursor API 均已从运行期删除；其 drop/创建语句作为 append-only 迁移历史保留，但运行期不再有代码读写该表。

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
- `BridgeStudioSnapshotResponse`、`BridgeSessionStateResponse`、`ProviderUsagesResponse`、`SkillsResponse`、`SubmitPromptResponse`、`StopPromptResponse`、`ResolveInteractionResponse`、`SettingsDraftResponse`、`ConfigSavedResponse`：FRB typed DTO。完整 config 与 general settings 作为 snapshot DTO 的 `configJson`/`generalSettingsJson` 留在 Dart adapter 边界。

Dart FRB adapter 从 `BridgeEventPayload` sealed union 归一出 app 内部 typed `StudioBridgeEventPayload`；Riverpod reducer 只按 payload 类型更新 store，不再读取 `event.payload[...]` Map。实时 stream 与 `loadStudioEvents` backfill 必须共用这套 typed envelope。命令与 snapshot 返回不使用 `JsonResponse` 外壳；Dart 只在 adapter 边界解 `configJson`、`generalSettingsJson` 和工具参数这类开放 JSON 标量。agent timeline 在 FRB 边界使用 typed payload union；Flutter 不解析历史 `payloadJson` agent event 记录，持久层必须在进入 Flutter 前投影为 typed `BridgeAgentTimelineEventDto`。

Flutter 桥接动作按同一 runtime 边界命名：`bootstrapStudio`、`openProject`、`selectProject`、`createSession` 和 `archiveSession` 返回新的 Studio 快照，`setSessionMode` 持久化当前 session 的下一轮协作模式，`setModelRole` 写回 provider/role 配置并返回 canonical config view，`submitPrompt`/`stopPrompt`/`resolveInteraction` 只表示请求已提交，`loadSessionState`/`loadStudioEvents` 用于会话恢复与 stale backfill，`saveRuntimePermissionMode` 写回 runtime config，`saveStudioSettingsDraft` 持久化尚未 typed 化的设置页草稿。
