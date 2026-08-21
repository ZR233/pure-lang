# 15. Subagent Worktree 隔离执行

## 动机

未启用 Studio worktree lifecycle 时，subagent（`AgentRuntime` 管理的 child AgentLoop）与父 agent 共享同一个
`workspace_root`，所有 file / git / lsp 工具在同一目录操作，仅靠进程内写锁做软隔离。
这带来三个问题：

- 多个 subagent 并发改同一份文件会互相覆盖，没有真正的物理隔离。
- subagent 的工作产物直接混入主工作区，无法原子性地「采纳或丢弃」。
- 没有「交付 → 合并」的结构化边界，父 agent 难以审查 subagent 的修改后再决定是否接受。

本设计为每个 subagent 分配独立的 git worktree，使其修改物理隔离。Task executor
必须通过 `report_completion` 创建不可变 completion；delivery reviewer 通过后，planner
显式关闭 executor，使用普通 Git 整合并调用 `task_record_merge` 记账。worktree 在交付被记录、
明确丢弃或任务终结后释放，不再与
单次 agent turn 终态绑定。

Task executor 只能由 `task_spawn_executor` 创建。Task planner 必须先调用
`task_finalize_design` 进入 `Implementing`；有设计阶段 workspace 变化时，finalize 创建的提交会推进
Task expected HEAD，因此 executor worktree 从该 HEAD 创建并自然继承全部文档、代码和其他修改。
`scopeHints` 是可选的仓库相对关注路径，只帮助 Planner 拆分任务、review 聚焦和提示已知冲突；
它不是文件授权、并发互斥或 completion 门禁。executor 可以修改自身 worktree 内任意仓库文件，
真正不变量是 canonical worktree、Git identity、clean delivery 与完整 base-to-HEAD diff。

## 与既有约定的关系

本设计是 `01-overview.md` 与 `06-phaces.md` 中「未来沙箱」方向的落地，但在文件系统
层面，因此需要正面处理两条既有约定：

- `03-pipeline.md`：child turn「复用同一套工具边界」。本设计把 subagent 的工具边界
  改为 **agent-scoped `workspace_root`**——同一套工具，不同 `workspace_root` 实参。
  实现上由 Studio turn factory 使用 durable WorkUnit/ReviewRound/Completion/TaskRun owner
  解析本轮 `AgentWorkspace`，单个工具无需理解 Studio Task 类型。
- `05-extension.md`：进程内 workspace 写锁共享。写锁以规范化后的 `workspace_root`
  路径为键，因此每个 subagent 独有的 worktree 路径会自动获得独立写锁，锁语义无需
  调整，sibling subagent 之间不竞争。

merge 在既有文档中零提及（`merge` 一词此前全部指 snapshot / config / UI 合并）。
本设计引入的 git merge 是净新增能力。

## 架构

Studio 产品层的 `pl-studio-runtime::agent::worktree` 模块按端口-适配器组织：

- `WorktreeBackend`（端口，RPITIT + `Send`，遵循仓库禁止 `async_trait` 的约定）：
  只封装 `git worktree add/remove` 与 branch cleanup；TaskService 不实现 Git merge。
- `LocalWorktreeBackend`（默认实现）：复用 `tool::git::LocalExecutionBackend` shell
  out `git`，复用 `GitPolicy::validate_branch` 校验分支名，复用
  `git_shell_command` 的 `core.hooksPath=/dev/null` / `safe.directory` 安全注入。
  **不引入 `git2` / `tempfile` 依赖**，与仓库现有 git 工具风格一致。
- `WorktreeManager`：持有 `Arc<dyn WorktreeBackend>` 与 repo_root，负责路径分配、
  创建和释放编排；提交由 executor 使用普通 Git 完成，整合由 Planner 自主完成。独立的 typed reconciler 在 Studio 启动恢复阶段根据
  durable owner inventory 对账孤儿 worktree。

`StudioHost` 的 lifecycle adapter 在 spawn prepare 阶段创建可回滚的临时 worktree handle，
在 durable WorkUnit owner 落盘后 activate，并在 rollback/close 阶段完成补偿或授权清理。
该 handle 不是 workspace 的事实源。`WorktreeManager::disabled()` 为 no-op；
Studio Task policy 显式提供 repo root 时才为 subagent 分配 worktree。创建过程只绑定主
`workspace_root` 解析出的 repo root，不扫描或清理磁盘；孤儿对账只属于 Studio 启动恢复。

每个 durable `WorkUnit` 还保存 typed `worktreeDisposition = protect |
cleanupRequested`。新记录默认 `protect`；v10 不导入旧记录或运行 backfill。普通 Cancelled、
Failed、blocked 或无证据的 terminal 记录继续保护，不能把状态枚举本身当成删除授权。
Task executor discard 必须在释放物理资源之前事务性持久化 `cleanupRequested`；即使
WorkUnit 已经 Failed/Cancelled，也必须幂等记录这次明确授权。

