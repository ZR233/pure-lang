# 16 - Simple / Task 与 TaskService

## 16.1 模式

root Thread 模式只有 `simple | task`。

- Simple：root 使用 executor role，可直接实现；只允许派生只读 explorer。
- Task：root 使用 planner role。planner 负责计划、设计、executor、review、merge、冲突和完成。

root 的 mode 与 role 是同一不变量，只能在没有活动 Turn、pending input 和活动 Task 时切换。
启动恢复在创建 ThreadActor 前修复旧数据库中 `simple/planner` 或 `task/executor` 的 root 记录；
child role 不参与该修复。Task root 永远不能进入 executor WorkUnit 生命周期。

每个 agent 固定对应一个 Thread。child 通过 `rootThreadId`、`parentThreadId`、`role` 和
`agentPath` 表达关系。TaskRun 只绑定 root Thread；executor 和 reviewer 直接由 WorkUnit、
ReviewRound 引用，不建立 AgentOutcome 镜像。

## 16.2 所有权

`TaskService` 位于 `pl-studio-runtime`，管理：

- TaskRun 与 phase；
- WorkUnit 与不可变 WorkCompletion；
- ReviewRound；
- MergeRecord 与 Planner Git 记账；
- BranchLease、worktree ownership 与安全清理。

Thread/Turn 的执行状态只从 Thread repository 读取。Task 状态从产品表直接组成
`TaskSnapshot`，只进入 product stream。planner 执行的 Task 工具仍是 planner Thread 自己的
toolCall Item。

## 16.3 Task 状态

Task phase 为：

```text
planning → pendingConfirmation → designUpdating → implementing → merging
         → reviewing → reworking
         → stopping → completed | blocked | failed | cancelled
```

WorkUnit 直接保存 executorThreadId、requestedByCallId、attempt、scopeHints、baseCommit、
worktree、branch、状态、summary/error 和 cleanup disposition。ReviewRound 直接保存
reviewerThreadId、scope、目标 completion/HEAD、verdict 和 findings。

WorkUnit 状态为 Pending、Running、AwaitingCompletion、ReadyForReview、Reviewing、
ChangesRequested、Approved、Merged、NoDelivery、Failed、Cancelled。

每次 durable transition 在一个 SQLite 事务中更新所有相关产品记录。Task phase 不复制进
Thread runtime snapshot，Thread 状态也不缓存进 Task 表。

## 16.4 Planner 与等待

Task root 只允许 planner 创建 explorer；executor 通过 `task_spawn_executor` 创建，reviewer
通过 delivery/integrated review 工具创建。executor/reviewer depth 固定为 1。

`wait_agents` 订阅 Thread directory watch 后读取 snapshot，只因 progress、interaction 或
terminal 变化返回。没有轮询、自动续轮或超时中断。五分钟仅允许 planner 读取有界 child
Thread 诊断，不是失败判据。

review request 成功创建 reviewer 后必须结束当前 planner Turn。reviewer 提交 durable verdict 后，
Runtime 等待 root Thread idle，再以稳定 mail ID 提交一次隐藏 continuation；新的 planner Turn 从
最新 Task phase 重新解析 canonical workspace 与 tool policy。这样 delivery pass 进入 Merging 后才
授予主 workspace 写入、普通 exec 与 Git 能力，旧 Turn 的只读 snapshot 不会被阶段变化旁路。

进程重启后不自动启动模型。活动 Task 无 pending input 时显示 paused；用户“继续任务”以稳定
mail ID 向 root Thread 提交一次隐藏的明确输入，要求 planner 先读取 `task_status` 和
`list_agents`。attach 只对账，不触发 Turn。

## 16.5 Executor 与交付

`task_spawn_executor` 必须提供 taskName、message；scopeHints 可省略或为空。TaskService 在创建
child Thread 前只校验 hint 是规范仓库相对路径，再校验并发上限、phase 和 branch lease并准备
专属 worktree。hint 重叠只形成提示，不拒绝并发。

executor 只能写自己的 worktree，并以以下工具结束可交付工作：

```text
report_completion {
  delivery { headCommit, verificationSummary }
  | noDelivery { verificationSummary }
}
```

