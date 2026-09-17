# 12 - Agent Profile 与协作编排

本文是 Profile 体系、child 派发、durable delivery、wait 语义、reviewer 门禁与返工合同的唯一
权威源；权限与工作区安全语义见 [04](./04-security.md)，通用工具运行时见
[09](./09-tool-runtime.md)，Task Mode 图与阶段见 [11](./11-thread-mode.md)。

## 12.1 边界与工作区模式

Studio 的 child Agent 使用与 root 相同的 Thread/Turn/Tool 框架。Profile 冻结模型路由与工作区
模式；父 Agent 负责拆分工作、避免冲突、审查成果，并用普通 Git 显式整合 worktree child 的
commit，不存在自动 merge 或 delivery gate。

工作区有三种模式：

- `unrestricted`：Profile 不增加额外项目隔离，root 是 Project root；项目内外仍遵循会话
  Permission Mode。
- `directory`：root 仍是 Project root；`writablePaths` 只限制 Pure 内置文件 mutation 工具在
  项目内的写入。它不是 OS 沙箱，shell、Git 与 MCP 可以绕过，工具描述、child 固定上下文和
  GUI 必须共同提示该边界。
- `worktree`：root 是独立 Git worktree，boundary 为 Confined，worktree 内全可写；主工作区
  未提交内容不复制过去，成果不会自动合并。

## 12.2 用户 Profile 与系统预设

