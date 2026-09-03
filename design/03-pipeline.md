# 03 - Thread / Turn / Item 流程

## 3.1 输入与 Turn

`startTurn(threadId, input, clientInputId)` 只在目标 Thread 空闲时创建 queued Turn；活动 Turn 的
补充输入使用 `steerTurn`。稳定输入 ID 保证重复提交幂等。提交事务写入输入、Turn 和 user Item，
成功后更新 owner snapshot 并广播 typed notification；只有 presentation 为 `visible` 的输入生成 user
Item。

`MessagePresentation::{Visible, Hidden}` 是所有消息共用的协议属性，不属于 Plan、Interaction 或 mailbox
特例。`Visible` 是省略时的默认值；`Hidden` 消息仍是 canonical AgentSession transcript，必须持久化并
完整发送给 provider，但 Thread 产品 projector 不为它产生 GUI Item/notification。输入进入 Turn 时把同一
presentation 从 durable mailbox 复制到 `TurnRequest` 和 `Message`，不得在宿主或 GUI 重新推导。上下文
压缩生成的摘要、压缩指令和 synthetic user input 使用 `Hidden`，未来其他内部输入也在创建点选择该值。
`MailboxInputSource::{User, ParentAgent}` 独立记录可见输入的来源，旧记录缺失时按 `User` 恢复；来源在
runtime 创建输入时冻结并随 mailbox 持久化，Thread projector 将 `ParentAgent` 投影为独立文本通道，
Studio 不根据 Thread 层级、消息正文或工具名猜测来源。

TurnFactory 从同一 Thread 事实构造：统一 root 指令、项目 `AGENTS.md`、当前 Thread Mode 快照、
可选的 `pl.workflow` projection、普通 Skill、工具快照与 provider route。Simple/Task 不选择
不同的模型循环；root 一律继承统一 planner route。

## 3.2 模型循环与工具

provider response 依次形成 assistant Item、tool call、tool result 与下一轮输入。普通工具可以并批；
`workflow_transition`、`workflow_restart` 与 `complete` 声明 `Solo`，同一 response 中若还有任何其他
调用则整批拒绝且无副作用；workflow 查询工具为 `Coexist`。

工作流状态调用在 working-state clone 上计算。成功调用的 assistant tool call、tool result 与新
working state 必须由一个 Thread checkpoint 原子提交；失败时三者共同回滚。工具结果在同一 Turn
立即返回新阶段约束，下一 Turn 和 context compaction 后由最新 `pl.workflow` section 继续约束。

## 3.3 Interaction

Interaction 只有通用 `UserInput` 与 `ToolApproval`。Task root 使用独立固定状态机的 `plan_*` 工具管理
计划 lifecycle；`plan_submit` 提交以一级 Markdown 标题开头的完整计划并请求批准或修订，缺失信息、
澄清和普通选项输入使用 `request_user_input`。`request_user_input` 不得询问是否实施、继续或批准完整
计划；计划已经完整时必须直接调用 `plan_submit`，其 Plan confirmation 是唯一实施授权入口。两者都生成
同一种通用 `UserInput` Interaction，Plan 只以 typed purpose 绑定状态机，不增加 PlanConfirmation kind。
Studio 可以用稳定的 `plan_confirmation` question ID 派生 pending Plan 的 Timeline 摘要、右侧详情和替换式
反馈栏，但不能据此推演 Plan 状态、复制正文或创建第二条 resolution 路径。pending Interaction 随 Thread
恢复，响应必须匹配 interaction identity 和 Plan revision，并且只能决议一次。完整合同见
[24-agent-session-plan.md](24-agent-session-plan.md)。

`InteractionRequest.continuation` 是通用的不可变 continuation 预设，声明 resolution 后用户消息的内容
来源和 `MessagePresentation`，并参与 request identity 与持久化。Studio 的 Plan question ID 特判只影响
pending UI 形态，不得用来推导 continuation 内容或可见性；没有预设的 UserInput 不得猜测 continuation。

## 3.4 工作流生命周期

