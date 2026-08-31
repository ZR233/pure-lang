请严格完成一次 Task 子代理验收，并在最终回复中逐项给出证据。

以下八段是不可省略的 live acceptance hard gates；root 必须将对应段落的具体要求原样落实到每个 child message，并以 wire receipts 证明调用顺序。

根 Agent 必须在每一个 spawn_agent 的 message（两个 explorer、executor、worktree_executor、reviewer）中，按以下同一顺序逐字包含八个 CHILD_CONTRACT marker；每一段必须是该 child 的具体内容，不能只引用本文件。两个 explorer 的目的和 ownership 必须不同。

spawn 调用参数硬门禁：每一次 `spawn_agent` 都必须在顶层参数中显式传入 `"forkTurns":"none"`。不接受省略后依赖默认值，不接受只把 `forkTurns:none` 写进 child message、计划、证据或事后转述。两个 explorer 的调用形状必须是 `spawn_agent({"profileId":"explorer","forkTurns":"none","message":"..."})`；executor 必须是 `spawn_agent({"profileId":"executor","forkTurns":"none","writablePaths":["allowed"],"message":"..."})`；worktree_executor 与 reviewer 也必须分别在各自 `spawn_agent` 顶层显式传入 `"forkTurns":"none"`，且不得传 `writablePaths`。

planning 阶段由 root 调用 list_agent_profiles，随后并行 spawn 两个 explorer；这两次 explorer `spawn_agent` 的顶层参数都必须显式包含 `"forkTurns":"none"`。Profile 查询和 workflow compile 是 root-only：root 把查询结果、已编译阶段图 `planning → awaiting_confirmation → editing_documents → working → integrating → reviewing → completed` 以及 Profile/spawn facts 作为已确认的 baseline 事实写入 child message，explorer child 不得调用 list_agent_profiles，也不得重新核验 Profile 配置。两个 explorer 都进入 terminal 后，root 分别读取它们的 submissions、综合事实形成计划，再调用 request_user_input 请求确认。确认后 root 才能写入 design/subagents-orchestration.md，并在文档基线完成后进入实现。

两个 explorer 的任务必须轻量、互异并在固定有限步骤后直接 final reply：探索者一只读核对 Task workflow 与 live artifact，只允许读取 fixture 的 Cargo.toml 与 src/lib.rs，并将这两个文件的 fixture/live facts 与 root 注入的已编译阶段图和 Profile/spawn facts 对照；探索者二只读核对 workspace 与 Git lifecycle，只允许读取 fixture 的 .gitignore，并调用 git_workspace_info 与 git_status 核对 Git 元数据。两者均只读，不得调用 skill_view，不得读取 Studio home 或配置，不得访问 studio-config，不得全仓 rg，不得扫描 target/ 或 .git/ 内部，不得运行 cargo test，也不得扩展到 fixture 之外。每个 explorer message 仍须完整包含以下八段，但其 steps 只能写本段规定的有限步骤；不得把 root-only 的 Profile 查询、确认、设计写入或实现编排复制给 child。

[[CHILD_CONTRACT:purpose]]
探索者一只读核对 Task workflow 与 live artifact：用 Cargo.toml 与 src/lib.rs 的 fixture/live facts 对照 root 注入的已编译阶段图和 Profile/spawn facts；探索者二只读核对 workspace 与 Git lifecycle：核对 .gitignore 与 git_workspace_info/git_status 返回的 Git 元数据。executor 在 directory assignment 中创建 allowed/directory.txt，且该文件内容必须逐字包含 DIRECTORY_MARKER；worktree_executor 在独立 worktree 中创建 worktree_result.txt，且该文件内容必须逐字包含 WORKTREE_RESULT_MARKER，再提交该文件；reviewer 只读复审。