用户 Profile 位于 Studio home 的 `agents/` 目录，默认路径是
`~/.anywork/agents/<agent-id>.toml`。目录只扫描第一层普通 `.toml` 文件，文件名 stem 是稳定
id，不递归读取临时、隐藏或备份文件。单个文件完整表达一个 Agent：

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
model = "deepseek-flash"
effort = "high"
```

用户 Profile 可选择三种模式。旧文件缺少 `workspace_mode` 时按 `directory` 解释，下一次保存
写回 canonical 字段。文件分别解析、校验和原子保存；无效文件保留原字节、从有效目录排除，
并以脱敏 warning 暴露。合法但 provider/model 当前不可解析的 Profile 保留在 Settings 中并
标记 unavailable。

Studio 注册五个系统 Profile：`explorer`、`planner`、`reviewer` 固定为 unrestricted，
`executor` 固定为 directory，`worktree_executor` 固定为 worktree。系统 id、名称、用途、指令
和模式不可编辑，但 Agents 设置页可以配置启用状态、provider/model 和由模型声明驱动的
effort。禁用 `planner` 只从子代理目录排除它，不影响 root 继续使用 planner route。

每个 child 创建时冻结 Profile id、正文与工作区分配快照，保存初建 provider、model、effort
与配置 revision。后续配置更新由宿主事件处理：模型绑定在下一 Turn 生效，工具权限和目录按
热刷新契约更新，不重写创建指令、不偷切物理工作区，也不在每轮回读 SQLite。产品在准备初始
外部资源时接收同一冻结 Profile 快照，不能从父 Agent 配置或非类型化 metadata 猜测权限和
系统指令。本文保留的是公共功能语义而不是固定 Rust 签名：任何实现 PL host 的产品都必须能
在外部资源产生副作用之前取得"本次 spawn 已冻结的完整 Profile"，用于按同一 provider、
model、effort、system instructions 和 workspace mode 创建容器、远端会话或审计收据；后续
可以重命名请求类型或拆分生命周期阶段，但不得删除这项能力，也不得把重新解析 Profile 的
责任推回产品层。

Plan 属于各自的 Thread，不随 Profile、消息 fork 或 workspace assignment 复制到 child，也不
存在 lineage 共享句柄。root 必须把 child 所需的已批准基线写入 `spawn_agent.message`；
child 的 `plan_*` 工具只操作自己的 session，不能查询 root Plan。配置冻结与 Plan session
隔离是两条独立边界。`spawn_agent.message` 和 root 后续通过 `send_message` 发送的补充输入
都在 child Timeline 中显示为 `parentAgent` 文本消息，并由 Studio 标记为"主智能体 / Main
agent"；它们对 provider 仍是普通 user role，不得改变 fork、Plan 隔离、预算刷新或
parent→direct-child 授权语义。

## 12.3 spawn 与目录写策略

`spawn_agent` 必须提供 `taskSummary` 与完整 `message`。概要由调用者用简短语句描述本次
任务，折叠空白后必须非空且不超过 80 个 Unicode 字符；缺失或非法概要在分配资源前拒绝，
不截断。Studio 通过类型化创建请求接收概要，作为 child 的初始 canonical Thread title
保存和发布。后续消息、进度与完成汇报不覆盖该标题；显式重命名沿用 Thread title 合同。
既有 child 保留保存的标题，不根据历史任务正文补写概要。

root 与所有 child 共用可见进展合同：非平凡任务首次工具调用前、重要发现、阶段切换、
等待和阻塞时输出 1–3 句 commentary，说明已确认进展和下一步。长任务持续提供有信息量
的更新，不为每批工具机械重复播报；隐藏推理和最终答复不替代执行期间的可见进展。

`spawn_agent` 接收可选 `writablePaths`。只有 directory Profile 接受该字段：省略表示整个项目
可写，空数组表示项目内只读；条目是项目相对目录前缀。runtime 拒绝绝对路径、`..`、非法分隔
以及解析后越界或经过不安全 symlink 的路径，规范化和去重后冻结。其他模式传入该字段直接
返回参数错误，避免形成虚假隔离预期。

模型可见的 `spawn_agent` schema 必须从本轮启用的 Profile 快照动态生成对象联合（oneOf），
而不是在一个公共对象上暴露所有模式字段。每个分支以 `profileId` 常量绑定一个 Profile：
directory 分支声明可选 `writablePaths`，unrestricted 与 worktree 分支不声明该字段；所有分支
都使用 `additionalProperties:false`。schema 同时给出 profileId → workspace mode 映射，供模型
在首次调用前完成选择。用户 Profile 按冻结的 workspace_mode 进入相同分支。运行时保留独立
硬拒绝，防止绕过 schema 的非法请求；explorer/reviewer 的只读是角色行为合同，不能用
`writablePaths:[]` 模拟。schema 约束用于降低模型首次调用错误，执行路径仍按 Profile 快照
重做同一语义校验，不能把 schema 当作安全边界。

spawn receipt 返回模式、实际 root、canonical 可写目录；worktree 模式额外返回 branch 与
base commit。Local 与 SSH backend 的所有 Pure 内置文件 mutation（apply patch、
write/delete/copy/move、项目 Skill 写入）都调用同一中央路径策略；该策略不因会话使用
full-access 而关闭。读取不受 `writablePaths` 限制，项目外路径仍只由 Permission Mode 决定。

协作查询（包括 `list_agents`）只读取宿主授权范围内的快照，允许与普通读取工具出现在同一
模型批次；该批次不承诺跨工具的联合事务快照。创建、发送消息、打断和关闭代理会改变协调
状态，仍必须单独调用，不能与查询或其他工具混批。

## 12.4 Task 编排合同

### 任务消息与依赖

Task root 先建立依赖图、文件所有权与验证边界。跨目录检索、历史核验和相互独立的事实收集
优先拆成多个 `explorer` 并行执行；复杂方案比较可以使用 `planner`。这两类 child 默认使用
fresh context，父消息必须自包含，explorer 返回 `file:line`、符号名、必要原文和不确定项；
root 负责综合结论和亲自维护 `design/**`，child 不替代 root 编译或转换 workflow。

实现 child 的任务消息必须完整声明：任务目的与用户价值、设计基线与前置事实、拥有的
文件/模块和不变量、禁止修改范围、按顺序执行的探索/实现/测试/提交步骤、可检查的完成与
失败条件、最终 diff/commit/测试/风险证据，以及 workspace、`writablePaths`、Git 与 cleanup
合同。root 应先按依赖边排序：无依赖且写集合互斥的任务并行，有真实语义依赖的任务保持
顺序；只有极少操作的微任务、重复目标、需要共享未稳定上下文或真实前后依赖的工作不得为
增加 agent 数量而拆分。

- 单个边界清晰的实现，或多个写集合完全互斥的并行实现，使用 `executor`；并行时为每个
  child 传入最窄且互不重叠的 `writablePaths`，禁止 child 借 shell、Git 或 MCP 越界修改、
  stage、commit 或 reset。
- 会触及共同接口、manifest、lockfile、生成文件、全仓格式化或高风险 Git 状态的任务使用
  `worktree_executor`。每个 child 在独立 worktree 提交，root 顺序审查和采纳；worktree 不能
  替代真实前后依赖的顺序执行。对于任务要求创建的新文件，child 必须先用文件工具创建并以
  只读工具确认精确路径和内容，之后才能执行引用该路径的 `git add` 或 `git commit`，不得用
  试探性暂存验证文件是否存在。状态检查、文件创建、内容确认、测试、暂存、提交和提交复核
  是独立步骤，不得用 `&&`、`||`、`;` 或 pipeline 拼成一个 exec，保证任一步失败都保留准确的
  首次调用责任和可重试边界。

平凡单文件任务保留所有阶段、Plan 批准和独立 reviewer；计划目标不超过八行，不机械展开
DAG 或验证表，无独立探索价值时不派 explorer。root 可以承担协调成本高于工作量的写入；无
文档或整合变更的阶段复用已有事实简短说明；阶段切换仍按新鲜查询和 Solo mutation 执行。

### ready frontier 调度

planning 开始和每批 child 交付后，root 都执行一次成本感知的并行化分析：从需求、仓库边界
和验证目标识别可交付节点；仅当多个实质交付存在依赖时，为合格节点记录前置依赖、读写
范围、适用 Profile、交付证据与 root-only 标记，形成任务 DAG。依赖已满足、边界清楚、可
独立验收且预计能缩短关键路径或显著增加独立证据的节点构成 ready frontier；root 必须在
首次 `wait` 前派出该前沿的全部 child，等待期间继续处理未委托的综合和编排工作，不得重复
child 的任务。收齐本批 durable delivery 后，root 更新 DAG 并立即释放下一 ready frontier，
直到没有剩余节点。调度不使用固定 agent 数量。

### durable delivery 与收据

每个 child 都使用 durable delivery；非 reviewer child 完成实际工作后、final reply 前调用
一次 `report_progress`，以 `readyForCompletion` 阶段提交 `CHILD_DELIVERY_READY` 及完整证据；
worktree child 还必须在 detail 中提供 `WORKTREE_COMMIT_READY`、40 位 commit 与 workspace
root，reviewer 继续使用专用 verdict marker。root 从成功 spawn receipt 保存 `agentId`、
`profileId`、`messageAccepted` 和 `messageSequence`；`send_message` 返回 `target`、
`messageId` 与 `sequence`。这些是消息准入收据，不携带 `turnId`，也不代表执行完成。每个
child 同时只保留一个待交付任务；复用前记录已知 `lastTurn.turnId` 与通知
`commitSequence`，后续必须看到新 Turn，不能用旧完成或旧 submission 满足新任务。消息
sequence 与 journal commitSequence 属于不同域。

### wait 语义

父代理没有独立工作时，使用 `wait`（空任务列表，`timeoutMs` 上限 300000）等待 Thread 消息；
这是最长等待时间，消息到达会提前唤醒；只有具体独立动作存在更早期限时才缩短等待。超时后
不机械调用 `list_agents` 或读取全量历史；按新通知、逾期里程碑、缺失终态证据或显式错误
进行目标明确的查询，否则继续等待。每条通知都需消费并核对尚未完成目标，不以减少查询为由
忽略失败。`wait` 返回 `tasks`、`messagesReady`、`timedOut`，不返回事件批次。消息就绪、
工具任务完成和 progress 都不是 child 的完成证据。

宿主将持久 child 通知作为 Thread 消息送入父模型上下文，字段为 `childId`、`commitSequence`、
`turn`、`lifecycle`、`progress`。父模型只在 `childId` 绑定真实 spawn 目标，且
`turn.state.kind = finished`、`turn.state.value = completed | toolCompleted` 时记录该
`turn.turnId` 为成功完成。`list_agents` 的目标行 `lastTurn`、`pendingInputs` 与
`runningTasks` 可用于核实；未出现在一次 wait 结果中的目标不能被推定完成。每个 pending
目标都必须取得自己的当前完成证据。`turn.state.value = stepLimit` 是预算暂停，
`waitingInteraction` 是等待交互，两者都不是成功交付：先读取该 child 的 durable Timeline
判断进展，健康且工作未完成时才发送明确 continuation；保留新消息准入收据，等待新观察到的
Turn 身份，不构造不存在的收据。父模型必须先消费匹配目标与 Turn 的成功终态通知，之后到达
的通知不能使更早的阶段推进变为有效；checkpoint 请求之后才到达的通知不能倒推授权。

### canonical submissions

每个目标都获得自己的当前成功终态后，才按绑定 agentId 读取 canonical 非空
`read_agent_submissions`。只有 `CHILD_DELIVERY_READY` 的 progress 仍需等待成功终态；
`stage = readyForCompletion` 不改变 Turn 生命周期，也不能清除 pending 或提前请求
checkpoint。canonical page 必须非空且完整，大提交按返回的 `nextCursor` 和 `fragment` 完整
读取，旧提交不可重复消费。

提交页增加 `targetState`，其 `agentId`、`throughSequence`、`turn`、`completion`、
`guidance` 均来自与 cursor 相同的冻结 history。`completion` 为穷尽值：`notStarted |
running | completed | toolCompleted | waitingInteraction | stepLimit | cancelled |
interrupted | failed`。`running/notStarted` 页即使非空或含完成 marker，也必须保留 pending，
按 guidance 等待并消费匹配目标与 Turn 的成功终态通知，再不带旧 cursor 重新读取——冻结的
Running 页不能靠翻页更新成当前终态。`completed/toolCompleted` 仅补充核实目标状态，不替代
父模型消费终态通知的要求；其他状态按交互、预算或失败处理。正常顺序仍为先终态再交付查询，
不因此新增提交轮询。取消、中断、失败或空提交进入诊断、收窄重派或显式关闭；
`read_agent_session` 不能替代正常 durable submission——它读取持久化可见 Timeline（默认
倒序最新 20 条文本 Item，可翻页、切换正序或请求完整 typed Item），查询在驻留 child 上先
等待目标 revision 耐久化，也能读取已关闭、淘汰或重启后未驻留的 child；它不激活目标、
不修改事件总线。空页进入诊断和收窄重派。

### 整合与 reviewer 门禁

Task 默认在 working 后进入 integrating。directory 成果由 root 检查组合 diff 并形成最终
提交；worktree 成果由 root 审查 commit、用普通 Git 显式整合；执行者和 worktree 保留至最终
审查与验证通过后才 cleanup。同一并行批次包含多个 worktree child 时，root 必须先审查并
整合该批次全部接受的 commit，第二次及后续整合全部成功后才能发出第一次 cleanup；随后再
逐个 cleanup 并验证对应 branch/worktree 消失，不得按 child 交错执行"整合一个、清理一个"。
root 可在解决冲突时完成保持合并语义所需的相邻实现和测试修复，但不得借机展开无关重构。
合适 child 不可用或失败时，root 先等待容量并收窄重派一次；仍失败才允许最小实现兜底，并
在交付中记录 `ROOT_IMPLEMENTATION_FALLBACK`、原因和直接修改文件。

参数或合同错误不得原样重放：root 先按工具 schema 修正 camelCase 参数、模式专属字段和目标
id，再用新的调用重试一次；容量或 provider 暂时失败则等待后收窄重派。刻意验证 directory
边界的拒绝必须标记为 expected rejection，禁止绕过，也不计入非预期首次调用失败。

所有成果整合后必须创建 fresh-context 的只读 `reviewer`，综合检查目标、设计、完整 diff、
错误路径、测试、冲突和 fallback。reviewer 不直接修复：代码 finding 回到 working 交给
executor，设计 finding 回到 editing_documents 由 root 修订；重新整合后必须再派新的
reviewer。reviewer 在最终回复前调用 `report_progress` 形成 durable verdict（finding 或
approval）；root 必须通过 `read_agent_submissions` 读取与冻结 reviewer agentId、读取 call
ID 绑定的 canonical page。该协作报告是只读审查的结构化交付，不允许文件/Git/exec 修复，
也不能用 root 转述或 session 摘要伪造 approval。reviewer 必须明确批准或阻塞 finding，不能
同时批准和要求先完成必要检查；root 自审不能替代 reviewer，必须等待该 wave 的每个
reviewer terminal 并按 agentId 读取 durable verdict，只有全部 approval 才能进入最终门禁，
任一阻塞 finding 都必须返工、重新整合并创建新的 review wave。范围较广或风险面可独立验收
时，同一 review wave 还应在首次等待前并行派出分别覆盖 API/错误路径、测试、GUI、Git/整合
等专项 reviewer。

上述职责由本 Turn 的 Mode/Profile 指令约束，不新增专用 executor/reviewer runtime 生命
周期，也不按阶段裁剪普通工具能力。

### 原执行者返工与验证证据

root 保存任务读写范围与原执行者 `agentId` 的映射，交付后保持执行者 idle、可续跑，不提前
关闭或清理 worktree。代码 finding 必须先用 `send_message` 续跑原执行者，附 finding、当前
整合基线、修复范围和验证记录；只有执行者不可用或原权限无法覆盖修复时才重派，并记录具体
原因。设计 finding 仍由 root 修订。worktree 返工前由 root 协调同步 canonical 基线；只采纳
本轮新增修复提交，不重复整合旧提交。每次续跑都重新建立 pending 集合，使用本轮消息之后的
terminal receipt 和 durable submission；旧轮次完成状态与旧交付不能证明返工完成。重新整合
后创建 fresh-context reviewer，全部 approval 且最终验证通过后才关闭执行者、清理 worktree；
停止或失败保留未交付现场并报告原因。

执行者、reviewer 的 durable submission 和最终回复都包含验证记录：实际执行者、完整命令、
工作目录、代码基线（commit 加相关未提交 diff 或文件内容身份）、覆盖范围、环境、结果和
工具/日志证据。明确区分本次实际执行、引用已有证据、尚未验证；没有执行测试也必须说明
原因，阅读测试代码不算执行测试。root 向后续 child 传递已确认的记录，并在最终交付逐项
汇总。执行身份以成功 spawn 回执中的 Pure agentId 为准；child 无法确认时报告角色/范围，
由 root 绑定 ID，不使用环境变量、进程或外层宿主 ID。相同命令、相关代码范围与环境未变且
已有成功证据时复用；修改或依赖变化、冲突、失败诊断、覆盖缺口与强制门禁要求重跑时，报告
具体原因。不同角色不机械重复全量检查，最终整合验证与项目强制门禁仍须满足；reviewer 不为
补测试越过只读工具边界。验证记录使用现有 submission detail，不新增生产协议、持久化字段或
GUI 接口。复用会话增加上下文及 provider prompt cache 复用机会，缓存命中只按上游实际
usage 报告，不承诺固定收益。阶段完成标准消费可复用的验证记录，只补缺失或失效的检查，
不为阶段切换机械重复全量测试。

用户要求的验证未完成时不得宣称 completed；同一基础设施故障确认后不派 child 试探同一
物理能力。以上均是提示词约束，不新增运行时完成硬门禁。

## 12.5 worktree 生命周期

本地和 SSH 后端都以 spawn 时解析的 `HEAD` 执行 `git worktree add -b`，禁用 hooks 和
credential helper，最长 120 秒。路径为 `<repo>/.anywork/worktrees/<root-thread-id>/<child-id>`，
分支使用 Pure-owned `pure-agent-*` 名称。非 Git 项目或无 HEAD 时类型化失败。

`studio_objects` 保存版本化 lease：`prepared | active | preserved | cleanupRequested |
cleaned`，以及 repo、path、branch、base 与 revision。spawn 任一阶段失败都按
`NoSideEffects | MayHaveCreated` 分类补偿 Thread、热资源、worktree 与 branch。启动恢复只按
durable lease 对账；资源部分缺失或身份不匹配时保留现场并发布 Recovery issue，不盲删目录或
非 Pure 分支。

`close_agent` 对 worktree child 接受 `workspaceDisposition = preserve | cleanup`，默认
preserve。关闭工具等待子孙 Thread、订阅与所选宿主资源处置完成，成功结果包含目标及
`lifecycle.kind = closed`；成功回执同时报告实际 workspaceDisposition。它不通过目录事件报告
完成；父 inbox 的 core Closed 通知可能早于物理资源清理，不能代替 `close_agent` 成功回执与
路径/分支复核。若工具执行仍是 pending task acknowledgement，须等待该任务的最终结果。清理
失败保留关闭状态、所选 disposition 与可恢复错误，显式重试不能使已取消的会话重新执行。
关闭不自动 commit、merge、cherry-pick 或修改主分支；父 Agent 应先审查 child commit、用
普通 Git 显式整合，最终审查与验证通过后再请求 cleanup。已经 preserved 的 lease 在
Agents/Recovery 中显示 revision、branch、base/head、dirty 与 changed-files 预览，并提供
显式清理。物理 worktree 清理尊重 Git 锁与注册身份拒绝：注销失败且目录仍存在时，不继续
绕过 Git 删除目录或分支；保留现场并返回实际错误，显式解除原因后可重试。

## 12.6 GUI

Agents 是 canonical Agent 配置中心，不保留重复 Roles 设置页。系统卡片显示固定模式徽标、
启用开关、provider/model/effort 控件，用户编辑器额外显示三模式选择。所有设置 mutation
携带 `expectedSettingsRevision`，成功后以返回的完整 canonical settings snapshot 原子刷新
UI（设置页整体契约见 [19](./19-studio-ui.md)）。
