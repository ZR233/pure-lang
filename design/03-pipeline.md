# 03 - Thread / Turn / Item 流程

## 3.1 提交输入

`startTurn(threadId, input, clientInputId)` 只在目标 Thread 没有活动 Turn 时创建 queued Turn。
`steerTurn` 把明确输入写入同一 durable input 表并交给活动 Turn；内部 agent `send_message`
根据目标是否活动选择 steer 或新 Turn。`clientInputId`/`mailId` 全局稳定且幂等。

提交顺序固定为：

1. ThreadActor 校验 Thread lifecycle、当前 Turn 和稳定输入 ID。
2. 单库事务插入 input，必要时同时创建 queued Turn 和 userMessage Item。
3. 事务成功后更新 actor 内存状态并广播 typed notification。
4. TurnFactory 准备 TurnEngine、instructions、tools 与 execution policy。
5. worker 执行 Turn；完成回调必须同时匹配 TurnId 与进程内 identity。

普通用户输入生成一个 userMessage Item。SyntheticHidden 输入只进入模型上下文和 durable input，
不生成伪用户 Item；SyntheticVisible 仅供明确要求展示的产品输入。

## 3.2 Turn 与 Item

`Turn.status` 为 `queued | inProgress | completed | failed | interrupted`。活动 phase 为
`preparing | thinking | responding | planning | runningTool | persisting`。
Thread 没有 active Turn 即 idle，不持久化第二套 agent activity 或 last outcome。

pending Interaction 是一种成功的 completion boundary：当 Turn 因 pending
Interaction 结束时，Turn 落 `completed`，不进入专门的“等待交互” phase。“是否
等待用户”由 Thread 上挂的 pending Interaction 派生，不进 Turn 状态机。

Item 使用固定 ordinal，首次创建后 `threadId/turnId/kind/ordinal/createdAt` 不可改变。Item kind：

- `userMessage`
- `agentMessage { phase: commentary | final }`
- `reasoning { summary, content }`
- `plan`
- `toolCall`
- `file`
- 内部 `contextCompaction`

Item start 持久化 inProgress 行；delta 只更新 ThreadActor live overlay；terminal Item 在单库事务中
写入完整 payload。进程崩溃只会丢失 transient delta，恢复时把未终态 Item 收束为 interrupted。

commentary 只进入可见历史，不写成模型 assistant final；final agentMessage 才进入下一次模型
请求的普通 assistant 历史。模型 transcript 通过 `thread_context_segments` 增量持久化，
`AgentWorkingState` 独立保存 pinned sections、Evidence Ledger、session note 与 prompt 状态；每次
inference 只在 transcript 末尾物化一份最新 working-context tail，不创建 Item，也不向 Flutter
暴露。`contextCompaction` 只保留为无正文内部审计 Item，并标记 transcript replacement。当前 Todo、
usage 和瞬时 progress 等产品事实仍只由 `ThreadRuntimeSnapshot` 拥有，working state 不能反向驱动
产品状态。`ProgressEmitter` 的 milestone 是用户可见的阶段 commentary，必须通过 durable trace
投影为 `agentMessage { phase: commentary }`；历史读取、重启恢复与重新订阅都必须保留它。

### 3.2.1 工具批次执行与结果预算

同一次模型响应产生的多个工具调用按 provider 顺序建立 canonical toolCall，并可在同一轮调度中
并发执行；每个结果仍按原顺序、原 callId 与对应调用配对后，一次性进入下一次模型请求。并行只适用于
工具自身明确声明可并行且运行时锁为共享或无锁的调用。MCP 工具只有服务器配置提供可信的
`ToolEffect::Read` 时才能声明可并行并进入共享锁；effect 未知或可能写入的 MCP 工具保持独占，
不从第三方 annotation 隐式提升权限。

成功工具批次在写入 session 前应用统一的模型可见结果总预算。预算以上下文压缩阈值的剩余空间为
输入，并受固定批次上限约束；分配采用稳定的公平水位算法，小结果优先完整保留，大结果共享剩余预算。
所有 tool result 和 callId 都必须保留，顺序不得改变。批次裁剪只修改 durable history 中模型可见的
结果及其 visibleBytes 指标；UI display result、原始字节数、原始内容哈希和 artifact 引用保持不变，
确保模型后续输入与持久化回放一致，同时不丢失用户可见诊断信息。

每次 inference 与其后工具批次保存一份可关联的编排诊断：请求的工具 schema 估算 token、模型
返回的工具调用数、可并行候选数、实际并行数、批次 wall-clock 与并行关键路径、写回模型的批次
结果估算 token，以及只读重复调用的缓存命中数。指标只保存计数、时长、token 估算和稳定类别，
不保存工具参数或结果正文。一次批次并行省时定义为串行执行耗时之和减去批次 wall-clock；没有
并行候选时该值为零，不根据调用数量推测收益。

## 3.3 Interaction

Interaction 是独立 durable server request，带 threadId、turnId 和可选 itemId/toolId/agentPath。
同进程回答唤醒 waiter；重启后回答 userInput/planConfirmation 时，以稳定 mail ID 创建明确后续
输入。toolApproval 重启后一律取消，避免重复外部副作用。

Flutter 同时只展示当前 Thread 最高优先级 pending interaction：toolApproval、userInput、
planConfirmation。Interaction 不进入普通 timeline；对应 tool Item 可在完成后展示脱敏摘要。

## 3.4 多代理

一个 agent 固定拥有一个 Thread，ThreadId 同时是 runtime actor identity。agentPath 是模型工具
可读地址，由 resolver 映射到 ThreadId。child Thread 保存 rootThreadId、parentThreadId、role
和 agentPath；父 timeline 只展示父自己执行的 agent-control tool Item，不复制 child 输出。

ThreadManager 只串行 spawn/close 和 directory mutation。普通 start/steer/interrupt 通过
ThreadHandle 直达 ThreadActor，不经过第二层 coordinator。`wait_agents` 订阅 directory watch，
没有轮询、inactivity timer 或 synthetic continuation。

## 3.5 Task

TaskRun 绑定 rootThreadId。WorkUnit 绑定 executorThreadId，ReviewRound 绑定 reviewerThreadId；
不存在独立 AgentOutcome 表。WorkUnit/ReviewRound 保存产品授权所需的 call ID、attempt、role、
status 与 error，Thread/Turn 提供运行状态和 progress。

Task 更新只进入 product stream。TaskService 保留文档编辑、worktree、delivery review、merge、
conflict、integrated review、stop、问题记录与安全清理合同；这些状态不进入 Thread stream。

## 3.6 恢复

启动在一个事务中：

1. 标记遗留 active Turn/Item 为 interrupted(runtimeRestarted)。
2. 将未确认消费的 input 恢复 queued。
3. 取消 toolApproval，保留 userInput/planConfirmation。
4. 恢复 Thread directory，不启动任何模型。
5. TaskService 对账 lease、worktree、merge 和 review；局部失败生成 scoped recovery issue。

没有 pending 明确输入的 Task 显示 paused。attach、恢复和 product event 都不能自动创建 Turn。