[[CHILD_CONTRACT:baseline]]
基线是当前仓库 HEAD；Task 使用 mode.task，四个内置 Profile 的 system instructions 与 workspace mode 必须冻结在 spawn receipt 中。root 先编译 workflow、查询四个 Profile，再把完整已编译阶段图以及 profileId、启用状态、route、workspace mode 与预期 spawn assignment 作为已确认事实注入每个 child baseline；child 不自行查询配置。validator 固定要求 allowed/directory.txt 含 DIRECTORY_MARKER、worktree_result.txt 含 WORKTREE_RESULT_MARKER，不得以修改 validator 或只报告 completion marker 替代文件内容；禁止复制 immutable Profile 首行到本段或 child message。

[[CHILD_CONTRACT:ownership]]
root 在 confirmation 后亲自使用内置 write_file 创建 design/subagents-orchestration.md，内容包含 ROOT_DESIGN_MARKER，并拥有整合、cherry-pick 和 cleanup；探索者一只拥有 Task workflow/live artifact 事实核对，探索者二只拥有 workspace/Git lifecycle 事实核对；executor 仅拥有 allowed，负责写入含 DIRECTORY_MARKER 的 directory.txt；worktree_executor 仅拥有自己的 worktree，负责写入含 WORKTREE_RESULT_MARKER 的 worktree_result.txt 并提交；explorer/reviewer 只读。directory 变更留在主 workspace 由 root 最终组合，root 不得为 directory child 产物额外提交后再 spawn worktree。

[[CHILD_CONTRACT:forbidden]]
executor 必须先尝试用内置 write_file 写 forbidden/denied.txt，预期被 writablePaths 拒绝，不得绕过边界；拒绝后仍须用内置 write_file 创建 allowed/directory.txt，并让内容逐字包含 DIRECTORY_MARKER；不得由 child 整合 worktree，不得由 reviewer 修改任何文件或 Git 状态。

[[CHILD_CONTRACT:steps]]
两个 explorer 的 steps 必须分别原样复制以下版本化 canonical block，不能增删、改写或追加动作。
steps 段允许且只允许在完整 canonical block 外包一层 Markdown `text` 展示围栏（恰为开头 ```text、结尾 ```）；围栏不是动作，也不得复制其它说明。不得使用其它围栏语言、嵌套围栏，或在围栏内外添加任何前后缀、动作或说明。

```text
LIVE_EXPLORER_STEPS_V1: fixture-source
1. 只读读取 Cargo.toml。
2. 只读读取 src/lib.rs。
3. 输出要求：总结这两个文件中与 Task workflow 和 live artifact 相关的有限源码事实，并与 root 注入的已编译阶段图及 Profile/spawn facts 对照；完成后直接 final reply。
```

```text
LIVE_EXPLORER_STEPS_V1: workspace-git
1. 只读读取 .gitignore。
2. 只调用 git_workspace_info。
3. 只调用 git_status。
4. 输出要求：总结 workspace/Git lifecycle 元数据，并在完成后直接 final reply。
```

