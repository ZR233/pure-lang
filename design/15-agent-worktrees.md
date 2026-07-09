# 15. Subagent Worktree 隔离执行

## 动机

当前 subagent（`AgentSupervisor` 管理的 child agent）与父 agent 共享同一个
`workspace_root`，所有 file / git / lsp 工具在同一目录操作，仅靠进程内写锁做软隔离。
这带来三个问题：

- 多个 subagent 并发改同一份文件会互相覆盖，没有真正的物理隔离。
- subagent 的工作产物直接混入主工作区，无法原子性地「采纳或丢弃」。
- 没有「交付 → 合并」的结构化边界，父 agent 难以审查 subagent 的修改后再决定是否接受。

本设计为每个 subagent 分配独立的 git worktree，使其修改物理隔离；subagent 关闭时
由调用方选择把产物 `merge` 回主工作区或 `discard` 丢弃，worktree 随 subagent 释放。
worktree 生命周期严格绑定到 subagent 生命周期。

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
  创建 / 提交 / 合并 / 释放编排，以及孤儿 worktree 的启动 GC。

`AgentSupervisor` 持有 `Arc<WorktreeManager>`。默认 `WorktreeManager::disabled()` 为
no-op，保持既有「subagent 共享 `workspace_root`」行为与全部既有测试不变；显式
`enable_worktrees(repo_root)` 后才为 subagent 分配 worktree。enable 在 root turn
启动时基于主 `workspace_root` 解析出的 repo_root 幂等完成（见「启用时机」）。

## 关键类型（接口契约）

- `WorktreeHandle { path: PathBuf, branch: String }`：存入 `AgentEntry`，随 agent
  条目同生共死；root agent 为 `None`。
- `WorktreeRef { path: String, branch: String }`：worktree 的模型可见出口。默认
  工具路径通过 `AgentHandle.worktree`（`spawn_agent` 返回）与 `SpawnAgentResult`
  暴露给调用方；`close_agent` 的 `merge` 入参（`CloseAgentArgs`）选择 disposition。
  `AgentControlBackend` 共享类型（宿主扩展路径）不携带 worktree，避免破坏性对外
  API 变更，宿主可经 `AgentSupervisor` 自行接入。
- `CloseDisposition`：
  - `Merge { target_branch: Option<String> }`：把 subagent 分支 merge 回主工作区
    当前分支（或指定目标），成功后释放 worktree。
  - `Discard`：放弃修改，直接释放 worktree。
- `MergeOutcome { Merged, Conflict }`：merge 结果，`Conflict` 时不释放 worktree。
- `WorktreeError`：`manager` 内部错误类型，向 `PureError::ToolExecutionFailed`
  `{ tool: "worktree", error }` 映射，不跨 crate 新增枚举变体。

## 生命周期状态机

```
spawn ──► running ──┬── close(Merge)   ──► commit兜底 ──► merge ──► released
                    │                                     └─ Conflict ──► 保留 worktree，返回错误
                    └── close(Discard) ───────────────────────────────────► released

released = git worktree remove + 删除分支 + 清空 AgentEntry.worktree
```

要点：

- worktree 生命周期 = agent 生命周期。`close_agent` 是唯一释放点，且必须带
  `CloseDisposition`。单次 turn 完成不释放 worktree（agent 可经 `send_input`
  多轮），与既有「turn 完成 ≠ agent 释放」语义一致。
- `close(Merge)`：系统先在 worktree 内兜底提交未提交改动（若 subagent 未自行
  commit），再在主工作区执行 `git merge <branch>`。冲突时不释放 worktree、返回
  `MergeConflict`，调用方可调整后重试或改用 `Discard`。
- `close(Discard)` 或级联关闭：`git worktree remove --force` + 删除 subagent 分支。
- spawn 失败回滚（`start_agent_turn` 失败）必须同步释放已分配的 worktree。

## 路径与命名约定

- worktree 根：`resolve_workspace_root`（`workspace.rs`）所得 repo 根下
  `.pure/worktrees/`。注意区分语义：用户级 `~/.pure`（`config/mod.rs`）是配置；
  项目级 `<repo_root>/.pure/` 是运行态产物。
- 命名：`<repo_root>/.pure/worktrees/<agent_id>/`。`agent_id` 由
  `AgentSupervisorState::next_id` 生成（`agent-1`、`agent-2`…），天然唯一不重名。
- 分支：`pure-agent-<agent_id>`，经 `GitPolicy::validate_branch` 校验。
- **`.gitignore` 必须忽略 `.pure/`**，否则 worktree 会污染主仓库索引；启用时检测并提示。

## 启用时机

`run_turn_with_trace`（root turn，`active_subagent.is_none()`）启动时，用主
`workspace_root` 经 `resolve_workspace_root` 解析 repo_root，幂等调用
`AgentSupervisor::enable_worktrees(repo_root)`。enable 内部跑一次启动 GC，扫描
`.pure/worktrees/` 删除注册表中不存在的残留目录（处理上次进程异常退出留下的孤儿）。

subagent turn（`active_subagent.is_some()`）不再 enable；其 `workspace_root` 已被
替换为自身 worktree 路径。

## 隔离

`runner.rs` 注册 subagent 工具时，`workspace_root` = 其 worktree 路径。
`register_tools(..., workspace_root, ...)` 是唯一入口，所有工具（git 的
`GitWorkspaceConfig.worktree`、file 工具、lsp root）自动隔离到 worktree，无需改动
单个工具。进程内写锁以 `workspace_root` 路径为键，每个 worktree 独立锁。

## 资源释放策略

遵循仓库现有风格（显式 async 清理为主，Drop 同步 best-effort 为辅）：

- 主路径：`close_agent` / `shutdown_descendants` / spawn 失败回滚里 async 释放。
- 兜底：进程异常退出留下的孤儿 worktree 由下次启动 GC 清理（不依赖 `Drop` await）。

`shutdown_descendants` 级联关闭后代时，后代默认走 `Discard`（不应自动 merge 未
验收的子树产物）。

## crate 边界

`WorktreeBackend` 端口与默认本地实现都位于 `pl-core::agent::worktree`，因为 worktree
是 subagent 执行环境的基础设施，而非业务端口。`AgentSupervisor` 只持有
`WorktreeManager`，不直接执行 git 命令。宿主若需要自定义 worktree 后端（如容器内
git），可注入实现 `WorktreeBackend` 的类型。
