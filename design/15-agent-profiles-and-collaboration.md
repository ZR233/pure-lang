# 15 - Agent Profile 与统一协作

## 15.1 边界

Studio 的 child Agent 使用与 root 相同的 Thread/Turn/Tool 框架。Profile 冻结模型路由与工作区模式，
但不恢复旧 Task/WorkUnit、completion、delivery review 或自动 merge 体系。父 Agent 负责拆分工作、
避免冲突、审查成果，并用普通 Git 显式整合 worktree child 的 commit。

工作区有三种模式：

- `unrestricted`：Profile 不增加额外项目隔离，root 是 Project root；项目内外仍遵循会话 Permission Mode。
- `directory`：root 仍是 Project root；`writablePaths` 只限制 Pure 内置文件 mutation 工具在项目内的写入。
  它不是 OS 沙箱，shell、Git 与 MCP 可以绕过，工具描述、child 固定上下文和 GUI 必须共同提示该边界。
- `worktree`：root 是独立 Git worktree，boundary 为 `Confined`，worktree 内全可写；主工作区未提交内容
  不复制过去，成果不会自动合并。

## 15.2 用户 Agent 文件

用户 Profile 位于 Studio home 的 `agents/` 目录；默认路径是
`~/.pure/agents/<agent-id>.toml`。目录只扫描第一层普通 `.toml` 文件，文件名 stem 是稳定 id，
不递归读取临时、隐藏或备份文件。单个文件完整表达一个 Agent：

```toml
schema_version = 1
enabled = true
display_name = "Rust 执行者"
description = "实现和重构 Rust 模块"
suitable_tasks = ["Rust 实现", "测试修复"]
workspace_mode = "directory"
system_instructions = """
遵循项目规范完成实现，并验证相关测试。
"""

[model]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"
```

用户 Profile 可选择三种模式。旧文件缺少 `workspace_mode` 时按 `directory` 解释，下一次保存写回
canonical 字段。文件分别解析、校验和原子保存；无效文件保留原字节、从有效目录排除，并以脱敏
warning 暴露。合法但 provider/model 当前不可解析的 Profile 保留在 Settings 中并标记 unavailable。

## 15.3 系统预设

Studio 注册五个系统 Profile：`explorer`、`planner`、`reviewer` 固定为 `unrestricted`，`executor`
固定为 `directory`，`worktree_executor` 固定为 `worktree`。系统 id、名称、用途、指令和模式不可编辑，
但 Agents 设置页可以配置启用状态、provider/model 和由模型声明驱动的 effort。禁用 `planner` 只从
子代理目录排除它，不影响 root 继续使用 planner route。

配置变化只影响未来 spawn。每个 child 创建时冻结 Profile id、正文、provider、model、effort、配置
revision 与 `AgentWorkspaceAssignmentSnapshot`；运行中的 child 不随设置变化，也不在每轮回读 SQLite。
产品 lifecycle 在准备外部资源时直接接收该 frozen Profile snapshot；不得通过父 Agent 配置、当前
Profile 文件或非类型化 metadata 重新推导模型路由与系统指令。

Plan 属于各自的 AgentSession，不随 Profile、消息 fork 或 workspace assignment 复制到 child，也不存在
lineage 共享句柄。root 必须把 child 所需的已批准基线写入 `spawn_agent.message`；child 的 `plan_*` 工具
只操作自己的 session，不能查询 root Plan。配置冻结与 Plan session 隔离是两条独立边界。
`spawn_agent.message` 和 root 后续通过 `send_message` 发送的补充输入都在 child Timeline 中显示为
`parentAgent` 文本消息，并由 Studio 标记为“主代理 / Main agent”；它们对 provider 仍是普通 user role，
不得改变 fork、Plan 隔离、预算刷新或 parent→direct-child 授权语义。

这里保留的是公共功能语义，而不是某个固定 Rust 签名：任何实现 PL host 的产品都必须能在外部资源
产生副作用之前取得“本次 spawn 已冻结的完整 Profile”。该能力用于让产品按同一 provider、model、
effort、system instructions 和 workspace mode 创建容器、远端会话或审计收据。如果 PL 只暴露 child
role 或任意 metadata，产品就只能回读当前配置或继承父 Agent，导致 durable session 与实际执行资源
分裂。后续可以重命名请求类型、拆分生命周期阶段或改为专门 accessor，但不得删除这项能力，也不得
把重新解析 Profile 的责任推回产品层。

## 15.4 spawn 与目录写策略

