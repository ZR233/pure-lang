# 03 - Thread / Turn / Item 流程

## 3.1 输入与 Turn

`startTurn(threadId, input, clientInputId)` 只在目标 Thread 空闲时创建 queued Turn；活动 Turn 的
补充输入使用 `steerTurn`。稳定输入 ID 保证重复提交幂等。提交事务写入输入、Turn 和 user Item，
成功后更新 owner snapshot 并广播 typed notification。

TurnFactory 从同一 Thread 事实构造：统一 root 指令、项目 `AGENTS.md`、当前 Mode Skill 或冻结
Mode snapshot、`pl.workflow` projection、普通 Skill、工具快照与 provider route。Simple/Task 不选择
不同的模型循环；root 一律继承统一 planner route。

## 3.2 模型循环与工具

provider response 依次形成 assistant Item、tool call、tool result 与下一轮输入。普通工具可以并批；
`workflow_state` 声明 `Solo`，同一 response 中若还有任何其他调用则整批拒绝且无副作用。

工作流状态调用在 working-state clone 上计算。成功调用的 assistant tool call、tool result 与新
working state 必须由一个 Thread checkpoint 原子提交；失败时三者共同回滚。工具结果在同一 Turn
立即返回新阶段约束，下一 Turn 和 context compaction 后由最新 `pl.workflow` section 继续约束。

## 3.3 Interaction

Interaction 只有通用 `UserInput` 与 `ToolApproval`。模式确认、澄清和选项输入统一通过
`request_user_input`；不存在专用 PlanConfirmation continuation。pending Interaction 随 Thread
恢复，响应必须匹配 interaction identity 并且只能决议一次。

## 3.4 工作流生命周期

首次 `workflow_state.compile` 生成 lineage/run 并冻结 Mode Skill。`transition` 用 run/revision/stage
三重 CAS 完成当前阶段并沿直接边进入下一阶段；进入 terminal stage 后 run 立即终止但 Turn 可继续
交付。活跃目标发生实质变化时使用 `supersede` 原子归档旧 run 并创建同 lineage replacement；正常
终态后的新任务用 `compile` 创建新 lineage。

Workflow 不拥有代码、文件、Git 或 Agent。任何阶段都可使用普通工具；图只约束状态记录和后续提示。

## 3.5 协作

root 通过 `list_agent_profiles` 选择 Profile，再以 `spawn_agent(profileId, ...)` 创建 child。生成时冻结
Profile 指令和模型路由。child 使用普通 Thread/Turn/Tool，会话共享项目 workspace，但不拥有 root 的
`workflow_state`。root 负责整合结果和推进阶段，不存在隐式 worktree、merge 或 delivery gate。
计划正文是普通 assistant 内容，确认由 `request_user_input` 承载；运行时不再维护专用
`PlanCompleted`、Plan trace part 或 Thread plan item 投影链。
