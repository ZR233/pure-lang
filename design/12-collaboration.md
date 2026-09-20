# 12 - Agent Profile 与协作编排

本文是 Profile 体系、child 派发、durable delivery、wait 语义、reviewer 门禁与返工合同的唯一
权威源；权限与工作区安全语义见 [04](./04-security.md)，通用工具运行时见
[09](./09-tool-runtime.md)，Task Mode 图与阶段见 [11](./11-thread-mode.md)。

## 12.1 边界与工作区模式

Studio 的 child Agent 使用与 root 相同的 Thread/Turn/Tool 框架。Profile 冻结模型路由与工作区
模式；父 Agent 负责拆分工作、避免冲突、审查成果，并用普通 Git 显式整合 worktree child 的
commit，不存在自动 merge 或 delivery gate。

根会话拥有会话级工作区模式 `ThreadWorkspaceMode`：`local`（默认）使用 Project 根目录，
`worktree` 从 Project 的 Git 仓库 `HEAD` 派生独立 checkout 并在其中工作。它是 Thread 的
canonical 产品事实，在创建会话时确定，之后不随配置或运行状态变化。它与 Profile 的 workspace
mode 是两条语义轴：会话模式决定工作区在哪里，Profile 模式决定 child 相对会话工作区的隔离
方式。根会话 worktree 的创建、绑定、恢复与清理见 12.5。

会话对外只有一个 canonical 工作区地址 `workspace_path`，与 `workspace_mode` 同层、在创建
会话时写定、之后只读：`local` 会话的地址是 canonical Project 目录，`worktree` 会话的地址
是该会话的工作树路径。`workspace_mode` 只是地址的类型标记；侧栏、会话顶栏与 GUI 状态不按
模式分别取路径，也不自行推导或拼接工作树布局。子线程继承所属根会话的地址与模式。地址与
durable lease 的职责边界见 12.5，产品投影见 [18](./18-studio-state.md) §18.2，持久化与迁移
见 [17](./17-studio-storage.md) §17.1。

child Profile 的工作区有三种模式，其中的 root 指该 child 所属根会话的工作区根：

- `unrestricted`：Profile 不增加额外项目隔离，root 就是会话工作区根（根会话为 `worktree`
  时即在该 worktree 内）；项目内外仍遵循会话 Permission Mode。
- `directory`：root 仍是会话工作区根；`writablePaths` 只限制 Pure 内置文件 mutation 工具在
  其中的写入。它不是 OS 沙箱，shell、Git 与 MCP 可以绕过，工具描述、child 固定上下文和
  GUI 必须共同提示该边界。
- `worktree`：root 是独立 Git worktree，boundary 为 Confined，worktree 内全可写；base 仍是
  Project 仓库的 `HEAD`，不是父会话 worktree；主工作区未提交内容不复制过去，成果不会自动
  合并。

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

