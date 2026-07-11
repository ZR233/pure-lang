# 15. Subagent Worktree 隔离执行

## 动机

当前 subagent（`AgentSupervisor` 管理的 child agent）与父 agent 共享同一个
`workspace_root`，所有 file / git / lsp 工具在同一目录操作，仅靠进程内写锁做软隔离。
这带来三个问题：

- 多个 subagent 并发改同一份文件会互相覆盖，没有真正的物理隔离。
- subagent 的工作产物直接混入主工作区，无法原子性地「采纳或丢弃」。
- 没有「交付 → 合并」的结构化边界，父 agent 难以审查 subagent 的修改后再决定是否接受。

本设计为每个 subagent 分配独立的 git worktree，使其修改物理隔离。Task executor
必须显式提交 delivery；planner 消费结果后再选择 merge 或 discard。worktree 在交付
被合并、丢弃或任务终结后释放，不再与单次 agent turn 终态绑定。

## 与既有约定的关系

本设计是 `01-overview.md` 与 `06-phaces.md` 中「未来沙箱」方向的落地，但在文件系统
层面，因此需要正面处理两条既有约定：

- `03-pipeline.md`：child turn「复用同一套工具边界」。本设计把 subagent 的工具边界
  改为 **agent-scoped `workspace_root`**——同一套工具，不同 `workspace_root` 实参。
  实现上只替换 `AgentRunSpec.workspace_root`，单个工具无需改动。
- `05-extension.md`：进程内 workspace 写锁共享。写锁以规范化后的 `workspace_root`
  路径为键，因此每个 subagent 独有的 worktree 路径会自动获得独立写锁，锁语义无需
  调整，sibling subagent 之间不竞争。

merge 在既有文档中零提及（`merge` 一词此前全部指 snapshot / config / UI 合并）。
本设计引入的 git merge 是净新增能力。

## 架构

新增 `pl-core::agent::worktree` 模块，按端口-适配器组织：

- `WorktreeBackend`（端口，RPITIT + `Send`，遵循仓库禁止 `async_trait` 的约定）：
  封装 `git worktree add/remove`、兜底 `git commit`、`git merge` 的底层执行。
- `LocalWorktreeBackend`（默认实现）：复用 `tool::git::LocalExecutionBackend` shell
  out `git`，复用 `GitPolicy::validate_branch` 校验分支名，复用
  `git_shell_command` 的 `core.hooksPath=/dev/null` / `safe.directory` 安全注入。
  **不引入 `git2` / `tempfile` 依赖**，与仓库现有 git 工具风格一致。
- `WorktreeManager`：持有 `Arc<dyn WorktreeBackend>` 与 repo_root，负责路径分配、
  创建 / 提交 / 合并 / 释放编排；独立的 typed reconciler 在 Studio 启动恢复阶段根据
  durable owner inventory 对账孤儿 worktree。

`AgentSupervisor` 持有 `Arc<WorktreeManager>`。默认 `WorktreeManager::disabled()` 为
no-op，保持既有「subagent 共享 `workspace_root`」行为与全部既有测试不变；显式
`enable_worktrees(repo_root)` 后才为 subagent 分配 worktree。enable 只幂等绑定主
`workspace_root` 解析出的 repo_root，不扫描或清理磁盘；孤儿对账只属于 Studio 启动恢复。

## 关键类型（接口契约）

- `WorktreeHandle { path: PathBuf, branch: String }`：存入 `AgentEntry`，随 agent
  条目同生共死；root agent 为 `None`。
- `WorktreeRef { path: String, branch: String }`：worktree 的模型可见出口。默认
  工具路径通过 `AgentHandle.worktree`（`spawn_agent` 返回）与 `SpawnAgentResult`
  暴露给调用方；`close_agent` 的 `merge` 入参（`CloseAgentArgs`）选择 disposition。
  `AgentControlBackend` 共享类型（宿主扩展路径）不携带 worktree，避免破坏性对外
  API 变更，宿主可经 `AgentSupervisor` 自行接入。
- `CloseDisposition::Discard` 只负责放弃未采纳产物；Task merge 由 coordinator 的
  `task_merge_agent` 负责，不通过 `close_agent` 隐式合并。
- `MergeOutcome { Merged, Conflict }`：merge 结果，`Conflict` 时不释放 worktree。
- `WorktreeError`：`manager` 内部错误类型，向 `PureError::ToolExecutionFailed`
  `{ tool: "worktree", error }` 映射，不跨 crate 新增枚举变体。

## 生命周期状态机

```
spawn -> running -> waitingForDelivery -> delivered -> planner merge -> released
                                   \-> discard / task terminal -> released

released = git worktree remove + 删除分支 + 清空 AgentEntry.worktree
```

要点：

- worktree 生命周期 = agent 生命周期。`close_agent` 是唯一释放点，且必须带
  `CloseDisposition`。单次 turn 完成不释放 worktree（agent 可经 `send_input`
  多轮），与既有「turn 完成 ≠ agent 释放」语义一致。
- runtime 不兜底 `git add -A` 或 commit。executor 必须自行提交并用
  `submit_delivery` 交付干净 worktree；planner 通过 task coordinator 合并。