带图 Mode 的首个根用户输入在 provider 前自动生成 lineage/run。`workflow_transition` 用
run/revision/stage 三重 CAS 完成当前阶段并沿直接边进入下一阶段；进入 terminal stage 后 run 立即终止
但 Turn 可继续交付。图 hash 变化由下一个 Turn 自动归档旧 run 并创建同 lineage replacement；正常
终态后的新任务自动创建新 lineage。所有 root Mode 在完成工作后都调用 `complete` 结束当前 Turn。

Workflow 不拥有代码、文件、Git 或 Agent。任何阶段都可使用普通工具；图只约束状态记录和后续提示。
Mode Prompt 可以在不裁剪工具的前提下声明合作式角色边界，例如要求 Task root 把普通实现交给 child、只在
文档与整合阶段亲自写入。该约束属于冻结 Mode 指令，不是 runtime effect 白名单或 OS 沙箱。

## 3.5 协作

root 通过 `list_agent_profiles` 选择 Profile，再以 `spawn_agent(profileId, ...)` 创建 child。生成时冻结
Profile 指令、模型路由与 workspace assignment。child 使用普通 Thread/Turn/Tool，但不拥有 root 的
workflow 工具。Task root 在 planning 开始和每批 child 交付后维护成本感知的任务依赖 DAG：节点包含
前置依赖、读写范围、Profile、交付证据与 root-only 标记；所有依赖已满足、边界清楚、可独立验收且
预计能缩短关键路径或显著增加独立证据的节点形成 ready frontier。root 必须在首次等待前派出该前沿的
全部 child，等待期间继续处理未委托的综合与编排工作，收齐 durable delivery 后释放下一前沿。
planning 中按独立事实域使用 `explorer`，复杂依赖分析可使用一个不重复 root 工作的 `planner`；working
中按文件所有权与依赖图把实现交给 `executor` 或 `worktree_executor`，integrating 中由 root 审查和
显式整合结果。整合后的主 workspace 必须由新的只读综合 `reviewer` 总审；范围较广或风险面独立时，
同一 review wave 可再并行派出专项 reviewer。最终 wave 的全部 reviewer 都必须提交 durable approval；
任一阻塞 finding 都回到 working 或 editing_documents，修复和重新整合后重新 review。不存在隐式 merge
或 delivery gate。
这里的只读约束针对项目文件、Git、worktree 和外部持久状态；reviewer 应以 `report_progress` 写入
协作层的 durable 审查报告，root 再按该 reviewer 的冻结 `agentId` 通过
`read_agent_submissions` 读取 verdict。该报告只承载 finding 或 approval，不授予 reviewer 修复权，
也不能由 root 自述或自由文本 session 摘要替代。

每个 child 消息必须自包含地给出目的、设计基线、所有权、禁止范围、有序步骤、完成/失败条件、证据
输出和 workspace/Git 合同。语义独立且写集合互斥的任务应并行；只有极少操作的微任务、重复目标、
需要共享未稳定上下文或真实前后依赖的工作不得为增加 agent 数量而拆分。directory child 适合单任务或
互斥目录并行，可能触及共同接口、清单、生成边界或 Git 状态的任务使用独立 worktree。worktree 只隔离
现场，不能消除语义依赖；root 仍负责顺序采纳 commit 和处理冲突。
`spawn_agent.message` 与后续 `send_message.message` 都作为 `Visible + ParentAgent` 输入进入 child 的
canonical transcript 和 Timeline；前者由首轮 `TurnStarted` 补齐尚未存在的稳定 Item，后者继续按目标
当前状态启动或 steer Turn。相同稳定 mailbox Item 已经由 `TurnQueued` 投影时不得重复生成。
`plan_submit` 使用 typed `{expectedRevision, plan}`、Markdown 标题校验、结构化状态机 receipt 与提交后
结束 Turn 的语义，并通过通用 `UserInput` 发起确认；`request_user_input` 和普通文本追问都不得替代计划
提交或重复询问实施授权。运行时只在
`AgentWorkingState.plan` 保存当前 AgentSession 的有界状态，不维护专用 `PlanCompleted`、Plan trace part
或 Thread plan item 投影链。批准后的完整 Plan 通过 `Hidden` mailbox continuation 作为 user message
进入同一 session transcript，但不投影到 GUI Timeline，也不跨 AgentSession 共享。