主智能体（Main agent）是 root，负责理解需求、制定计划、协调子代理、整合与验证结果。
它使用 `planner` 模型路由，不是可派发 Profile，不提供启用开关。
Studio 注册四个系统子代理 Profile：`explorer`、`reviewer` 固定为 unrestricted，
`executor` 固定为 directory，`worktree_executor` 固定为 worktree。系统 id、名称、用途、指令
和模式不可编辑；Agents 设置页可以配置其启用状态、provider/model 和模型声明的 effort。
`planner` 是保留标识，用户 Profile 不可占用；目录查询和 spawn 均不接受它。
历史 planner child 保留身份、消息和执行记录，仅供查看；恢复或续跑在资源创建前拒绝，
界面显示“该子代理角色已停用”。历史订阅直接重放日志并发布只读快照，不装配运行资源；
后续等待随订阅取消而释放。该限制仅针对 child，不影响使用同一路由的 root。

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
child 不装配 `plan_*` 工具；方案与问题通过 Turn 报告交给 root。配置冻结与 Plan session
隔离是两条独立边界。`spawn_agent.message` 和 root 后续通过 `send_message` 发送的补充输入
都在 child Timeline 中显示为 `parentAgent` 文本消息，并由 Studio 标记为"主智能体 / Main
agent"；`send_message` 对运行中 child 请求打断并在清理后携带新内容自动继续，对空闲 child 启动执行。
它复用用户 prompt 的 owner 控制入口，`interrupt_agent` 则仅停止，不自动续跑。消息收据只表示受理。它们对 provider 仍是普通 user role，不得改变 fork、Plan 隔离、预算刷新或
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
优先拆成多个 `explorer` 并行执行；主智能体综合事实、比较方案并决定计划。探索 child 默认使用
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
- 会触及共同接口、manifest、lockfile、生成文件或高风险 Git 状态的任务使用
  `worktree_executor`。每个 child 在独立 worktree 提交，经局部审查通过后由 root 串行合入当前工作空间；worktree 不能
  替代真实前后依赖的顺序执行。对于任务要求创建的新文件，child 必须先用文件工具创建并以
  只读工具确认精确路径和内容，之后才能执行引用该路径的 `git add` 或 `git commit`，不得用
  试探性暂存验证文件是否存在。状态检查、文件创建、内容确认、测试、暂存、提交和提交复核
  是独立步骤，不得用 `&&`、`||`、`;` 或 pipeline 拼成一个 exec，保证任一步失败都保留准确的
  首次调用责任和可重试边界。

平凡单文件任务保留所有阶段、Plan 批准和独立 reviewer；计划目标不超过八行，不机械展开
DAG 或验证表，无独立探索价值时不派 explorer。root 可以承担协调成本高于工作量的写入；无
文档或整合变更的阶段复用已有事实简短说明；阶段切换仍按新鲜查询和 Solo mutation 执行。

### ready frontier 调度

planning 开始和每个 child 交付后，root 都执行一次成本感知的并行化分析：从需求、仓库边界
和验证目标识别可交付节点；仅当多个实质交付存在依赖时，为合格节点记录前置依赖、读写
范围、适用 Profile、交付证据与 root-only 标记，形成任务 DAG。依赖已满足、边界清楚、可
独立验收且预计能缩短关键路径或显著增加独立证据的节点构成 ready frontier；root 按容量
派发就绪任务，等待期间继续处理未委托工作，不得重复 child 的任务。每收到一个执行者的
本轮终态报告，root 即核对证据、安排局部审查并更新依赖；满足依赖的
后续任务立即释放，不等待整批完成。容量不足时将就绪审查排队，不用整批屏障代替容量调度。
调度不使用固定 agent 数量。

### Turn 报告与收据

主代理统一承担用户澄清和 Plan 确认。child 不装配 plan_* 或 request_user_input；权限审批仍由
运行时处理。父消息提供已批准基线，child 遇到问题时结束本轮汇报，由父代理决定续跑。

finish_turn({message}) 提交完整非空正文并结束 Turn；自然 final 同样结束本轮，不需要再调用
工具或重复汇报。派发、续跑和汇报正文不设应用层长度上限；模型容量和存储背压仍按各自合同
处理，不能静默截断。正文说明目标、事实、架构、接口形状、错误语义、依赖步骤、复杂流程
伪代码、所有权和验收。契约明确、局部自主，事实和建议必须区分。

TurnState 是唯一执行终态；报告由对应终态水位的 journal 投影，不拥有独立状态机、revision、
registry 或就绪 marker。报告包含 child 身份、Turn/input 身份、触发及本轮消费消息的身份与序号、终态序号、实际停止原因、完整
正文、未完成后台任务和交互，以及本轮实际权限审批记录（含已完成审批）。正常结束不等于任务完成，父代理结合目标和证据判断。
强制停止没有模型总结时由运行时报告实际原因及本轮已提交可见输出，不追加模型请求、不借用
旧轮总结、不转发隐藏 reasoning。步数耗尽保留现场，由父代理决定继续，不自动续跑。

spawn/send 收据仅表示消息受理。父代理绑定真实 agentId 和本轮 input/Turn 身份，不能把旧轮
报告用作新任务交付。消息序号和 journal 序号属于不同域。