delivery 要求 worktree clean、HEAD 相对固定 base 推进、commit 身份一致，并记录完整
base-to-HEAD changed files；worktree 内变更不受 scopeHints 限制。成功事务创建不可变
WorkCompletion 并将 WorkUnit 置为 ReadyForReview。普通文本
结束、工具错误或预算中止不会伪造交付，WorkUnit 保持 AwaitingCompletion，可由 planner 向同一
Thread 发送明确 follow-up。

WorkUnit 在 ReadyForReview 之后以 `executorAgentId` 创建 fresh Delivery reviewer。ReviewRound
事务固定最新 Completion revision，reviewer canonical workspace 直接绑定同一 worktree，不接受
模型提供路径。findings 使 WorkUnit 进入 ChangesRequested；
planner 把具体 finding 发回原 executor Thread，新的 completion revision 重新审查。pass 后
WorkUnit 进入 Approved 或 NoDelivery。

## 16.6 Planner 自主 Git、合并记账与综合审查

Approved 且 executor 已关闭的 delivery 由 `task_status` 投影为 `MergeCandidate`，包含 executor、
completion revision、相对 worktree locator、branch、base/head commit 与 Task expected HEAD。
TaskService 不执行 merge，也不提供专用 conflict 文件工具。Planner 在 Task 主 workspace 使用普通
exec/file/Git，自行选择 merge、cherry-pick、squash、rebase 或 manual，并自行解决或 abort 冲突。
冲突期间 Task phase 仍是 Merging，不创建独立 conflict state 或持久化 conflict tool session。

Git 收束后 Planner 调用 `task_record_merge`，提交 executor、completion revision、previous/resulting
HEAD、typed method 与 summary。该工具只在 branch mutation lock 内重读并验证 caller、Approved
completion、已关闭 executor、BranchLease/Git identity、当前 HEAD、previous 是 resulting 的祖先、
clean workspace 且没有未结束的 Git operation；它不运行或补偿 Git，也不判断 patch 等价形状。
成功事务写 MergeRecord、推进 WorkUnit/TaskRun/BranchLease，并授权幂等清理源 worktree。
Git 已变化但记账失败时保留现场并 scoped block，不 reset。

所有 WorkUnit 均有 MergeRecord 或 NoDelivery、design 已与当前 HEAD 一致后，创建 fresh integrated
reviewer，其 canonical workspace 是 TaskRun 主 workspace。findings 进入 reworking，由新 executor
修复；pass 才允许 `task_complete`。

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
round 收束为失败，并按精确 completion/HEAD 恢复 WorkUnit。Merging 重启时先验证 canonical
workspace、Git common directory 和 branch；HEAD 仍等于 expectedHead、workspace clean 且没有未结束
Git operation 时保持 Merging 并进入 paused，等待用户继续 Planner。若 HEAD 已变化、workspace dirty
或仍有 merge/rebase/cherry-pick 等状态，则保留现场、将精确 Task scoped block，并提供非破坏性的
Retry/Reconcile，不自动 reset、abort 或 cleanup。Retry 重新验证 canonical Git identity，原子重建
BranchLease 和进程 lease、清除该恢复终态后回到 Merging；失败时继续保留 issue 和磁盘现场。
Task phase、delivery、review、merge record、worktree 和 lease 均从产品表恢复；没有 pending input 时
保持 paused。

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

不兼容 Studio 数据库直接删除并重建，不迁移或归档 ownership。数据库重建本身不扫描或删除磁盘
worktree/branch；失去 durable owner 的资源继续保留现场。

## 16.10 完成条件

`task_complete` 要求：

- design 与当前 HEAD 一致；
- 全部 WorkUnit 为 Merged 或 NoDelivery；
- 最新 integrated review 针对当前 HEAD 且 verdict 为 pass；
- 当前分支、workspace、TaskRun 和 BranchLease expectedHead 精确一致；
- 最终验证通过且不存在 StopRequested。

完成事务写 completed 并删除 BranchLease。任何迟到 child completion、旧 generation 或旧 Turn
通知都不能改变已提交的 Task 终态。
