# 16. Simple / Task 模式与任务编排

## 模式语义

Studio 会话模式固定为 `simple | task`。新会话默认 `simple`；数据库中旧
`auto | plan` 会话保留但不进入列表、直接读取或恢复流程。

- `simple`：根 turn 使用 executor 角色，直接对话和实施；只能创建只读 explorer。
- `task`：根 turn 始终使用 planner 角色。planner 是唯一协调者，负责澄清意图、
  提交计划、更新设计、发起代理、消费结果、掌管当前分支、解决 merge 冲突、启动
  reviewer 和完成汇报。

explorer、executor、reviewer 的 agent depth 固定为 1，不得派生后代。它们只能由
planner 直接创建，或由 planner 调用 harness 后间接创建；所有终态结果必须回流
planner。

## 执行边界

核心层使用 `TurnExecutionProfile` 与工具 effect 强制角色边界。effect 分为
`Read`、`WorkspaceWrite`、`Process`、`AgentControl`、`BranchControl` 和
`ConflictWrite`；未知 effect 对 planner、explorer、reviewer 默认拒绝。

- planner 平时只允许读取、交互、agent control、任务 harness 和受限的
  `task_update_design`。
- executor 只写自己的 worktree，并用 `submit_delivery` 显式提交交付。
- reviewer 只读 plan、diff、代码和按需定位的 design 文档，通过 `review_exit`
  返回结构化审查结果。
- planner 在 `resolvingConflict` 阶段临时获得 `ConflictWrite`，且只能修改当前
  `MergeRecord` 列出的冲突文件。

## 持久化 coordinator

任务事实通过 SQLite 持久化为 `TaskRun`、`WorkUnit`、`AgentOutcome`、
`MergeRecord`、`ReviewRound` 和 `BranchLease`。生命周期为：

```text
planning -> pendingConfirmation -> designUpdating -> implementing -> merging
         -> resolvingConflict -> reviewing -> reworking
         -> completed | blocked | failed | cancelled
```

coordinator 后台监控代理。代理终结、merge 冲突或 reviewer 返回后，runtime 写入事实
并启动 planner continuation turn；planner 不依靠单个长 turn 持续 wait。应用重启后
从持久事实恢复，Git 状态与 `expectedHead` 不一致时进入 `blocked`。

同一 Git common directory 与分支只允许一个写入任务。`BranchLease` 是进程内所有权，
`expectedHead` CAS 和工作区清洁检查负责检测用户或外部进程的变化。

通用 agent supervisor 通过生命周期 hook 与精确 worktree spec 接入 Task 编排，
不依赖 Studio store 或 coordinator 类型。Task coordinator 在 agent id 分配后先以事务
创建 Pending `WorkUnit` 与 `AgentOutcome`，并返回固定的 repository、path、branch
和 base commit；supervisor 按该 spec 创建 worktree，coordinator 再以事务把两条记录
激活为 Running，之后才允许子 turn 启动。只有 `implementing | reworking` 阶段允许
分配 executor。并行 allocation 在进程内串行化检查 attempt、并发数和 ownedPaths 后再
事务写入；prepare 事务未提交时不留记录，随后 worktree 创建、持久化激活或 turn
启动失败时删除 agent registry entry、worktree 与分支，并将 WorkUnit/Outcome
事务性标记为 Failed，由 Outcome 保存错误供 planner 与重启恢复审计。

## Executor 交付

executor 必须先自行 commit，并调用：

```text
submit_delivery { headCommit, verificationSummary }
```

runtime 只接受 HEAD 已推进且 worktree 干净的交付，返回 `baseCommit`、`headCommit`、
`changedFiles`、验证摘要和 `{ path, branch }`。runtime 不隐式执行 `git add -A`。
work unit 声明 `ownedPaths`；并行 executor 的写入范围不得重叠，超出范围的交付必须
返回 planner 决策。work unit 创建时固定记录 `baseCommit`、预期 worktree path 和
branch；交付校验不得改用随后可能因其他 executor 合并而推进的 task `expectedHead`，
且 caller workspace 和 branch 必须与固定记录精确匹配。rename/copy 同时校验 source
与 destination，delete 校验被删除的原路径。单个 work unit 最多尝试三次，同时运行
的 executor 最多四个。

Task executor 的通用 `close_agent merge=true` 必须在进入 worktree merge 路径前拒绝；
关闭或取消只允许丢弃 worktree。Task worktree 不得调用通用的隐式 `commit_all`。

worktree 路径和分支包含 task run id：

```text
.pure/worktrees/<taskRunId>/<agentId>
pure-task-<runId>-<agentId>
```

## Planner 合并与冲突

planner 调用 `task_merge_agent { agentId, expectedHeadCommit }`。runtime 校验当前分支
和交付 commit 后执行 `git merge --no-ff --no-commit`。无冲突时先运行相关集成检查，
通过后才创建 merge commit。成功后更新 `expectedHead`、关闭 agent 并释放 worktree；
executor 完成一个即可合并。

冲突时持久化 `MergeRecord`，暂停其他 merge，由 planner 使用以下受限工具亲自解决：

- `merge_list_conflicts`
- `merge_read_conflict`
- `merge_resolve_file`
- `merge_verify`
- `merge_continue`
- `merge_abort`

planner 只能修改冲突清单中的文件。continue 前必须没有 unmerged index、冲突标记或
未解释修改，并通过格式化与相关测试。三次解决仍失败时必须 abort 并进入 blocked。

## 设计与审查门禁

用户确认实施后，planner 先调用 `task_update_design` 修改并提交 `design/**`；成功前
不得创建 executor。任务取消或部分失败时，design 必须回退或更新到与当前实现一致。

当前编码轮全部交付合并后，planner 调用 `task_request_review` 间接创建只读 reviewer。
reviewer 初始上下文包含 plan、任务 diff、代理结果、验证摘要和 design 文件索引，不
预载 design 正文。系统提示词要求 reviewer 根据改动主动搜索并读取相关 design，再
对照设计审查正确性、回归、安全和测试缺口。

`review_exit` 返回 `verdict`、`summary`、`designReferences` 和 `findings`。runtime 根据
tool trace 校验引用位于 `design/**` 且确实被 reviewer 读取。`changesRequired` 返回
planner 派发修复 executor；修复合并后必须创建新的 reviewer。最多三轮审查修复。

只有 design 一致、所有交付已处理、当前分支干净、最新 reviewer 对当前 HEAD 返回
`pass` 且验证通过时，planner 才能调用 `task_complete`。