### 通知、等待与恢复

终态提交后，完整报告以稳定 child/终态 commit 消息身份写入父 Thread inbox，重试幂等；固定终态水位
禁止混入后续 Turn 输出。wait 只等待就绪，消息通过下一模型请求进入上下文，不再另外查询提交
或返回重复报告。空 taskIds 等待子代理消息，工具 taskIds 仅引用工具任务。正常超时不机械轮询。

显式恢复时核对已保存 child 终态与父 inbox 并补齐缺失通知；历史查询不激活模型。Turn 结束
不关闭 Thread、不取消跨 Turn 后台任务、不清理 worktree。read_agent_session 保留为诊断与
历史查询，不是正常交付必经步骤。旧进度与通知仅在迁移边界转换，保留原始内容、身份和顺序，
不得把 readyForCompletion 推断成成功执行。

### 独立审查与增量合入

working 内各实现任务独立推进编码、局部验证、静态审查和修复。root 收到单个执行者的本轮
完成证据后即派 fresh-context reviewer，不等待其他执行者；原执行者在该轮审查期间暂停修改
被审范围，其他互斥范围可继续工作。执行者只做自身改动范围的格式检查、静态检查、单元测试
和必要定向回归，不运行全仓格式化、全量测试或最终集成门禁。无法定向执行的检查交 root；
若目标项目强制每次提交前全量验证，派 directory executor，由 root 统一验证后按授权提交，
不能让 worktree executor 跳过项目规则。

每次审查消息包含当前已批准的完整实施计划及已确认调整、该任务范围与验收要求、执行者
身份、实际工作区、基线、差异和局部验证证据。reviewer 逐项核对负责范围是否完成计划、
是否偏离设计以及代码缺陷，只做静态审查，不执行 shell 或测试。未完成的其他独立任务不算
当前任务遗漏，但影响本任务正确性的真实依赖必须明确。缺少完整计划或必要源码证据时补齐
后才能批准。计划明确留给 root 的最终验收尚未执行，不单独阻塞局部静态审查。

审查结论绑定 worktree commit 或 directory 差异及文件内容身份。reviewer 读取被审执行者
的实际工作区，不限定为 root 工作区；工具不能访问时，root 提供完整可核对的源码和差异，
不能凭摘要批准。报告明确批准或阻塞问题并附证据，不得同时批准又要求先补齐审查必要证据。
root 消费对应 reviewer 本轮终态报告，不解析字符串 marker，不另读 submission。

- directory 成果已在当前工作空间。审查通过、局部验证和前置条件满足后标记该任务完成，
  不执行 cherry-pick、merge 或模拟合入，也不顺带提交其他未完成或未审查的本地改动。
- worktree 成果审查通过且依赖满足后，root 即用普通 Git 将已批准提交合入当前工作空间，
  不等待其他执行者。主工作区 Git 操作串行；合入成功后才标记任务完成并释放依赖。
- 冲突解决、后续修改或依赖变化影响已审范围时，补充该范围的验证和 fresh-context 复审；
  无关变化不使全部审查失效。不得丢弃其他所有者的改动或借冲突处理扩大为无关重构。

代码 finding 由 root 发回原执行者；修复后先局部复审，再按工作区类型完成或合入。设计问题
由 root 在继续相关编码前同步文档；需要调整计划时遵循既有确认流程，不用修改计划掩盖偏离。
各任务循环互不阻塞，所有执行者及 worktree 保留至最终验收通过，不能把增量合入当作 cleanup。

全部本地任务完成、全部工作树任务完成并合入后，integrating 核对交付完整性和审查版本；
没有 worktree 就不制造合入操作。reviewing 由 root 对照完整计划进行跨任务静态审查、适用的
全量检查、集成与功能验收，逐项对应要求、实现和实际证据。不强制再派全量 reviewer，局部
审查批准也不能代替整体验收。最终验收发现代码问题时回 working，由原执行者局部修复、复审，
worktree 修复再次合入，directory 修复直接留在当前工作空间；设计问题回 editing_documents。
仅环境故障则处理环境并重跑受阻检查，不机械触发代码返工。所有必要验收通过后关闭子代理，
清理已接受的 worktree 并核对资源回收，再进入 completed；失败或取消保留未交付现场。