`spawn_agent` 接收可选 `writablePaths`。只有 `directory` Profile 接受该字段：省略表示整个项目可写，
空数组表示项目内只读；条目是项目相对目录前缀。runtime 拒绝绝对路径、`..`、非法分隔以及解析后
越界或经过不安全 symlink 的路径，规范化和去重后冻结。其他模式传入该字段直接返回参数错误，避免
形成虚假隔离预期。

模型可见的 `spawn_agent` schema 必须从本轮启用的 Profile snapshot 动态生成对象联合，而不是在一个
公共对象上暴露所有模式字段。每个分支以 `profileId` 常量绑定一个 Profile：`directory` 分支声明可选
`writablePaths`，`unrestricted` 与 `worktree` 分支不声明该字段；所有分支都使用
`additionalProperties:false`。schema 同时给出 `profileId -> workspace mode` 映射，供模型在首次调用前
完成选择。用户 Profile 按冻结的 `workspace_mode` 进入相同分支。运行时保留独立硬拒绝，防止绕过
schema 的非法请求；explorer/reviewer 的只读是角色行为合同，不能用 `writablePaths:[]` 模拟。

spawn receipt 返回模式、实际 root、canonical 可写目录；worktree 模式额外返回 branch 与 base commit。
Local 与 SSH backend 的所有 Pure 内置文件 mutation（apply patch、write/delete/copy/move、项目 Skill
写入）都调用同一中央路径策略；该策略不因会话使用 `full-access` 而关闭。读取不受 `writablePaths`
限制，项目外路径仍只由 Permission Mode 决定。

## 15.5 Task Mode 编排合同

Task root 先建立依赖图、文件所有权与验证边界。跨目录检索、历史核验和相互独立的事实收集优先拆成
多个 `explorer` 并行执行；复杂方案比较可以使用 `planner`。这两类 child 默认使用 fresh context，父消息
必须自包含，explorer 返回 `file:line`、符号名、必要原文和不确定项。root 负责综合结论和亲自维护
`design/**`，child 不替代 root 编译或转换 workflow。

实现 child 的任务消息必须完整声明：任务目的与用户价值、设计基线与前置事实、拥有的文件/模块和
不变量、禁止修改范围、按顺序执行的探索/实现/测试/提交步骤、可检查的完成与失败条件、最终 diff/
commit/测试/风险证据，以及 workspace、`writablePaths`、Git 与 cleanup 合同。root 应先按依赖边排序；
无依赖且写集合互斥的任务并行，有真实语义依赖的任务保持顺序。

每个 child 都使用 durable delivery；非 reviewer child 完成实际工作后、final reply 前调用一次
`report_progress`，以 `readyForCompletion` 阶段提交 `CHILD_DELIVERY_READY` 及完整证据；worktree child
还必须在 detail 中提供 `WORKTREE_COMMIT_READY`、40 位 commit 与 workspace root，reviewer 继续使用
专用 verdict marker。
root 从成功 spawn receipt 冻结真实 `agentId`，循环 `wait_agents` 直到该 child terminal，再按该 id 调用
`read_agent_submissions`。progress 唤醒不等于 terminal；空 submission page 是 child 交付合同失败，
`read_agent_session` 只用于诊断和收窄重派，不能作为正常成果 fallback。child 命中预算时
`wait_agents` 以独立的 `budgetLimited` reason 返回，root 不得把它当作 terminal success 或从 pending
集合移除：先分页读取该 child 的 durable Timeline 判断进展，确认状态正常、任务未完成后再以
`send_message` 显式续跑；异常时应收窄指令、关闭或重新派发，而不是无条件续轮。
`wait_agents` 是“任一目标有新事件即返回”，批量等待一次不代表其余目标已完成。root 必须维护尚未
terminal 的 agentId 集合；只有同一次 canonical wait receipt 同时满足 `reason="terminal"`、message 的
`agentId` 精确匹配、`state.agent.kind` 为 `idle` 或 `closed`，且 `lastTurnOutcome` 为 completed，才可从
集合移除该 child。`CHILD_DELIVERY_READY` 出现在 progress message 中只表示成果已发布，不能替代终态；
集合非空时继续等待，所有目标分别取得 terminal receipt 后才能开始读取 submissions。

`read_agent_session` 读取持久化可见 Timeline，而不是 provider 当前 transcript。默认按倒序返回最新
20 条 `user | parentAgent | commentary | final` 文本 Item；调用方可用 opaque cursor 翻页、切换正序，
或请求包含 thinking、tool、agent、turn、inference、skill、file 与 context compaction 的完整 typed
Item。查询在驻留 child 上先等待目标 revision 耐久化，也能读取已经关闭、淘汰或重启后未驻留的 child；
它不激活目标、不修改 ThreadEventBus，且仍不能替代 durable submission。