## 关键类型（接口契约）

- `WorktreeHandle { path: PathBuf, branch: String }`：存入 Studio lifecycle resource lease，
  随 agent 产品资源同生共死；root agent 为 `None`。
- `AgentWorkspace { root, boundary, mutability }`：`pl-core` 的通用 turn/tool 边界，不携带
  Studio Task 类型。Studio 用 durable owner 解析实例；Task child 一律 `confined`。
- `WorktreeRef { relativePath, branch, baseCommit, headCommit }`：只向 Planner 暴露相对
  locator 与 Git identity；reviewer 不需要物理 locator，因为其 canonical root 已是目标 worktree。
- `CloseDisposition::Discard` 只负责放弃未采纳产物；Planner 使用普通 Git 自主整合，随后由
  `task_record_merge` 记录结果。`close_agent` 不隐式合并。
- `WorktreeError`：`manager` 内部错误类型，向 `PureError::ToolExecutionFailed`
  `{ tool: "worktree", error }` 映射，不跨 crate 新增枚举变体。

## 生命周期状态机

```
spawn -> running -> awaitingCompletion -> readyForReview
                    ^                    |
                    |                    v
                    +---- changesRequested
                                         |
                                         v
                                   approved -> close(preserve) -> planner Git -> record -> released
                                         \-> noDelivery -> close(discard) -> released

released = git worktree remove + 删除分支 + 清空 durable lifecycle resource
```

要点：

- 单次 turn 完成不释放 worktree（agent 可经 `send_message` 多轮），与既有
  「turn 完成 ≠ agent 释放」语义一致。approved executor 关闭后进入
  `PreserveForMerge`，由成功 merge record 清理；noDelivery、明确 discard 与已记录资源才允许
  走 cleanup。