合适 child 不可用或失败时，root 先等待容量并收窄重派一次；仍失败才允许最小实现兜底，并
在交付中记录 `ROOT_IMPLEMENTATION_FALLBACK`、原因和直接修改范围。参数错误按实际 schema
修正后再调用，不能重复相同错误；刻意验证目录边界的拒绝记为 expected rejection，不能绕过。
上述职责是 Mode/Profile 提示词合同，不新增运行时子任务状态机或工具能力裁剪。

### 原执行者返工与验证证据

root 保存任务读写范围与原执行者 `agentId` 的映射，交付后保持执行者 idle、可续跑，不提前
关闭或清理 worktree。代码 finding 必须先用 `send_message` 续跑原执行者，附 finding、当前
整合基线、修复范围和验证记录；只有执行者不可用或原权限无法覆盖修复时才重派，并记录具体
原因。设计 finding 仍由 root 修订。worktree 返工前由 root 协调核对最新 canonical 基线，仅在依赖或冲突需要时同步；只采纳
本轮新增修复提交，不重复整合旧提交。每次续跑都重新建立 pending 集合，使用本轮消息之后的
终态报告；旧轮次完成状态与旧交付不能证明返工完成。修复后创建 fresh-context reviewer，
局部批准后本地任务直接完成，worktree 新增提交由 root 合入；最终验收通过后才关闭执行者、清理 worktree；
停止或失败保留未交付现场并报告原因。

执行者、reviewer 的本轮报告包含验证记录：实际执行者、完整命令、
工作目录、代码基线（commit 加相关未提交 diff 或文件内容身份）、覆盖范围、环境、结果和
工具/日志证据。明确区分本次实际执行、引用已有证据、尚未验证；没有执行测试也必须说明
原因，阅读测试代码不算执行测试。root 向后续 child 传递已确认的记录，并在最终交付逐项
汇总。执行身份以成功 spawn 回执中的 Pure agentId 为准；child 无法确认时报告角色/范围，
由 root 绑定 ID，不使用环境变量、进程或外层宿主 ID。相同命令、相关代码范围与环境未变且
已有成功证据时复用；修改或依赖变化、冲突、失败诊断、覆盖缺口与强制门禁要求重跑时，报告
具体原因。执行者只做局部验证，root 承担最终整合验证与项目强制门禁；reviewer 不为
补测试越过只读工具边界。验证记录使用报告正文，不新增生产协议、持久化字段或
GUI 接口。复用会话增加上下文及 provider prompt cache 复用机会，缓存命中只按上游实际
usage 报告，不承诺固定收益。阶段完成标准消费可复用的验证记录，只补缺失或失效的检查，
不为阶段切换机械重复全量测试。

用户要求的验证未完成时不得宣称 completed；同一基础设施故障确认后不派 child 试探同一
物理能力。以上均是提示词约束，不新增运行时完成硬门禁。

## 12.5 worktree 生命周期

本地和 SSH 后端都以创建时解析的 `HEAD` 执行 `git worktree add -b`，禁用 hooks 和
credential helper，最长 120 秒。两者都以本次解析出的仓库根作为 worktree backend 根：远端
项目的 workspace handle 根、Git 工作目录与相对路径基准都取自该仓库根，而不是配置的 Project
目录，因此 Project 目录是仓库子目录时同样成立；远端 Git 与目录操作仍由本地编排，经远端进程
面与文件面在远端执行，helper 内不实现 Git/worktree。创建路径与恢复、preview 和清理共用该
约定。lease 记录归属：child 智能体使用
`<repo>/.anywork/worktrees/<root-thread-id>/<child-id>` 与 Pure-owned `pure-agent-*` 分支；
根会话自身工作区使用 `<repo>/.anywork/worktrees/<root-thread-id>/session` 与 Pure-owned
`pure-session-*` 分支。身份校验按归属校验期望 leaf 与期望分支，仍拒绝任何非 Pure 分支。
非 Git 项目或无 HEAD 时类型化失败。

