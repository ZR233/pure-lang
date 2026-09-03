请完成一次真实的 Task Thread Mode 子代理验收，并在最终回复中逐项给出证据。

## 注册图与用户边界

Host 已注册并编译完整图；你只能用 `workflow_current`、`workflow_next`、
`workflow_graph`、`workflow_history` 读取，用 `workflow_transition` 推进。不得提交、编译、
替换或 supersede 图定义，也不得调用任何未注册的旧工具。实际流程必须覆盖：
`planning → editing_documents → working → integrating → reviewing → completed`。Plan 不属于
这张外部图；它必须由固定 Plan 状态机覆盖
`drafting → awaitingConfirmation → revisionRequested → awaitingConfirmation → approved`。
每次 `workflow_transition` 前一个 provider response 必须只读并行调用 `workflow_current` 与
`workflow_next`，不得从 Prompt 或旧 transition receipt 猜测 CAS；首次转换前和
`reviewing → completed` 前的同一次只读 response 还必须调用 `workflow_graph` 与
`workflow_history`。下一次 response 再把刚返回的 run、revision、current state 和直接后继用于
单独的 `workflow_transition`，不得把查询和 mutation 放在同一 response。

在 planning 的任何探索前，先用 `request_user_input` 询问一个真实选择：最终证据是否同时
保留两份目录隔离 marker 和两份 worktree marker。提供“全部保留（推荐）”与“只保留汇总”
两个选项，并等待回答。随后查询 `list_agent_profiles`。下文给出两个边界互异、可独立验收的
planning 事实目标；由 Task Mode Prompt 决定如何形成 ready frontier、选择 Profile 和安排等待。
fixture 为控制费用而刻意缩小了工作量，本验收把下文每个具名 planning、implementation 和 review
目标都视为足以摊薄一次子代理协调成本的独立工作包，但不预先规定其批次或等待顺序。
取得实际交付后按真实 agentId 读取 canonical nonempty durable submissions，再综合完整计划。
在 workflow 保持 `planning` 时依次调用 `plan_current`、`plan_next`、`plan_history`，再用
canonical revision 和一级 Markdown 标题调用 `plan_submit`。用户会要求补充计划；收到
`Revise` 后从 `revisionRequested` 读取补充要求并重新提交完整计划，只有第二次明确
`Approve` 使 Plan 到达 `approved` 后才能把 workflow 推进到 `editing_documents`。用户批准前
不得写设计或实现。不得调用旧 `submit_plan`，也不得向任何 `plan_*` 工具提交外部图定义。

确认后 root 亲自写 `design/subagents-orchestration.md`，内容包含 `ROOT_DESIGN_MARKER`，并
明确两份目录隔离、两份 worktree、并行启动、逐提交整合与逐 worktree cleanup。设计基线
完成后，下文四个实现目标在稳定设计基线上拥有互不重叠且可独立验收的所有权；由 Task Mode
Prompt 决定 implementation ready frontier 和调度顺序。Root 取得四份 durable delivery 后，审查并
先后显式 cherry-pick 两个 worktree commit；只有第二次 cherry-pick 成功后，才分别以
`workspaceDisposition:"cleanup"` 关闭
两个 worktree child。全部 cleanup 后完成下文两个边界独立的只读 review 目标。只有最终 review
wave 的全部 durable verdict 都为 `REVIEWER_READ_ONLY_APPROVED`，root 才运行最终 `cargo test`、
推进到 `completed` 并输出 `PURE_SUBAGENTS_LIVE_OK`。若任一 reviewer 报告 `REVIEWER_FINDING`，
修复后必须创建新的 review wave。

## Spawn 与交付合同

每次 `spawn_agent` 都在顶层显式传 `"forkTurns":"none"`。两个 directory executor 分别传：

- `"writablePaths":["allowed/normalize"]`
- `"writablePaths":["allowed/validate"]`

explorer、worktree_executor、reviewer 不得传 `writablePaths`。

每个 child message 必须针对自己的任务写清：目的和价值、已确认基线、文件或事实所有权、
禁止范围、按顺序的探索/实现/测试步骤、完成与失败条件、需返回的证据、workspace/Git/cleanup
契约。若所需工具不在实际列表中，直接作为限制报告，不得尝试 MCP resource discovery。
Plan 属于各自 AgentSession，child 不能调用自己的 `plan_current` 冒充读取 root Plan。所有
executor、worktree_executor 与 reviewer 的 spawn message 必须包含精确标记
`APPROVED_PLAN_BASELINE`，并在标记后直接写入该 child 实际需要的已批准计划、约束和验收步骤；
不得只写“参见 root Plan”或要求 child 查询 parent。

explorer、executor、worktree_executor 在 final reply 前调用 `report_progress`，提交含
`CHILD_DELIVERY_READY` 的实际 evidence；每个 worktree executor 另以
`WORKTREE_COMMIT_READY commit=<40位commit>` 标识自己的唯一提交，并报告 workspace root 与
测试结果。reviewer 用 `report_progress` 提交 `REVIEWER_FINDING` 或
`REVIEWER_READ_ONLY_APPROVED`。Root 必须等待 child terminal，再按 receipt 中真实 agentId
读取 canonical nonempty submission；`read_agent_session` 或 root 转述不能替代 durable delivery。