- 单个边界清晰的实现，或多个写集合完全互斥的并行实现，使用 `executor`；并行时为每个 child 传入
  最窄且互不重叠的 `writablePaths`，禁止 child 借 shell、Git 或 MCP 越界修改、stage、commit 或 reset。
- 会触及共同接口、manifest、lockfile、生成文件、全仓格式化或高风险 Git 状态的任务使用
  `worktree_executor`。每个 child 在独立 worktree 提交，root 顺序审查和采纳；worktree 不能替代真实
  前后依赖的顺序执行。对于任务要求创建的新文件，child 必须先用文件工具创建并以只读工具确认精确
  路径和内容，之后才能执行引用该路径的 `git add` 或 `git commit`；不得用试探性暂存验证文件是否存在。
  状态检查、文件创建、内容确认、测试、暂存、提交和提交复核是独立步骤，不得用 `&&`、`||`、`;` 或
  pipeline 拼成一个 exec，从而保证任一步失败都保留准确的首次调用责任和可重试边界。

Task 默认在 working 后进入 integrating。directory 成果由 root 检查组合 diff 并形成最终提交；worktree
成果由 root 审查 commit、用普通 Git 显式整合；执行者和 worktree 保留至最终审查与验证通过后才 cleanup。
同一并行批次包含多个 worktree child 时，
root 必须先审查并整合该批次全部接受的 commit，第二次及后续整合全部成功后才能发出第一次 cleanup；
随后再逐个 cleanup 并验证对应 branch/worktree 消失，不得按 child 交错执行“整合一个、清理一个”。
root 可在解决冲突时完成保持合并语义所需的
相邻实现和测试修复，但不得借机展开无关重构。合适 child 不可用或失败时，root 先等待容量并收窄重派
一次；仍失败才允许最小实现兜底，并在交付中记录 `ROOT_IMPLEMENTATION_FALLBACK`、原因和直接修改文件。
参数或合同错误不得原样重放：root 先按工具 schema 修正 camelCase 参数、模式专属字段和目标 id，再用
新的调用重试一次；容量或 provider 暂时失败则等待后收窄重派。刻意验证 directory 边界的拒绝必须标记为
expected rejection，禁止绕过，也不计入非预期首次调用失败。

所有成果整合后必须创建 fresh-context 的只读 `reviewer`，综合检查目标、设计、完整 diff、错误路径、
测试、冲突和 fallback。reviewer 不直接修复：代码 finding 回到 working 交给 executor，设计 finding 回到
editing_documents 由 root 修订；重新整合后必须再派新的 reviewer。reviewer 通过后 root 才执行最终门禁
并完成 workflow。reviewer 在最终回复前调用 `report_progress` 形成 durable verdict；root 必须通过
`read_agent_submissions` 读取与冻结 reviewer `agentId`、读取 call ID 绑定的 canonical page。该协作报告
是只读审查的结构化交付，不允许文件/Git/exec 修复，也不能用 root 转述或 session 摘要伪造 approval。

### 原执行者返工与验证证据

root 保存任务读写范围与原执行者 `agentId` 的映射，交付后保持执行者 idle、可续跑，不提前关闭或清理
worktree。代码 finding 必须先用 `send_message` 续跑原执行者，附 finding、当前整合基线、修复范围和
验证记录；只有执行者不可用或原权限无法覆盖修复时才重派，并记录具体原因。设计 finding 仍由 root
修订。worktree 返工前由 root 协调同步 canonical 基线；只采纳本轮新增修复提交，不重复整合旧提交。
每次续跑都重新建立 pending 集合，使用本轮消息之后的 terminal receipt 和 durable submission；旧轮次
完成状态与旧交付不能证明返工完成。重新整合后创建 fresh-context reviewer，全部 approval 且最终验证
通过后才关闭执行者、清理 worktree；停止或失败保留未交付现场并报告原因。

执行者、reviewer 的 durable submission 和最终回复都包含验证记录：实际执行者、完整命令、工作目录、
代码基线（commit 加相关未提交 diff 或文件内容身份）、覆盖范围、环境、结果和工具/日志证据。明确区分
本次实际执行、引用已有证据、尚未验证；没有执行测试也必须说明原因，阅读测试代码不算执行测试。
root 向后续 child 传递已确认的记录，并在最终交付逐项汇总。执行身份以成功 spawn 回执中的 Pure
agentId 为准；child 无法确认时报告角色/范围，由 root 绑定 ID，不使用环境变量、进程或外层宿主 ID。相同命令、相关代码范围与环境未变且已有
成功证据时复用；修改或依赖变化、冲突、失败诊断、覆盖缺口与强制门禁要求重跑时，报告具体原因。
不同角色不机械重复全量检查，最终整合验证与项目强制门禁仍须满足；reviewer 不为补测试越过只读工具边界。
验证记录使用现有 submission detail，不新增生产协议、持久化字段或 GUI 接口。复用会话增加上下文及
provider prompt cache 复用机会，缓存命中只按上游实际 usage 报告，不承诺固定收益。