远端后端的 `git worktree add`/`remove` 路径参数、workspace 打开参数与 lease 记录使用同一
POSIX 形式：客户端宿主形态（含 Windows 路径分隔符）不得跨端，否则物理 worktree 会落在与
lease 记录不一致的位置。远端绝对路径的唯一规范化边界是
`pl-tool::remote::normalize_remote_absolute_path`：它把反斜杠转换为 `/`、折叠重复分隔符与
`.`，并拒绝空值、相对路径和 `..`；Studio、worktree backend 与恢复代码不得维护第二套路径
解释（跨端路径约定见 [22](./22-ssh-remote.md) §22.3）。

根会话 worktree 对本地与 SSH 项目都可用，并在创建会话的命令内建立：先按 `HEAD` 解析仓库与
base、记录含 `ssh_alias` 的 `prepared` lease，再创建物理 worktree，随后转为 `active`，然后
发布含 `workspaceMode` 的 Thread 目录事实，最后激活 owner 并把该路径绑定为会话工作区根。
远端会话的工作区根与 canonical Project 目录分离，工具层按会话工作区根打开远端 workspace
handle。任一阶段失败都让命令失败、不发布 Thread，并按 `NoSideEffects | MayHaveCreated`
收束；已创建资源保留现场，不用 `--force` 绕过 Git 锁或注册身份。崩溃可能留下已落库 lease
而没有 Thread 记录，启动对账把这类 lease 作为诊断保留。

根会话激活只按 Thread 的 `workspaceMode` 与 durable lease 解析工作区：`local` 使用 Project
根目录；`worktree` 必须存在 identity 匹配的 `active` lease，缺失、身份不符或已经清理都返回
类型化失败并发布带 revision、branch/base/head 与 path 的 Recovery，不静默回落到主工作区。
归档清理该会话树在 worktree 模式下拥有的物理工作树：会话自身 lease 与树内 child lease 都按
`cleanupRequested → discard → cleaned` 收束，Pure-owned 分支一并删除，身份校验规则不变；
任一步失败回落 `preserved` 并发布 Recovery，归档本身仍完成，不静默留下既不确定又无处处置
的资源。归档因此是破坏性动作：工作树中未提交的改动与未整合的提交一并删除，确认入口见
[concepts/project-sidebar.md](./concepts/project-sidebar.md)。

非归档的关闭路径不删除会话自身 worktree；child worktree 仍按关闭路径的 disposition 处置。

归档不改变 Thread 身份：恢复把 `archived` 翻转回去，并在原路径重建已清理的工作树——会话自身
与树内 child 各按同一归属重新派生路径并准备 lease——使会话地址在归档与恢复之间保持稳定。
运行期不再存在「Thread 已归档后仍长期持有 worktree」的状态，只在清理失败时保留 `preserved`
现场。

恢复必须让 worktree 会话重新落到可用状态，不能只翻转归档标记后留下无法激活的会话：已清理或
缺失的 lease 在原路径重建；归档清理失败留下的 `preserved` 现场在身份匹配且物理工作树仍然
存在时重新绑定为 `active`（并退掉对应的 Recovery 条目），否则同样在原路径重建；两条路都失败
时返回类型化失败并保留现场。恢复完成后该会话的 lease 必须是 `active`，否则恢复本身失败。

Recovery 的 worktree preview 与显式清理只作用于不再由活动 owner 使用的资源：保留
（`preserved`）的 lease、没有已注册 Thread 的孤儿 lease、归档清理失败而保留的现场，以及
身份或物理资源缺失、不匹配的现场。健康 `active`（以及创建中的 `prepared`）lease 不进入清理
入口，也不显示为待清理项；显式清理命令必须在服务端复核同一前置条件，陈旧 GUI 卡片不能删除
在用 worktree。创建会话失败当场收束为 `preserved` 的会话 worktree 必须立即发布带归属、状态、
revision 与 preview 的 Recovery 条目，不依赖下次启动审计才可见；该条目覆盖同一 Thread 的旧
诊断，不留下来源已保留却无处清理的资源。