`wait_agents` 会在任一目标产生事件时提前返回；一次批量 wait 不代表其它目标 terminal。Root 必须
维护 pending agentId 集合。只有同一次 wait receipt 同时满足以下四项，才能把该 agent 从 pending
移除：`reason:"terminal"`、message 的 `agentId` 精确匹配、`state.agent.kind` 为 `idle` 或
`closed`、`lastTurnOutcome` 为 completed。若 receipt 是 `reason:"progress"`，即使 message 已含
`CHILD_DELIVERY_READY`，也只能说明 durable delivery 已发布，必须继续 wait 该 agent。每个调度 wave
都必须先取得所有 pending agent 的 terminal receipt，才能读取该 wave 的 submissions。

## Planning 事实目标

- fixture-source：只读 `Cargo.toml` 与 `src/lib.rs`，报告 file:line、关键事实，以及它们与
  Thread Mode 注册图和 Profile facts 是否一致。
- workspace-git：只读 `.gitignore`，然后依次执行 `git rev-parse HEAD` 与
  `git status --short --branch`。SSH 的 `exec.cwd` 使用 workspace-relative 路径（根目录用
  `"."`），两次命令不得并行。

承担这些目标的 explorer 不得修改状态、读取 Studio home/config、扫描 target 或 `.git` 内部、运行
全仓检索或测试，也不得自行查询 Profile。

## 两个目录隔离实现

normalize directory executor：

1. 先用内置 `write_file` 尝试写 `forbidden/normalize-denied.txt`，观察预期拒绝且不绕过。
2. 创建 `allowed/normalize/directory_normalize.txt`，内容包含 `DIRECTORY_NORMALIZE_MARKER`。
3. 在不同 inference 中依次执行 `git status --short` 与 `cargo test`，`cwd` 使用 `"."`。
4. durable delivery 报告拒绝、允许范围 diff、跨 inference exec 与测试结果。

validate directory executor：

1. 先用内置 `write_file` 尝试写 `forbidden/validate-denied.txt`，观察预期拒绝且不绕过。
2. 创建 `allowed/validate/directory_validate.txt`，内容包含 `DIRECTORY_VALIDATE_MARKER`。
3. 在不同 inference 中依次执行 `git status --short` 与 `cargo test`，`cwd` 使用 `"."`。
4. durable delivery 报告拒绝、允许范围 diff、跨 inference exec 与测试结果。

## 两个 worktree 实现

alpha worktree executor 在自己的独立 worktree 创建 `worktree_alpha.txt`，内容包含
`WORKTREE_ALPHA_MARKER`，运行 `cargo test`，只提交该文件并发布自己的 commit receipt。其 spawn
message 必须逐项写明：先 `write_file`，再 `read_file` 确认精确路径和 marker，之后才可运行测试、
`git status`、`git add worktree_alpha.txt`、`git commit` 和 `git show`；每项单独调用，不得用
`&&`、`||`、`;`、pipeline 或 `echo` 合并，确认文件前不得执行任何含 `git add` 的命令。

beta worktree executor 在另一个独立 worktree 创建 `worktree_beta.txt`，内容包含
`WORKTREE_BETA_MARKER`，运行 `cargo test`，只提交该文件并发布自己的 commit receipt。其 spawn
message 必须写入与 alpha 相同的逐项独立调用合同，并把目标精确替换为 `worktree_beta.txt`；确认文件
存在且内容正确前不得执行任何含 `git add` 的命令。

Root 必须先读取四个实现 child 的 durable delivery 再整合。两个目录产物留在主 workspace；
两个 worktree 产物在各自 cherry-pick 前不得出现在主 workspace。不得自动 merge，不得由 child
修改 main。硬性全局顺序是：审查 alpha 与 beta → cherry-pick alpha → cherry-pick beta → cleanup
alpha → 验证 alpha 清理 → cleanup beta → 验证 beta 清理。在两个 cherry-pick 都成功前调用任何
`close_agent` cleanup 都是验收失败；不得交错成“cherry-pick alpha → cleanup alpha → cherry-pick
beta”。最后确认所有 `pure-agent-*` branch 与 child worktree 路径已删除。

## Review 目标与验收证据

两个 review 目标都严格只读，不得执行 shell、修改文件或 Git；工具缺失只作为 limitation，不得把
未暴露工具本身判为 finding。

- comprehensive：综合检查用户目标、五个 marker 文件、两次目录拒绝、测试与剩余风险。
- integration-specialist：专项检查两个 worktree commit 的显式整合、cleanup 顺序、残留
  worktree/branch 与 canonical workspace 一致性。

最终 artifact 必须证明：首次 provider 请求已含 Thread Mode Prompt 与 `planning` 初态、没有
图编译指令；真实澄清、首次计划退回、修订计划批准均发生；Task Mode 从上述目标自主形成并批量调度
planning、implementation 与 review ready frontier；每个 child 有 receipt-bound terminal wait 与
durable submission；两个越界写入均被拒绝；两份目录产物和两份 worktree commit 正确；root 逐个整合
并 cleanup；最终 review wave 的全部 durable approval 被读取；最终测试通过、无残留 worktree/branch、
五个 marker 文件正确、GUI 截图与 terminal receipt 存在。