- runtime 不兜底 `git add -A` 或 commit。executor 必须自行提交并用
  `report_completion` 报告干净 worktree 与验证摘要；delivery reviewer 通过后 planner
  才能关闭 executor、执行普通 Git，并用 `task_record_merge` 记账。
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
- Windows 上持久化的 repository、Git common directory 与 worktree 路径统一使用 native
  non-verbatim absolute representation；canonical 安全校验可以临时产生 extended path，但跨
  Task/工具/子进程边界前必须移除 `\\?\` / `\\?\UNC\` 前缀，避免同一目录因两种表示导致
  `strip_prefix`、Git 或依赖 cwd 的生成工具失配。

## 启用时机

孤儿 GC 只在 Studio 启动恢复阶段运行，并以持久化 `TaskRun`、`WorkUnit`、
`ReviewRound` 与 `BranchLease` 为唯一所有权来源。普通 root Turn、后续 Turn、Thread
选择切换和 `enable_worktrees` 都不得扫描或删除其他 Thread 的 worktree。

启动对账必须逐个 leaf registration/path/branch 精确处理，禁止递归删除
`.pure/worktrees/<taskRunId>` 父目录：

- active、blocked、因重启收束为 cancelled、awaitingCompletion、readyForReview、
  reviewing、changesRequested、approved 以及 disposition=`protect` 的资源继续保护；
  只有 disposition=`cleanupRequested` 的 leaf 才进入 cleanup-pending 重试。
- 没有 durable owner 的 leaf 只能在完整 ownership snapshot 已建立后按孤儿策略处理；
  durable owner 已终态但未明确授权时仍禁止删除。
- durable 记录声明资源存在而 registration、path、branch 部分缺失时，关联 run 进入
  blocked，保留现场；无法归属的 orphan 清理失败形成应用降级 issue，均不得吞错或击穿
  Runtime Ready。
- `Pending/Queued` allocation 事务可能先于 worktree create 落盘；重启时仅这一 typed
  creation state 允许 registration、path、branch 三者全部不存在。三者全部存在仍保护，
  任意部分存在仍 block。`Running`、`AwaitingCompletion`、`ReadyForReview`、
  `Reviewing`、`ChangesRequested`、`Approved` 不允许 all-absent。
- 对账幂等；重复启动不得误删已保护资源，也不得重新报告已经观察过的 terminal 事件。
- 启动先读取完整 durable ownership snapshot，再解析全部已知 Task workspace 的 canonical
  Git common directory，并按 common
  directory 聚合 durable owner；同组只读取一次 Git inventory，并同时覆盖组内所有
  canonical `.pure/worktrees` root。只有 blocked、terminal 或 cleanup-pending 记录时也必须
  执行；active run 只有在所属 common-directory group 对账成功后才能进入 Recovery
  continuation。任何已知 workspace（包括仅有 terminal owner 的 workspace）无法完成 Git
  identity 预检时，该 common-directory group 不执行 GC、不恢复 agent，并产生对应
  项目/Thread issue；其他已完整识别 owner 且通过预检的安全 group 可以继续。SQLite、schema
  或完整 ownership snapshot 本身无法读取时才是应用致命错误。
- inventory、registration remove、leaf remove 每一步都重新拒绝 symlink、Windows junction
  与 reparse ancestor，并证明 canonical leaf 严格位于 canonical `.pure/worktrees` root。
  `git worktree remove` 返回后、任何 fallback 文件系统删除之前必须再次证明，覆盖 Git
  调用期间祖先被替换为链接的竞态。
  上述检查与文件工具共享 `pl-core::path_safety`；fallback 递归删除不跟随 worktree
  子树里的链接，只解除链接入口并保留目标。
  Git 子进程禁用交互、设置有界超时并在超时后终止。

用户确认的恢复清理使用精确 leaf API，不复用会扫描其他 orphan 的全局 GC。预览逐个检查
registration、path、branch，报告 missing/partial/complete、dirty、相对 durable base 的
ahead commit 与 changed-file 数，并对规范化事实计算 `expectedRevision`。执行前必须重新
检查并比较 revision；stale revision、非规范 `.pure/worktrees/<taskRunId>/<agentId>` leaf、
非 `pure-task-*` 分支、symlink、junction 或 reparse ancestor 均拒绝，且不得产生部分删除。
清理绝不递归删除 `.pure/worktrees` 或 task-run 父目录，也不触碰主分支、用户仓库目录或
不属于 Pure 的分支。

确认操作先以事务终结故障 Task、删除 lease，并把相关资源标记为 `cleanupRequested`，再逐
leaf 释放物理资源。进程在 durable 授权与物理清理之间退出时，下次恢复必须幂等续清理。
完整存在、全部缺失和部分缺失资源都使用同一精确状态机：已缺失组件视为已完成，仍存在的
安全组件继续处理；任何安全证明失败都保留 issue 与现场。

subagent turn（`active_subagent.is_some()`）不再 enable；其 `workspace_root` 已被
替换为自身 worktree 路径。

## 隔离

TurnFactory 从 durable owner 解析 typed Agent workspace：executor 使用 WorkUnit worktree，
Delivery reviewer 使用该 executor 最新 Completion 的同一 worktree，Integrated reviewer 使用
TaskRun 主 workspace，root/explorer 使用 Project workspace。工具注册只消费该 workspace；file、
apply_patch、exec cwd、Git、project skills 和 LSP 都从同一 root 构造。LSP runtime 按 canonical
root 池化，同一 delivery reviewer 与 executor 复用实例，不同 worktree 不互相关闭或替换。
进程内写锁仍以 canonical root 为键。

`StudioAgentResources` 只保存 worktree handle、cleanup takeover 与进程 lease。恢复后即使该表为空，
TurnFactory 仍必须从 WorkUnit/ReviewRound/Completion/TaskRun 解析 workspace；Task child 无法解析时
形成 scoped issue，禁止回退 Project root。共享 MCP lease 是显式例外，不声明 per-agent root；
Task child 的仓库文件访问必须使用内置工具。

## 资源释放策略

遵循仓库现有风格（显式 async 清理为主，Drop 同步 best-effort 为辅）：

- 主路径：`close_agent` / 明确 discard / spawn 失败回滚里 async 释放。Task runtime
  shutdown 不调用 `shutdown_descendants`，而是 cancel-and-wait/quiesce agent task 并
  保留 durable worktree、Thread 与 directory entry，供重启对账和审计。
- spawn 失败回滚必须尝试全部独立步骤并聚合错误；单个 `git worktree remove` 失败
  不得阻止删除分支或宿主 rollback hook。
- `git worktree remove` 的非零退出可能已经移除 registration，只在删除物理 leaf 时失败。
  manager 必须继续执行经过 path-safety 复核的精确 leaf fallback 与分支删除；若最终 leaf 已缺失且
  分支删除成功，则以最终资源不变量为准报告 cleanup 成功。只有最终仍有 leaf/registration/branch
  或任一步安全证明失败时，才聚合原始 Git 错误与后续失败并报告 cleanup failed。
- 兜底：进程异常退出留下的孤儿 worktree 由下次启动 GC 清理（不依赖 `Drop` await）。

通用 `shutdown_descendants` 级联关闭后代时仍默认走 `Discard`，但它不属于 Task
runtime shutdown 路径；Task shutdown 的清理错误必须显式返回，不能吞掉后继续销毁资源。

## crate 边界

`WorktreeBackend` 端口与默认本地实现都位于 `pl-studio-runtime::agent::worktree`，因为
worktree 是 Studio Task 的产品资源，不属于通用 agent 框架。`StudioHost` lifecycle
只编排 `WorktreeManager`，不直接执行 git 命令。其他宿主若需要容器或远端 workspace，
应在自己的 `AgentLifecycleAdapter` 中定义资源 lease，不向 `pl-core` 注入 Studio 类型。