「创建中」与「崩溃遗留」以进程内显式标记区分：标记存续期间该 lease 既不发布也不可清理（即使
owner Thread 记录尚未发布）；进程重启后标记消失，遗留 lease 按需要人工处置的资源处理。所有
运行期把会话 worktree 收束为 `preserved` 的路径，包括创建阶段的失败收束与身份不符收束，都
必须在返回错误前完成发布。

`studio_objects` 保存版本化 lease：归属类型、owner Thread id、
`prepared | active | preserved | cleanupRequested | cleaned`，以及 repo、path、branch、base
与 revision。spawn 或创建会话的任一阶段失败都按
`NoSideEffects | MayHaveCreated` 分类补偿 Thread、热资源、worktree 与 branch。启动恢复按
durable lease 对账，本地与远端 lease 同样处理：远端 lease 的预览与显式清理在连接可用时经
远端 backend，SSH 离线时保留现场并给出诊断。除 lease 外，启动期只对本地项目额外扫描文件
系统中的未注册 Pure-owned worktree，不为远端项目打开 SSH 连接，远端资源完全由 durable
lease 覆盖。资源部分缺失或身份不匹配时保留现场并发布 Recovery issue，不盲删目录或非 Pure
分支。

`close_agent` 对 worktree child 接受 `workspaceDisposition = preserve | cleanup`，默认
preserve。关闭工具等待子孙 Thread、订阅与所选宿主资源处置完成，成功结果包含目标及
`lifecycle.kind = closed`；成功回执同时报告实际 workspaceDisposition。它不通过目录事件报告
完成；父 inbox 的 core Closed 通知可能早于物理资源清理，不能代替 `close_agent` 成功回执与
路径/分支复核。若工具执行仍是 pending task acknowledgement，须等待该任务的最终结果。清理
失败保留关闭状态、所选 disposition 与可恢复错误，显式重试不能使已取消的会话重新执行。
关闭不自动 commit、merge、cherry-pick 或修改主分支；父 Agent 应先审查 child commit、用
普通 Git 显式整合，最终审查与验证通过后再请求 cleanup。已经 preserved 的 lease，不论归属
child 智能体还是根会话，都在 Agents/Recovery 中显示 revision、branch、base/head、dirty 与
changed-files 预览，并提供显式清理；两种归属共用同一清理入口，按归属标识区分。物理
worktree 清理尊重 Git 锁与注册身份拒绝：注销失败且目录仍存在时，不继续
绕过 Git 删除目录或分支；保留现场并返回实际错误，显式解除原因后可重试。

## 12.6 GUI

Agents 是 canonical Agent 配置中心，不保留重复 Roles 设置页。系统卡片显示固定模式徽标、
启用开关、provider/model/effort 控件，用户编辑器额外显示三模式选择。所有设置 mutation
携带 `expectedSettingsRevision`，成功后以返回的完整 canonical settings snapshot 原子刷新
UI（设置页整体契约见 [19](./19-studio-ui.md)）。

显式 send_message 可按已保存的直接父子身份激活 cold child，沿用原 agentId、现场和上下文；
list_agents 同时投影热、冷历史成员。恢复观察可补写缺失 inbox，但历史观察不唤醒模型；
新的终态才主动唤醒父代理，显式输入/续跑负责消费恢复的待处理消息。已保存消息身份及消费水位
去重，不因报告投影升级重送旧通知。保留 worktree 的恢复提示不阻断根任务续跑。

根任务接受显式新输入时，对未加载子代理的保存 journal 做只读终态对账，补齐父 inbox；
无需打开子代理工作区或调用模型。仅打开历史页面不触发这一步或恢复执行。
