请完成一次真实的 Task 子代理验收，并在最终回复中逐项给出证据。

## 目标与阶段

首次 `workflow_state.compile` 就提交合法阶段图：
`planning → awaiting_confirmation → editing_documents → working → integrating → reviewing → completed`，另有终态 `stopped`。`completed` 与 `stopped` 不得有 outgoing transition。

Root 在 planning 查询 `list_agent_profiles`，并行派出两个任务互异的只读 explorer。两者 terminal 后，按真实 agentId 读取 canonical nonempty durable submissions，再综合为以一级 Markdown 标题开头的完整计划并调用 `submit_plan`。`request_user_input` 只可用于真正的澄清，不能替代提交计划。用户确认前不得写设计或实现。

确认后 root 亲自写 `design/subagents-orchestration.md`，内容包含 `ROOT_DESIGN_MARKER`。设计基线完成后，并行派出 directory executor 与 worktree executor；两者都必须在首次 wait/read 前完成 spawn。实现完成后 root 审查并显式 cherry-pick worktree commit，再以 `workspaceDisposition:"cleanup"` 关闭 worktree child。cleanup 后派全新的只读 reviewer。最终 reviewer durable verdict 为 `REVIEWER_READ_ONLY_APPROVED` 后，root 才运行最终 `cargo test` 并输出 `PURE_SUBAGENTS_LIVE_OK`。若 reviewer 报告 `REVIEWER_FINDING`，修复并重新整合后必须派新的 reviewer。

## Spawn 与交付合同

每次 `spawn_agent` 都在顶层显式传 `"forkTurns":"none"`。executor 额外传 `"writablePaths":["allowed"]`；explorer、worktree_executor、reviewer 不得传 `writablePaths`。

每个 child message 必须针对自己的任务写清：目的和价值、已确认基线、文件或事实所有权、禁止范围、按顺序的探索/实现/测试步骤、完成与失败条件、需返回的证据、workspace/Git/cleanup 契约。允许自由措辞和补充上下文，不要求复制固定段落或 marker 模板。若所需工具不在实际列表中，直接把它作为限制报告；不得尝试 MCP resource discovery 或其它未授权替代工具。

explorer、executor、worktree_executor 在 final reply 前调用 `report_progress` 提交含 `CHILD_DELIVERY_READY` 的实际 evidence；worktree executor 的交付另含 `WORKTREE_COMMIT_READY`、commit hash、workspace root 与测试结果。reviewer 用 `report_progress` 提交 `REVIEWER_FINDING` 或 `REVIEWER_READ_ONLY_APPROVED`。Root 必须等待 child terminal，并按 receipt 中真实 agentId 读取 canonical nonempty submission；`read_agent_session` 或 root 转述不能替代 durable delivery。

## 两个只读探索任务

- fixture-source：只读 `Cargo.toml` 与 `src/lib.rs`，报告 file:line、关键事实，以及它们与 root 注入的阶段图和 Profile facts 是否一致。
- workspace-git：只读 `.gitignore`，然后依次执行 `git rev-parse HEAD` 与 `git status --short --branch`。SSH 的 `exec.cwd` 使用 workspace-relative 路径（根目录用 `"."`），两次命令不得并行。

两个 explorer 都不得修改状态、读取 Studio home/config、扫描 target 或 `.git` 内部、运行全仓检索或测试，也不得自行查询 Profile。两次 explorer spawn 都必须发生在首次 wait/read 之前；不要求在 wire 中相邻，也不限制合法收窄重派次数。

## 两种实现与隔离

directory executor：

1. 先用内置 `write_file` 尝试写 `forbidden/denied.txt`，观察 writablePaths 的预期拒绝；不得绕过边界。
2. 用内置 `write_file` 创建 `allowed/directory.txt`，内容包含 `DIRECTORY_MARKER`。
3. 在不同 inference 中依次执行 `git status --short` 与 `cargo test`，`cwd` 使用 `"."`；先消费前一结果再调用下一条。
4. durable delivery 报告目录拒绝、允许范围 diff、两次 SSH-safe exec 与测试结果。

worktree executor：

1. 在独立 worktree 创建 `worktree_result.txt`，内容包含 `WORKTREE_RESULT_MARKER`。
2. 运行 `cargo test` 并提交该文件。
3. durable delivery 报告 `WORKTREE_COMMIT_READY`、40 位 commit hash、workspace root 和测试结果。

Root 必须在读取两个实现 child 的 durable delivery 后才能整合。directory 产物留在主 workspace 由 root 组合；worktree 产物在整合前不得出现在主 workspace。不得自动 merge，不得由 child 修改主分支。Root 审查 commit 后使用普通 Git 显式整合，随后 cleanup，并确认 Pure-owned branch 与 worktree 路径已删除。

## Reviewer 与验收证据

Reviewer 严格只读，只检查目标、三个 marker 文件、整合结果、错误路径、测试和剩余风险；不得执行 shell、修改文件或 Git。实际工具缺失只作为 limitation 报告，不得把“没有未暴露的 Git 工具”本身判为代码 finding。

最终 artifact 必须证明：

- `submit_plan` 被用于计划，且探索交付发生在确认和实现前；
- 两个互异 explorer 在首次等待前均已 spawn，并有绑定真实 agentId 的 terminal wait 与 durable submission；
- directory/worktree 的冻结 workspace receipt 正确；
- 至少一次预期目录越界拒绝，允许目录产物正确，且除此之外没有工具失败；
- SSH executor 有跨 inference 的 workspace-relative `exec`，没有绝对 cwd 或重复 process ID；
- worktree commit 在整合前隔离，root 显式整合并 cleanup；
- fresh reviewer 只读且最终 durable approval 被 root 读取；
- 最终测试通过、fixture 无残留 worktree/branch、三个 marker 文件正确、GUI 截图与 terminal receipt 存在，并输出 `PURE_SUBAGENTS_LIVE_OK`。
