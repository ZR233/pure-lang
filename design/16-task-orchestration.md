# 16 - Simple / Task 与 TaskService

## 16.1 模式

root Thread 模式只有 `simple | task`。

- Simple：root 使用 executor role，可直接实现；只允许派生只读 explorer。
- Task：root 使用 planner role。planner 负责计划、设计、executor、review、merge、冲突和完成。

每个 agent 固定对应一个 Thread。child 通过 `rootThreadId`、`parentThreadId`、`role` 和
`agentPath` 表达关系。TaskRun 只绑定 root Thread；executor 和 reviewer 直接由 WorkUnit、
ReviewRound 引用，不建立 AgentOutcome 镜像。

## 16.2 所有权

`TaskService` 位于 `pl-studio-runtime`，管理：

- TaskRun 与 phase；
- WorkUnit 与不可变 WorkCompletion；
- ReviewRound；
- MergeRecord 与冲突证据；
- BranchLease、worktree ownership 与安全清理。

Thread/Turn 的执行状态只从 Thread repository 读取。Task 状态从产品表直接组成
`TaskSnapshot`，只进入 product stream。planner 执行的 Task 工具仍是 planner Thread 自己的
toolCall Item。

## 16.3 Task 状态

Task phase 为：

```text
planning → pendingConfirmation → designUpdating → implementing → merging
         → resolvingConflict → reviewing → reworking
         → stopping → completed | blocked | failed | cancelled
```

WorkUnit 直接保存 executorThreadId、requestedByCallId、attempt、ownedPaths、baseCommit、
worktree、branch、状态、summary/error 和 cleanup disposition。ReviewRound 直接保存
reviewerThreadId、scope、目标 completion/HEAD、verdict 和 findings。

WorkUnit 状态为 Pending、Running、AwaitingCompletion、ReadyForReview、Reviewing、
ChangesRequested、Approved、Merging、Merged、NoDelivery、Failed、Cancelled。

每次 durable transition 在一个 SQLite 事务中更新所有相关产品记录。Task phase 不复制进
Thread runtime snapshot，Thread 状态也不缓存进 Task 表。

## 16.4 Planner 与等待

Task root 只允许 planner 创建 explorer；executor 通过 `task_spawn_executor` 创建，reviewer
通过 delivery/integrated review 工具创建。executor/reviewer depth 固定为 1。

`wait_agents` 订阅 Thread directory watch 后读取 snapshot，只因 progress、interaction 或
terminal 变化返回。没有轮询、自动续轮或超时中断。五分钟仅允许 planner 读取有界 child
Thread 诊断，不是失败判据。

进程重启后不自动启动模型。活动 Task 无 pending input 时显示 paused；用户“继续任务”以稳定
mail ID 向 root Thread 提交一次隐藏的明确输入，要求 planner 先读取 `task_status` 和
`list_agents`。attach 只对账，不触发 Turn。

## 16.5 Executor 与交付

`task_spawn_executor` 必须提供 taskName、message 和非空 ownedPaths。TaskService 在创建 child
Thread 前校验路径规范、重叠、并发上限、phase 和 branch lease，再准备专属 worktree。

executor 只能写自己的 worktree，并以以下工具结束可交付工作：

```text
report_completion {
  delivery { headCommit, verificationSummary }
  | noDelivery { verificationSummary }
}
```

delivery 要求 worktree clean、HEAD 相对固定 base 推进、commit 身份一致且变更不越过
ownedPaths。成功事务创建不可变 WorkCompletion 并将 WorkUnit 置为 ReadyForReview。普通文本
结束、工具错误或预算中止不会伪造交付，WorkUnit 保持 AwaitingCompletion，可由 planner 向同一
Thread 发送明确 follow-up。

WorkUnit 在 ReadyForReview 之后创建 fresh reviewer。findings 使其进入 ChangesRequested；
planner 把具体 finding 发回原 executor Thread，新的 completion revision 重新审查。pass 后
WorkUnit 进入 Approved 或 NoDelivery。

