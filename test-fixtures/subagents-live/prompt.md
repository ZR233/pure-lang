请严格完成一次 Task 子代理验收，并在最终回复中逐项给出证据。

根 Agent 必须在每一个 spawn_agent 的 message（两个 explorer、executor、worktree_executor、reviewer）中，按以下同一顺序逐字包含八个 CHILD_CONTRACT marker；每一段必须是该 child 的具体内容，不能只引用本文件。两个 explorer 的目的和 ownership 必须不同。

[[CHILD_CONTRACT:purpose]]
探索者一只读核对 task workflow 与 live artifact；探索者二只读核对 workspace/Git 生命周期。executor 在 directory assignment 中创建 allowed/directory.txt；worktree_executor 在独立 worktree 中创建并提交 worktree_result.txt；reviewer 只读复审。

[[CHILD_CONTRACT:baseline]]
基线是当前仓库 HEAD；Task 使用 mode.task，四个内置 Profile 的 system instructions 与 workspace mode 必须冻结在 spawn receipt 中；禁止复制 immutable Profile 首行到本段或 child message。

[[CHILD_CONTRACT:ownership]]
root 在 confirmation 后亲自使用内置 write_file 创建 design/subagents-orchestration.md，内容包含 ROOT_DESIGN_MARKER，并拥有整合、cherry-pick 和 cleanup；executor 仅拥有 allowed，worktree_executor 仅拥有自己的 worktree，explorer/reviewer 只读。

[[CHILD_CONTRACT:forbidden]]
executor 必须先尝试用内置 write_file 写 forbidden/denied.txt，预期被 writablePaths 拒绝，不得绕过边界；不得由 child 整合 worktree，不得由 reviewer 修改任何文件或 Git 状态。

[[CHILD_CONTRACT:steps]]
 confirmation 后 root 必须先调用 list_agent_profiles 确认 explorer、executor、worktree_executor、reviewer 四个 Profile，再亲自 write_file design/subagents-orchestration.md，再并行 spawn 两个 explorer；随后并行 spawn executor 与 worktree_executor。executor 先观察拒绝，再用内置 write_file 写 allowed/directory.txt，内容必须为 directory child accepted\n 并报告。worktree_executor 写 worktree_result.txt，内容必须为 worktree child committed\n，运行 cargo test，提交固定 subject feat: worktree executor marker 并报告 40 位 hash。root 必须在整合前证明主 workspace 隔离，再显式 cherry-pick，随后 close_agent 并使用 workspaceDisposition:cleanup，证明分支与路径均已清除，最后 spawn fresh-context reviewer。除标准 confirmation 外不得询问用户。

[[CHILD_CONTRACT:completion_failure]]
executor 必须报告 DIRECTORY_DENIAL_OBSERVED；worktree_executor 必须报告 WORKTREE_COMMIT_READY、40 位 commit hash 和 workspace root；reviewer 最终 cargo test、核对文件、marker、sentinel，并报告 REVIEWER_READ_ONLY_APPROVED。任一步失败都必须保留错误证据并停止伪造成功。

[[CHILD_CONTRACT:evidence]]
记录四个 Profile 的 profileId、forkTurns:none、workspace receipt、工具调用顺序、拒绝原因、独立分支和最终测试；最终成功标记为 PURE_SUBAGENTS_LIVE_OK。若 reviewer 输出 REVIEWER_FINDING，必须重新 spawn 不同 callId 的 implementation 与 reviewer，并提供第二次 integration 证据后才可 approval。

[[CHILD_CONTRACT:workspace_git_cleanup]]
directory workspace 使用 writablePaths:["allowed"]；worktree 使用 pure-agent-* 分支和独立路径；root 只能在核对未整合状态后 git cherry-pick，并在 close_agent 时显式 workspaceDisposition:"cleanup"，确认路径和分支均已删除。