## 15.6 worktree 生命周期

本地和 SSH 后端都以 spawn 时解析的 `HEAD` 执行 `git worktree add -b`，禁用 hooks 和 credential
helper，最长 120 秒。路径为 `<repo>/.pure/worktrees/<root-thread-id>/<child-id>`，分支使用 Pure-owned
`pure-agent-*` 名称。非 Git 项目或无 HEAD 时 typed 失败。

`studio_objects` 保存版本化 lease：`prepared | active | preserved | cleanupRequested | cleaned`，以及
repo、path、branch、base 与 revision。spawn 任一阶段失败都按 `NoSideEffects | MayHaveCreated` 分类补偿
Thread、热资源、worktree 与 branch。启动恢复只按 durable lease 对账；资源部分缺失或身份不匹配时
保留现场并发布 Recovery issue，不盲删目录或非 Pure 分支。

`close_agent` 对 worktree child 接受 `workspaceDisposition = preserve | cleanup`，默认 `preserve`。
关闭不自动 commit、merge、cherry-pick 或修改主分支。父 Agent 应先审查 child commit、用普通 Git 显式
整合，最终审查与验证通过后再请求 cleanup。已经 preserved 的 lease 在 Agents/Recovery 中显示 revision、branch、base/head、
dirty 与 changed-files 预览，并提供显式清理。

## 15.7 GUI 与验证

Agents 是 canonical Agent 配置中心；不再保留重复 Roles 设置页。系统卡片显示固定模式徽标、启用开关、
provider/model/effort 控件，用户编辑器额外显示三模式选择。所有设置 mutation 携带
`expectedSettingsRevision`，成功后以返回的完整 canonical settings snapshot 原子刷新 UI。

确定性验收覆盖 schema 迁移、模式冻结、目录允许/拒绝/外部路径/symlink、shell 可绕过的显式合同、
本地与 SSH worktree 创建和补偿、preserve/cleanup、重启 reconcile 及 GUI revision。真实验收入口为
`cargo xtask verify-subagents --live --gui`：使用隔离的临时 Studio home 与 Git fixture，并仅在该临时
配置中把会话 Permission Mode 设为 `full-access`，从 GUI 配置并提交真实 Task prompt，证明并行
explorer、两种 executor 的 spawn receipt、详细任务合同、目录拒绝、
worktree 分支、显式整合、整合后的只读 reviewer、最终测试、通过后的 cleanup、截图和 terminal receipt。
artifact 还必须保留每次协作工具调用的 attempt/outcome 分类；成功重试不能隐藏先前的参数、容量或
provider 失败。验收要求每个 child 都有绑定真实 `agentId` 的 canonical nonempty durable submission，
两个 worktree commit 都必须在第一次 cleanup 前显式整合，
并将预期目录拒绝与非预期 tool failure 分开统计。`full-access` 只取消会话级工具审批，不改变 directory
内置文件写策略或 worktree confined assignment，也不得被表述为 directory 的 OS 沙箱。

设置 `PURE_SUBAGENTS_SSH_SERVER`、`PURE_SUBAGENTS_SSH_USERNAME` 与一次性的
`PURE_SUBAGENTS_SSH_PASSWORD` 后，同一入口改用 SSH 实机验收。harness 通过系统 OpenSSH 在远端用户
home 下创建唯一临时 Git fixture；Driver 必须从 Agents 页面切到 SSH 页面，经可见控件保存、测试连接、
浏览并打开该项目，再从 GUI 提交同一 Task prompt。password 只进入 Driver 进程环境、可见密码框和产品
内存 secret lease，不得进入 CLI argv、GUI 进程环境、SQLite、wire 或 artifact。验收额外要求 executor
在不同 inference 中多次以 `cwd:"."` 执行只读命令，所有 SSH `exec.cwd` 首次即为 `.` 或省略，且没有
process id 冲突；成功或失败都按精确路径核验并清理远端 fixture。

验证交付采用简短表格，记录实际执行、引用证据与尚未验证，不要求固定语言或标签。
必要性、返工目标和复审时机由 agent 判断；生产代码只处理通用状态、权限和资源所有权。
验收器保存原始调用与提交供交付审查，不用自然语言关键词代替证据有效性判断。

物理 worktree 清理尊重 Git 锁与注册身份拒绝。注销失败且目录仍存在时，不继续绕过 Git
删除目录或分支；保留现场并返回实际错误，显式解除原因后可重试。