## 16.6 合并、冲突与综合审查

planner 只能合并 Approved 且已关闭模型生命周期的 executor。`task_merge_agent` 在 branch
mutation lock 内重新验证 TaskRun、BranchLease、named branch、Git common directory、clean
workspace、expected HEAD、delivery commit 和 worktree ownership。

无冲突时执行相关验证并创建 focused merge commit，再在事务中推进 TaskRun、BranchLease、
MergeRecord 和 WorkUnit。Git 已提交而数据库 CAS 失败时，只有能证明现场仍精确属于本次操作
才允许安全补偿，否则保留现场并 block。

冲突时保留 MERGE_HEAD 和 index，持久化 typed conflict manifest，并进入 resolvingConflict。
planner 只能通过 merge 工具读取和修改 manifest 中的文件；verify/continue 成功后推进 HEAD，
abort 或无法证明现场安全时 block，不覆盖用户修改。

所有 WorkUnit 均为 Merged 或 NoDelivery、design 已与当前 HEAD 一致后，创建 fresh integrated
reviewer。findings 进入 reworking，由新 executor 修复；pass 才允许 `task_complete`。

## 16.7 设计门禁

用户确认实施后，planner 必须先用 `task_update_design` 提交 `design/**` 的 focused patch。
解析、路径、安全检查、应用和提交是 all-or-nothing；失败恢复已触及路径和 index。

成功后以旧 HEAD 为 CAS，在同一事务推进 TaskRun 与 BranchLease expectedHead 并记录
designCommit。存在 source merge 时，完成或取消前 design 必须再次与当前实现一致。

## 16.8 Lease、停止与恢复

同一 Git common directory 与分支只有一个 BranchLease。所有设计、merge、冲突、完成和取消
共享 branch mutation lock；持锁后必须重新读取数据库和 Git 现场，不能依赖旧预检。

stop 先写 typed StopRequested 并禁止新 allocation，再 interrupt 活动 Turn。存在未报告 commit
或 dirty worktree 时返回 deferred，保留成果供 planner 处理；只有全部 completion contract 已
收束才进入 stopping、清理 Pure-owned worktree、处理 design 一致性并在事务中写 cancelled 与
删除 lease。

启动恢复把遗留 inProgress Turn/Item 标记为 interrupted(runtimeRestarted)，把 pending reviewer
round 收束为失败，并按精确 completion/HEAD 恢复 WorkUnit。Task phase、delivery、review、
merge、worktree 和 lease 均从产品表恢复；没有 pending input 时保持 paused。

任何 run、Thread、WorkUnit、review、merge、lease 或 Git 身份配对失败都只 block 精确作用域，
不击穿其他 Project。恢复不重建物理模型连接、不启动 continuation、不删除外部资源。

## 16.9 清理安全

worktree 路径和分支包含 run 与 executor Thread 身份：

```text
.pure/worktrees/<taskRunId>/<threadId>
pure-task-<runId>-<threadId>
```

恢复清理和项目关闭遵循 preview → confirmation → execution-time revalidation。预览列出 path、
branch、存在状态、dirty、ahead、changed files 和 expected revision。执行只处理数据库证明归属
且仍与预览一致的 `pure-task-*` 分支与 `.pure/worktrees/**`；绝不删除用户主工作区。

旧数据库归档时只生成 Task/worktree/branch manifest，不自动删除任何 worktree 或 branch。

## 16.10 完成条件

`task_complete` 要求：

- design 与当前 HEAD 一致；
- 全部 WorkUnit 为 Merged 或 NoDelivery；
- 最新 integrated review 针对当前 HEAD 且 verdict 为 pass；
- 当前分支、workspace、TaskRun 和 BranchLease expectedHead 精确一致；
- 最终验证通过且不存在 StopRequested。

完成事务写 completed 并删除 BranchLease。任何迟到 child completion、旧 generation 或旧 Turn
通知都不能改变已提交的 Task 终态。