- `close(Discard)` 或级联关闭：`git worktree remove --force` + 删除 subagent 分支。
- spawn 失败回滚（包括 worktree 创建部分成功、持久化激活失败或
  `start_agent_turn` 失败）必须同步尝试移除 worktree、删除分支并撤销宿主生命周期
  事实。主错误与所有回滚失败必须一并返回，不能把失败的清理报告成成功。
- `WorktreeBackend::create` 必须用结构化 disposition 声明失败是否可能已创建本次 spec
  的资源。参数校验、进程启动前 IO 失败和明确的 Git 非零退出不得 cleanup；只有超时、
  启动后状态不确定或 backend 明确报告 `MayHaveCreated` 时，manager 才能按本次 spec
  补偿清理，避免删除调用前已存在的 branch 或 worktree。

## 路径与命名约定

- worktree 根：`resolve_workspace_root`（`workspace.rs`）所得 repo 根下
  `.pure/worktrees/`。注意区分语义：用户级 `~/.pure`（`config/mod.rs`）是配置；
  项目级 `<repo_root>/.pure/` 是运行态产物。
- 命名：`<repo_root>/.pure/worktrees/<task_run_id>/<agent_id>/`。
- 分支：`pure-task-<task_run_id>-<agent_id>`，经 `GitPolicy::validate_branch` 校验。
- **`.gitignore` 必须忽略 `.pure/`**，否则 worktree 会污染主仓库索引；启用时检测并提示。

## 启用时机

孤儿 GC 只在 Studio 启动恢复阶段运行，并以持久化 `TaskRun`、`WorkUnit`、
`AgentOutcome` 为唯一所有权来源。普通 root turn、continuation turn、会话选择切换和
`enable_worktrees` 都不得扫描或删除其他 session 的 worktree。

启动对账必须逐个 leaf registration/path/branch 精确处理，禁止递归删除
`.pure/worktrees/<taskRunId>` 父目录：

- active、blocked、因重启收束为 cancelled、delivered 的资源继续保护；merged 但尚未
  清理的资源进入 cleanup-pending 重试。
- 只有没有 durable owner，或 durable owner 已终态且明确可清理的 leaf 才允许删除。
- durable 记录声明资源存在而 registration、path、branch 部分缺失时，关联 run 进入
  blocked，保留现场；无 owner 的清理失败使初始化显式失败，均不得吞错。
- `Pending/Queued` allocation 事务可能先于 worktree create 落盘；重启时仅这一 typed
  creation state 允许 registration、path、branch 三者全部不存在。三者全部存在仍保护，
  任意部分存在仍 block。`Running`、`WaitingForDelivery`、`Delivered` 不允许 all-absent。
- 对账幂等；重复启动不得误删已保护资源，也不得重新报告已经观察过的 terminal 事件。
- 启动按全部已知 Task workspace 去重对账，而不是依附某个 active run；只有 blocked、
  terminal 或 cleanup-pending 记录时也必须执行。active run 只有在所属 workspace 对账成功
  后才能进入 Recovery continuation。
- inventory、registration remove、leaf remove 每一步都重新拒绝 symlink、Windows junction
  与 reparse ancestor，并证明 canonical leaf 严格位于 canonical `.pure/worktrees` root。
  `git worktree remove` 返回后、任何 fallback 文件系统删除之前必须再次证明，覆盖 Git
  调用期间祖先被替换为链接的竞态。
  Git 子进程禁用交互、设置有界超时并在超时后终止。

subagent turn（`active_subagent.is_some()`）不再 enable；其 `workspace_root` 已被
替换为自身 worktree 路径。

## 隔离

`runner.rs` 注册 subagent 工具时，`workspace_root` = 其 worktree 路径。
`register_tools(..., workspace_root, ...)` 是唯一入口，所有工具（git 的
`GitWorkspaceConfig.worktree`、file 工具、lsp root）自动隔离到 worktree，无需改动
单个工具。进程内写锁以 `workspace_root` 路径为键，每个 worktree 独立锁。

## 资源释放策略

遵循仓库现有风格（显式 async 清理为主，Drop 同步 best-effort 为辅）：

- 主路径：`close_agent` / 明确 discard / spawn 失败回滚里 async 释放。Task runtime
  shutdown 不调用 `shutdown_descendants`，而是 cancel-and-wait/quiesce agent task 并
  保留 durable worktree、session 与 entry，供重启对账和审计。
- spawn 失败回滚必须尝试全部独立步骤并聚合错误；单个 `git worktree remove` 失败
  不得阻止删除分支或宿主 rollback hook。
- 兜底：进程异常退出留下的孤儿 worktree 由下次启动 GC 清理（不依赖 `Drop` await）。

通用 `shutdown_descendants` 级联关闭后代时仍默认走 `Discard`，但它不属于 Task
runtime shutdown 路径；Task shutdown 的清理错误必须显式返回，不能吞掉后继续销毁资源。

## crate 边界

`WorktreeBackend` 端口与默认本地实现都位于 `pl-core::agent::worktree`，因为 worktree
是 subagent 执行环境的基础设施，而非业务端口。`AgentSupervisor` 只持有
`WorktreeManager`，不直接执行 git 命令。宿主若需要自定义 worktree 后端（如容器内
git），可注入实现 `WorktreeBackend` 的类型。