root 完成 workflow compile 后，在 planning 调用 list_agent_profiles 确认四个 Profile，并在同一并行工具批次连续 spawn 两个 explorer；两次调用都必须显式传顶层 `"forkTurns":"none"`，不得在两次 explorer spawn 之间插入任何其他工具调用，不得先请求确认或写设计。root 对 explorer 的 root-only 编排要求与越界禁止项仍适用，但不属于 explorer canonical steps。root 对 spawn receipt 中两个真实 agentId 调用 wait_agents；若一次只返回部分 terminal targets，继续只等待尚未 terminal 的 target，直到 same-callId canonical wait outputs 分别为两个真实 agentId 给出 reason:terminal。随后 root 必须分别调用 read_agent_submissions({target:真实 agentId}) 读取两个 explorer submissions，必要时才分别 read_agent_session 补充摘要；两个 explorer submissions 都读取后，root 综合事实形成计划、进入 awaiting_confirmation，再调用 request_user_input 请求确认。确认后 root 进入 editing_documents，亲自 write_file design/subagents-orchestration.md；设计 marker 写入完成后才进入 working。root 随后必须在同一并行工具批次连续发出 executor 与 worktree_executor 两个 spawn_agent，且两次调用都必须显式传顶层 `"forkTurns":"none"`；只有 executor 额外传 `writablePaths:["allowed"]`，worktree_executor 不得传 `writablePaths`。不得在两次 implementation spawn 之间插入任何其他工具调用；两次 implementation spawn 均完成后才能对任一实现调用 wait/read。不得让 directory child 的提交、产物读取或其他真实结果成为 worktree_executor spawn 的前置条件；directory 变更可留在主 workspace 由 root 最终组合。executor 先观察拒绝，再写 allowed/directory.txt，且文件内容逐字包含 DIRECTORY_MARKER；root 必须针对其 agentId 调用 read_agent_submissions。worktree_executor 写 worktree_result.txt，且文件内容逐字包含 WORKTREE_RESULT_MARKER，运行 cargo test，提交固定 subject feat: worktree executor marker 并报告 40 位 hash；root 必须针对其 agentId 调用 read_agent_submissions 后才 cherry-pick。root 隔离证明后 cherry-pick、close_agent(workspaceDisposition:cleanup)，最后 spawn reviewer；reviewer 的 `spawn_agent` 也必须显式传顶层 `"forkTurns":"none"` 且不得传 `writablePaths`。reviewer 只能使用 read_file、list_files、stat_path、lsp_capabilities、lsp_query、git_status、git_diff、git_workspace_info、read_session_note、search_session_note、report_progress，不得 exec/cargo test；reviewer 完成只读检查后、final reply 前必须调用 report_progress 提交最终 durable verdict；在此之前可提交不含最终 marker 的中间 progress，且这些中间 submission 不可替代最终 verdict。最终 submission 以 REVIEWER_FINDING 或 REVIEWER_READ_ONLY_APPROVED 生成 durable verdict，不得借该工具修改 workspace、Git 或外部状态。root 必须从 spawn receipt 取得 reviewer agentId，按 reviewer agentId 调用 read_agent_submissions，并确认 same callId 的 canonical nonempty page 包含 marker；root/session 转述或 read_agent_session 不算。root targeted read 到该最终 durable verdict 后才执行最终 cargo test。

[[CHILD_CONTRACT:completion_failure]]
executor 必须报告 DIRECTORY_DENIAL_OBSERVED，并确认 allowed/directory.txt 内容含 DIRECTORY_MARKER；worktree_executor 必须报告 WORKTREE_COMMIT_READY、40 位 commit hash 和 workspace root，并确认 worktree_result.txt 内容含 WORKTREE_RESULT_MARKER；reviewer 只读核对文件、marker、sentinel，并通过 report_progress durable submission 报告 REVIEWER_FINDING 或 REVIEWER_READ_ONLY_APPROVED；root targeted read_agent_submissions 读到 REVIEWER_READ_ONLY_APPROVED 后执行最终 cargo test。任一步失败都必须保留错误证据并停止伪造成功。

[[CHILD_CONTRACT:evidence]]
记录四个 Profile 的 profileId、forkTurns:none、workspace receipt、工具调用顺序（尤其是两个 explorer submissions 都读取后，executor 与 worktree_executor 两次 spawn 在任一 implementation wait/read 之前完成）、拒绝原因、独立分支和最终测试；记录两个固定文件的逐字 marker DIRECTORY_MARKER 与 WORKTREE_RESULT_MARKER；记录 reviewer report_progress 调用以及 root 绑定 reviewer agentId/callId 得到的 canonical nonempty submission page；最终成功标记为 PURE_SUBAGENTS_LIVE_OK。若 reviewer durable submission 输出 REVIEWER_FINDING，必须重新 spawn 不同 callId 的 implementation 与 reviewer，并提供第二次 integration 证据及新的 targeted submission read 后才可 approval。

[[CHILD_CONTRACT:workspace_git_cleanup]]
directory workspace 使用 writablePaths:["allowed"]，只保留含 DIRECTORY_MARKER 的 allowed/directory.txt 作为 root 可组合产物；worktree 使用 pure-agent-* 分支和独立路径，只提交含 WORKTREE_RESULT_MARKER 的 worktree_result.txt；root 只能在核对未整合状态后 git cherry-pick，并在 close_agent 时显式 workspaceDisposition:"cleanup"，确认路径和分支均已删除。不得先额外提交 directory child 产物再 spawn worktree。
