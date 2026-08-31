请严格完成一次 Task 子代理验收，并在最终回复中逐项给出证据。

[[CHILD_CONTRACT:purpose]]
探索者一负责核对 task workflow 与 live artifact；探索者二负责核对 workspace/Git 生命周期。executor 创建 allowed/directory.txt，worktree_executor 创建并提交 worktree_result.txt，reviewer 只读复审。

[[CHILD_CONTRACT:baseline]]
基线是当前仓库 HEAD；Task 使用 mode.task，四个内置 Profile 的 system instructions 与 workspace mode 必须冻结在 spawn receipt 中。

[[CHILD_CONTRACT:ownership]]
root 拥有 design/subagents-orchestration.md、整合、cherry-pick 和 cleanup；executor 仅拥有 allowed，worktree_executor 仅拥有自己的 worktree，explorer/reviewer 只读。

[[CHILD_CONTRACT:forbidden]]
不得写 forbidden/denied.txt，不得绕过 writablePaths，不得由 child 整合 worktree，不得由 reviewer 修改任何文件或 Git 状态。

[[CHILD_CONTRACT:steps]]
root 先并行 spawn 两个 explorer，再并行 spawn executor 与 worktree_executor；先 wait/read 并核对目录结果，再显式 cherry-pick，随后 cleanup，最后 spawn fresh-context reviewer。executor 必须先尝试 forbidden/denied.txt，再写 allowed/directory.txt；worktree_executor 必须写 worktree_result.txt、cargo test 并提交 feat: worktree executor marker。

[[CHILD_CONTRACT:completion_failure]]
executor 必须报告 DIRECTORY_DENIAL_OBSERVED；worktree_executor 必须报告 WORKTREE_COMMIT_READY、40 位 commit hash 和 workspace root；reviewer 必须报告 REVIEWER_READ_ONLY_APPROVED。任一步失败都必须保留错误证据并停止伪造成功。

[[CHILD_CONTRACT:evidence]]
记录四个 Profile 的 profileId、forkTurns:none、workspace receipt、工具调用顺序、拒绝原因、独立分支和最终测试；最终成功标记为 PURE_SUBAGENTS_LIVE_OK。

[[CHILD_CONTRACT:workspace_git_cleanup]]
directory workspace 使用 writablePaths:["allowed"]；worktree 使用 pure-agent-* 分支和独立路径；root 只能在核对未整合状态后 git cherry-pick，并在 close_agent 时显式 workspaceDisposition:"cleanup"，确认路径和分支均已删除。
