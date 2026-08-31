请完成一次严格的子代理真实验收。不要询问用户，不要跳步，不要由 root 直接创建验收文件。

1. 先调用 `list_agent_profiles`，确认 `executor` 与 `worktree_executor` 均已启用。
2. 调用 `spawn_agent` 创建 `executor`，参数必须包含 `profileId: "executor"`、`forkTurns: "none"`、`writablePaths: ["allowed"]`。消息要求它严格按顺序：
   - 必须先调用 Pure 内置 `write_file` 尝试创建 `forbidden/denied.txt`，内容为 `must be rejected\n`，并观察该调用因 writablePaths 被拒绝；不得用 shell、Git 或 MCP 绕过。
   - 拒绝后调用 Pure 内置 `write_file` 创建 `allowed/directory.txt`，内容为 `directory child accepted\n`。
   - 读取允许文件，调用 `report_progress` 报告 `readyForCompletion`，detail 明确包含 `DIRECTORY_DENIAL_OBSERVED` 和允许文件路径，然后结束。
3. 调用 `spawn_agent` 创建 `worktree_executor`，不得传 `writablePaths`，消息要求它：
   - 调用 Pure 内置 `write_file` 创建仓库根的 `worktree_result.txt`，内容为 `worktree child committed\n`。
   - 在自己的 workspace 运行 `cargo test`。
   - 使用普通 Git 提交，commit subject 必须为 `feat: worktree executor marker`，随后取得 commit hash。
   - 调用 `report_progress` 报告 `readyForCompletion`，detail 明确包含 `WORKTREE_COMMIT_READY`、commit hash 和 workspace root，然后结束。
4. 等待两个 child 完成，读取并核对它们的 submissions。必须在显式整合前用普通 shell/Git 证明主 workspace 不存在 `worktree_result.txt`，并证明 `forbidden/denied.txt` 不存在、`allowed/directory.txt` 存在且内容正确。
5. 用普通 Git 找到 `pure-agent-*` 分支并将 worktree child 的 commit 显式 `git cherry-pick` 到主 workspace；不得手工复制该文件，不得让 close_agent 自动整合。
6. 用 `close_agent` 关闭 worktree child，必须显式传 `workspaceDisposition: "cleanup"`。关闭 directory child。随后用普通 Git 证明 Pure-owned worktree 路径和分支均已清理。
7. 在主 workspace 运行 `cargo test`，核对两个最终文件内容。最终回复必须逐项报告两个 Profile、冻结 workspace receipt、目录拒绝、独立 worktree、显式 cherry-pick、显式 cleanup 和最终测试，并把单独一行 `PURE_SUBAGENTS_LIVE_OK` 作为最后一行。
