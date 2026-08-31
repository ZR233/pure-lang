# 03 - Thread / Turn / Item 流程

## 3.1 输入与 Turn

`startTurn(threadId, input, clientInputId)` 只在目标 Thread 空闲时创建 queued Turn；活动 Turn 的
补充输入使用 `steerTurn`。稳定输入 ID 保证重复提交幂等。提交事务写入输入、Turn 和 user Item，
成功后更新 owner snapshot 并广播 typed notification。

TurnFactory 从同一 Thread 事实构造：统一 root 指令、项目 `AGENTS.md`、当前 Mode Skill 或冻结
Mode snapshot、可选的 `pl.workflow` projection、普通 Skill、工具快照与 provider route。Simple/Task 不选择
不同的模型循环；root 一律继承统一 planner route。

## 3.2 模型循环与工具

provider response 依次形成 assistant Item、tool call、tool result 与下一轮输入。普通工具可以并批；
`workflow_state` 与 `complete` 都声明 `Solo`，同一 response 中若还有任何其他调用则整批拒绝且无副作用。

工作流状态调用在 working-state clone 上计算。成功调用的 assistant tool call、tool result 与新
working state 必须由一个 Thread checkpoint 原子提交；失败时三者共同回滚。工具结果在同一 Turn
立即返回新阶段约束，下一 Turn 和 context compaction 后由最新 `pl.workflow` section 继续约束。

## 3.3 Interaction

Interaction 只有通用 `UserInput` 与 `ToolApproval`。模式确认、澄清和选项输入统一通过
`request_user_input`；不存在专用 PlanConfirmation continuation。pending Interaction 随 Thread
恢复，响应必须匹配 interaction identity 并且只能决议一次。

## 3.4 工作流生命周期

当 Mode Skill 选择使用 workflow 时，首次 `workflow_state.compile` 生成 lineage/run 并冻结 Mode Skill。`transition` 用 run/revision/stage
三重 CAS 完成当前阶段并沿直接边进入下一阶段；进入 terminal stage 后 run 立即终止但 Turn 可继续
交付。活跃目标发生实质变化时使用 `supersede` 原子归档旧 run 并创建同 lineage replacement；正常
终态后的新任务用 `compile` 创建新 lineage。所有 root Mode 在完成工作后都调用 `complete` 结束当前 Turn。

Workflow 不拥有代码、文件、Git 或 Agent。任何阶段都可使用普通工具；图只约束状态记录和后续提示。
Mode Skill 可以在不裁剪工具的前提下声明合作式角色边界，例如要求 Task root 把普通实现交给 child、只在
文档与整合阶段亲自写入。该约束属于冻结 Mode 指令，不是 runtime effect 白名单或 OS 沙箱。

## 3.5 协作

root 通过 `list_agent_profiles` 选择 Profile，再以 `spawn_agent(profileId, ...)` 创建 child。生成时冻结
Profile 指令、模型路由与 workspace assignment。child 使用普通 Thread/Turn/Tool，但不拥有 root 的
`workflow_state`。Task root 在 planning 中优先把可独立的探索并行交给 `explorer`，在 working 中按文件
所有权与依赖图把实现交给 `executor` 或 `worktree_executor`，并在 integrating 中审查和显式整合结果。
整合后的主 workspace 必须由新的只读 `reviewer` 总审；阻塞 finding 回到 working 或
editing_documents，修复和重新整合后再次 review。不存在隐式 merge 或 delivery gate。
这里的只读约束针对项目文件、Git、worktree 和外部持久状态；reviewer 应以 `report_progress` 写入
协作层的 durable 审查报告，root 再按该 reviewer 的冻结 `agentId` 通过
`read_agent_submissions` 读取 verdict。该报告只承载 finding 或 approval，不授予 reviewer 修复权，
也不能由 root 自述或自由文本 session 摘要替代。

每个 child 消息必须自包含地给出目的、设计基线、所有权、禁止范围、有序步骤、完成/失败条件、证据
输出和 workspace/Git 合同。语义独立且写集合互斥的任务应并行；真实前后依赖保持顺序。directory child
适合单任务或互斥目录并行，可能触及共同接口、清单、生成边界或 Git 状态的任务使用独立 worktree。
worktree 只隔离现场，不能消除语义依赖；root 仍负责顺序采纳 commit 和处理冲突。
计划正文是普通 assistant 内容，确认由 `request_user_input` 承载；运行时不再维护专用
`PlanCompleted`、Plan trace part 或 Thread plan item 投影链。
