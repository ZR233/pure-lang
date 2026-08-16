# 工具调用运行时

## 目标

工具调用运行时负责把模型返回的 tool call 转换为本地工具执行、审批、Studio toolCall Item
与下一轮模型输入。它必须保证同一个工具调用身份稳定、终态唯一、失败结果可回传给模型，
并在 Studio 中保留用户可读的错误原因。

## 身份字段

每个工具调用携带必填的 typed `ToolCallIdentity { item_id, call_id }`：`item_id` 是 provider
返回的工具调用 item id，`call_id` 是跨协议回放的 canonical 调用 id。两者在协议解码边界一次
性确定——Responses 使用事件携带的 `item.id` 与 `call_id`；Chat Completions 解码时确定性赋
`call_id = item_id`。不存在 optional `call_id`、`stable_call_id()` 回落或 id 与 call_id 互为
fallback 的规范化路径；任一身份缺失都是 provider 协议错误。

Chat Completions 历史回放时，assistant 消息中的 `tool_calls[].id` 与 tool 消息的
`tool_call_id` 使用 `item_id`；Responses 回放中 `function_call_output` 与
`custom_tool_call_output` 使用 `call_id`。同一 Thread 内 Chat 与 Responses 可以互相接力回放，
typed identity 保证两种 wire 都能重建完整工具历史，不出现
"missing call_id for Responses history replay" 一类的协议缺口。

Core 会话保存 typed 工具 transcript：assistant 侧的工具调用集合与 tool 侧的结果都是 typed
记录（identity、工具名、原始参数、`function`/`custom` 种类），不再使用字符串化 JSON blob
或散落的 metadata key。`tool_call_kind` 等字段缺失或未知时 provider request 构造一律返回
协议错误——运行期不保留任何缺字段兼容路径，也不能静默回退到 function。

## 工具来源与代际发布

工具注册表是唯一抽象：builtin、host、MCP、LSP 都是它的客户，一律以工具条目发布能力；
core 不存在按来源的分支或按名称前缀的规则。来源用 opaque typed `ToolSourceId` 标识
（如 `builtin`、`host`、`mcp`、`lsp`），新增来源或语言不需要修改 core。

每个来源通过 `publish(source_id, entries)` 整组发布：发布方在注册表之外构建完整的下一代
条目集合，注册表校验命名空间所有权与跨源名称冲突后原子替换该来源全部工具并递增全局
`RegistryRevision`；构建或校验失败时旧一代原样保留，绝无半代可见。发布返回的 RAII guard
被 drop 时该来源工具整组注销。确定性公共名由纯函数生成（如
`mcp__<本地 server id>__<raw name>`，有损归一化时追加稳定哈希后缀），来源身份取本地配置
而非远端自报名；注册表维护名称所有权表，任何来源不得抢占其他来源的名称。

外部来源有两种接入模式，对注册表完全同构：

- 逐工具直通：来源定义自己的工具面（如 MCP server 的远端 tools 与 resource façade），
  schema 原样透传，安全属性由 typed 元数据声明。
- 能力 seam：来源是能力池（如 LSP），模型面是 harness 策划的稳定契约工具
  （`lsp_capabilities`、`lsp_query`），server 不控制模型可见 schema。

每个 Turn 开始时通过 `acquire_turn_lease` 冻结 `TurnToolLease`：canonical 化并排序后的
模型可见 schema 集合、Tool Search catalog、执行策略视图与 executor（工具条目的共享句柄）。
排序 canonical 化只在 lease 构造处发生一次，注册时序不影响请求字节。活动 Turn 持有的
lease 不受后续 publish 影响——已启动调用继续使用旧代 executor，新 Turn 获取新代。注册表
变化通过 change 通知广播，Turn 准备时重新读取，不做事件驱动的 lease 失效。

Studio toolCall Item id 使用最早稳定的 provider item id 或 runtime tool id 作为相关性锚点，
再按 Turn 做命名空间隔离；provider 没有 item id 时才回退到 `ToolCall.call_id` 或本地 id。
后续才出现的新身份只作为 correlation alias，继续更新同一个 Item。core `TracePart` 只作为运行时
诊断输入，由 ThreadActor 转换成 typed `ItemStarted`、`ItemDelta` 和 `ItemCompleted`；它不是
Studio 的第二套持久化模型。

## 模型流完成语义

`pl-model` 只有在收到 provider-independent `StreamEvent::Completed` 后，才能把一次模型调用视为成功。SSE parse error、transport error 或 EOF-before-completed 都必须返回稳定 `PureError`，并让 turn 失败，不能把已累计的局部内容当作成功 assistant 响应写入历史。`Completed` 或 `Failed` 是单次 provider stream 的终态；终态之后到达的 text、reasoning summary、raw reasoning、tool 或 usage 事件都是 provider 协议错误，不得继续追加到 response 内容、trace part 或 Studio timeline。

Chat Completions provider 如果没有 Responses 风格的 completed event，protocol mapper 必须在终止 chunk 合成 `StreamEvent::Completed`。聚合层允许在缺少显式 tool complete event 时兜底完成已聚合工具调用，但该工具调用必须已经有稳定工具名，以及 `id` 或 `call_id`。缺少工具名的残留 tool accumulator 表示 provider/protocol 可恢复错误，不得执行空工具名。

## 生命周期

每个工具调用先建立一个稳定的 toolCall Item，并发送 `ItemStarted`。随后运行时执行：

1. 检查当前模式是否允许该工具。
2. 查找工具注册表。
3. 对声明路径语义的工具输入执行统一路径解析和访问分类。
4. 计算权限策略，必要时请求用户审批或 reviewer 审批。
5. 对批准的工具执行本地实现；对禁用、未知或拒绝的工具直接生成工具结果。
6. 在统一收尾阶段写入完整 authoritative payload，并发送唯一 `ItemCompleted`。

路径类工具不要求模型提供绝对路径。运行时把相对路径按 typed `AgentWorkspace.root` 解析，
规范化为绝对路径后再进入权限判断和实际执行；文件工具、`apply_patch`、`exec.cwd`、`lsp_query`
的 `filePath` 和权限 precheck 必须复用同一个 resolver。`boundary=confined`
拒绝 `..`、Windows drive-relative、越界绝对路径、越界 UNC / verbatim 路径和符号链接越界，
即使会话权限是 `full-access` 也不能逃逸。只有 `boundary=hostPermitted` 且权限策略明确放行时，
本地 backend 才能解析 workspace 外路径。

模型只看到环境无关的 `exec`、`write_stdin`、`read_file`、`list_files` 和 `apply_patch` 等通用入口。模型没有独立的内容搜索文件工具；文本搜索优先通过 `exec` 运行 `rg`，文件名或文件列表搜索优先运行 `rg --files`，当前平台没有 ripgrep 时才使用等价的平台命令。`ToolSetBuilder` 分别接收 `CommandBackend` 与 `WorkspaceFileBackend`；Studio 注入本地实现，Mai 等宿主注入容器或远程实现。PL 统一拥有 schema、权限、进程表、stdin、超时、取消、输出截断和 turn 清理，backend 只负责 cwd 映射、启动/终止进程、发布完整输出和生成宿主 artifact。低层容器复制能力只服务文件 backend 与输出同步，不注册为模型工具。命令工具唯一名称是 `exec`，不注册或运行期改写其他命令工具别名。

`list_files` 的 `glob` 既可以匹配 workspace-relative 路径，也可以匹配 `path` 参数之下的相对条目；`includeDirs=true` 时目录候选按带尾随 `/` 的形式参与匹配。`**/` 表示零层或多层目录，因此 `**/Cargo.toml` 必须同时匹配 workspace 根下和子目录下的 `Cargo.toml`。

主机本地路径统一通过 `pl-core::path_safety` 在 `canonicalize` 前检查；canonical workspace root 是可信边界，根以下的 Unix symbolic link、Windows symlink、junction、mount point 和其他 reparse point 都是不可信路径入口。`stat_path`、`read_file`、写入、创建、patch、复制、移动、删除、LSP 文件参数与 `exec.cwd` 直接命中链接或经链接祖先访问时必须拒绝，即使目标仍在 workspace 内。`exec` 只解析并约束 `cwd`，不分析命令正文的读写行为；`WorkspaceMutability::ReadOnly` 不阻止 shell 启动。因此 shell 技术上可以写 workspace，或在 permission mode 允许时访问 `cwd` 之外的显式路径；角色提示负责约束 Planner、Explorer 和 Reviewer 遵守各自职责。cwd 解析、permission mode、超时、取消和进程回收语义不变。

本地 `list_files` 递归遍历不得跟随链接，也不把链接入口作为普通文件或目录返回。链接不应让 Flutter plugin symlink 或不可访问的挂载目标使整次遍历失败；跳过链接之外的真实目录读取和元数据错误仍须显式失败。递归删除必须使用同一安全分类：目标及祖先为链接时拒绝，子树内链接只解除入口而不访问目标。

MCP runtime 在连接 generation 激活时把该代全部工具与 resource façade 一次性 `publish` 到
注册表（统一来源 `mcp`，按 server 划分 `mcp_<server>` 命名空间），不在每个 Turn 临时安装或卸载。工具 handler 捕获所在
generation 的调用通道、server id 和远端 raw tool name，因而与内置工具共享模式门控、权限与
审批、runtime lock、批次、cache、trace、结果预算、Timeline 和历史配对；PL 不保留 MCP 专用
tool backend 或第二套 dispatch。resource 的 list、template 和 read façade 随来源一起发布与
撤除，handler 直接调用 rmcp typed resource API。generation 退避清理仍等最后一个引用释放；
配置更新只影响下一次 publish，不得改变活动 Turn lease 冻结的连接或工具集合。

rmcp 是唯一 MCP 协议实现。transport 优先通过 `server/discover` 协商 `2026-07-28`，并只在明确的
`METHOD_NOT_FOUND` 后回退传统初始化；Streamable HTTP 若因 startup SSE 终止而只暴露关闭的
discover response，则重建 transport 并使用 `2025-11-25` 标准 `initialize` 兼容现役服务。
`McpConnector` 仅负责构造 rmcp stdio、
Streamable HTTP 或宿主提供的容器 transport，并启动 `RunningService`；`ConnectedMcp` 持有可克隆
`Peer` 与唯一关闭 owner。rmcp 负责协议发现、分页、typed request/result、response cache、MRTR、
请求取消、OAuth、SSE 和连接关闭；PL 只负责跨 server reconcile、generation lease、health、可信
effect 和 Turn 生命周期。最后一个 generation lease 释放后必须显式关闭 rmcp service，Drop 只做
best-effort 兜底。

远端 annotations 解析为 typed 安全提示（readOnlyHint、destructiveHint、openWorldHint），用于
推导该工具的 `ToolEffect` 与并行资格；icons 与其余 meta 只作为展示与审计信息，不参与调度
决策。per-server 配置显式声明的 effect 优先于远端提示；无法推导出可信 effect 的动态工具
默认 `ToolCachePolicy::Never`、独占 runtime lock、不可 programmatic 调用。Simple executor
可以使用 available tools；Task planner、explorer、reviewer 只暴露 effect 策略明确允许的动态
工具，未知 effect 默认拒绝。内置 Zhipu 工具统一声明为 `ToolEffect::Read`。

缓存与工具集合的关系由三类独立指纹表达：

- `RegistryRevision`：注册表全局发布代数，只作诊断，不参与缓存轮换。
- `ToolCatalogFingerprint`：Turn lease 冻结的 deferred catalog（namespace、描述、参数 schema、
  `allowed_callers`、`output_schema`）的 canonical 哈希。catalog 变化只改变后续 Turn 的
  Tool Search 检索结果，不轮换 prompt cache。
- `WirePrefixFingerprint`：本 Turn 实际发送的 eager 工具 schema 集合的 canonical 哈希。只有
  该指纹变化才以 `ToolSchemaChanged` 轮换 prompt generation 并使 Responses WebSocket
  continuation 失效；deferred-only 变化不断链。

工具按模型可见名称和 canonical JSON Schema 确定性排序；运行中的 lease 不允许替换工具集，
也不能为了缓存命中跨 Thread、Workspace 或权限策略复用 lease。工具结果无论成功、失败或拒绝
都只追加到 durable history，不得重排旧输入。Chat Completions 没有延迟加载概念，全部工具
eager 上线，工具集合变化自然反映在 wire 前缀指纹中。

Tool Search 使用客户端执行：初始请求的 tools 只包含 eager 工具与一个 schema 固定的
`tool_search` 函数工具，deferred 工具不出现在请求前缀。模型调用 `tool_search` 时，运行时在
Turn lease 冻结的 catalog 上按确定性检索返回匹配工具的名称、namespace、描述与参数 schema，
加载结果以 Responses 原生 `tool_search_output` item 进入 canonical context，其后模型可直接
发起 function call。`tool_search_call` / `tool_search_output` 是有序 Responses 原生 item，必须
进入 canonical context，并在 HTTP 重放、WebSocket 增量与恢复后完整重建。若模型、provider
profile 或 wire 不支持该机制，Turn lease 中全部可见工具恢复 eager function schema，执行与
权限语义不变；hosted `tool_search` 编排路径不再保留。

Programmatic eligibility 是工具发布时声明的 typed 属性：effect 为 `Read` 且发布方显式声明
programmatic 资格的工具才设置 `allowed_callers: programmatic`，不存在按名称的白名单，也不
接受模型或第三方 annotation 自行提升。WorkspaceWrite、Process、BranchControl、effect 未知、
审批/交互与 agent-control 类工具一律不声明。program 内的每个嵌套调用仍逐个经过注册、模式、
路径和权限校验；runtime 不因为 caller 为 program 绕过任何本地策略。Tool Search 只允许在
program 外层运行。

内置 Zhipu Coding Plan MCP server 优先复用 Zhipu Coding Plan provider 的 `bearer_token`，并兼容回退到普通 Zhipu provider 的 `bearer_token`。缺少 token 时内置 server 处于 `missingCredential`，不参与后台探测，也不应导致普通 turn 或 subagent 启动失败；检测到 token 后进入后台探测流程，只有探测成功的 server 会被主会话和 subagent runner 注册。HTTP 内置 server 在 transport 层直接发送 bearer token；stdio Vision server 在启动进程时注入 `Z_AI_API_KEY` 和 `Z_AI_MODE=ZHIPU`。

每个 Turn 开始时，运行时把实际暴露给模型的工具名保留为内部诊断 trace。它只包含 Turn id、
模式和工具名，不进入模型上下文，也不创建可见 Item；旧 `timeline_events` 表已删除。

`plan_exit` 是 Task planning 阶段专用的内置协调工具，schema 只包含 `content: string`。它表示
“计划已完成，请 Studio 发起确认交互”，不是执行工具；确认后 root Thread 保持 Task，由
TaskService 通过显式 durable input 推进。`<proposed_plan>` 不再是协议入口。

`request_user_input` 在 Studio 中是 durable Turn 边界，Simple root、Task root 和可执行该工具的
child 使用相同语义。工具返回 typed `ToolRuntimeEvent::InteractionRequested`，RunningTurn 把它
转换成 Turn observation，再由 ThreadActor/ThreadRepository 提交 pending Interaction；只有该事务
成功后，紧随其后的 `EndTurn` 才能让原 Turn 进入 terminal。工具不得在后台 spawn 一个
interaction callback 后立即结束，也不得依赖进程内 waiter 保存用户问题。非 Studio 宿主仍默认
使用 `AwaitResponse`，可在原 Turn 内等待 callback，不受 Studio 语义影响。

pending Interaction 是一种成功的 Turn completion boundary，不是业务 finalization。若当前 role 配置了
`RequiredTool`（例如 Task planner 的 `plan_exit`），RunningTurn 在该边界不得把缺少 required tool
改写为 validation failure；用户答复后的 fresh Turn 仍按原 role policy 继续，只有真正完成该阶段时
才必须调用 required tool。

Studio 回答 UserInput 时，把 resolved Interaction 与一个 hidden durable input 放入同一个
`ThreadCommit`。mail ID 固定为 `interaction-resolution:{interactionId}`，提交策略固定为
`StartOrQueue`：Thread idle 时开启 fresh Turn，有活动 Turn 时只排队，绝不 steer 当前 Turn；该输入
没有 queue coalescing key，也不参与 Task Planner wake 合并。事务失败时 Interaction 与 input 都不
落地；重复回答先按 canonical Interaction 状态、再按稳定 mail ID 幂等返回，不能创建第二个 input
或 Turn。ToolApproval 保留原有等待和审批语义。PlanConfirmation 的 `ContinuePlanning` 保留
Planner/Task phase 语义，但必须复用同一 durable continuation 边界：在一个 `ThreadCommit` 中
提交 resolved PlanConfirmation 与包含调整要求的 hidden input，mail ID 同样固定为
`interaction-resolution:{interactionId}`，idle 时开启 fresh Planner Turn，active 时只排队且绝不
steer。新的 Planner Turn 必须重新完成 `plan_exit`；`ImplementFreshContext` 和 `Dismiss` 继续使用
各自的实施启动与忽略语义，不因计划调整路径而改变。

pending Interaction 是成功的 Turn completion boundary，不是 Turn 内的等待 phase。当
origin Turn 仍是 Thread 当前 active Turn 时，pending Interaction 与随后的 `EndTurn` 一起提交：
原 Turn 落 `completed`，“等待用户”状态由 Thread 上挂的 pending Interaction 派生。Interaction
resolution 通过稳定 mail ID 的 durable hidden input 在 fresh Turn 继续，绝不复活已 terminal 的
origin Turn、覆盖无关 Turn 或伪造 active 状态。

Studio attach 会对活动 Task root Thread 执行一次有证据门禁的检查：只读取最新完整 plan Item，
并在没有对应 interaction、没有活动 TaskRun、且 plan 未进入实施或终态时补建确认。重复 attach
必须幂等，不能复活旧计划或制造多个 pending confirmation。

后台 stdio 子进程（MCP server、`exec` 命令、LSP server）由运行时显式持有生命周期。正常路径必须通过 async shutdown / terminate 请求关闭 stdin、终止进程树并等待退出；Drop 只能做 best-effort 兜底。容器 backend 必须同时终止宿主 transport 进程和容器内进程组，不能只杀 Docker CLI 后留下孤儿任务。Windows GUI 进程中启动这些后台子进程和兜底终止命令时不得显示额外终端窗口。

终态事件只允许出现一次。`completed` 表示工具成功执行，`failed` 表示工具实现或注册失败，`denied` 表示模式、策略或审批拒绝，`interrupted` 和 `budgetLimited` 表示 turn 控制层中断或预算限制。`approved` 可作为执行前的非终态状态展示，但不能替代最终 `completed` 或 `failed`。

`Tool` 通过明确的预算计时策略声明执行期间是否计入 turn 的活跃 wall-clock，默认计时。
`wait_agents` 声明暂停 wall-clock，但只有模型本批次恰好包含一个且该调用已通过注册、策略与
审批并成功调度时，才从活跃预算中扣除其阻塞区间。普通工具、模型请求、混合工具批次、
审批和 interaction 等待继续计时；该策略不改变工具终态、取消传播或 `waitCalls` 可观测计数。

工具批次完成后，canonical tool result 先进入 transcript，再把全部 receipt 一次性提交到
`AgentWorkingState` 的 Evidence Ledger。Ledger 仍最多保存 64 条、持久化正文最多 16 KiB，
但不直接进入 provider request；同一 Turn 的模型可见 working context 在首个 inference 前冻结，
并锚定在该 Turn 初始 transcript 之后。后续 inference 把新 assistant/tool result 追加在该锚点
之后，不得把 working-context message 重新移动到最新 transcript 尾部；否则 Responses WebSocket
无法复用严格前缀。上下文压缩会显式建立新锚点并使当前 continuation 失效，压缩后的后续请求再
恢复 append-only。旧 Ledger 版本不得作为 system message 累积在 transcript 中。

Programmatic program 本身不作为本地 shell 执行，也不授予命令能力；provider 只可请求 eligible
工具，runtime 返回带原始 `caller` 的 function/custom output，provider 再产生 program output。
原生 program、caller 与 output 的顺序是 transcript 协议事实，持久化恢复或 transport fallback
必须原样回放，且不得重复执行已经有配对 output 的嵌套调用。

确定性本地只读工具失败使用版本化 failure envelope 保存类别、首次 call id、错误 hash 和有界
摘要。相同工具、canonical 参数、workspace 与 mutation epoch 内只执行一次，重复调用返回紧凑
duplicate receipt；任何 WorkspaceWrite、Process 或 BranchControl 尝试都会推进 mutation epoch，
即使调用以错误结束。权限、瞬态 transport、外部副作用和未知错误不得缓存或自动重放。

每次 provider response 的工具批次在调度前冻结一次 mutation epoch，批次内所有只读缓存查询
使用同一快照。并发完成的 Process 或写工具仍立即推进真实 epoch，但只影响下一次 provider
response；不得让同一批次的完成时序把 canonical 相同的只读调用随机分裂到不同 cache key。
这只固定缓存事实的批次边界，不让 shell、写工具或外部副作用工具进入自动复用。

成功的本地只读结果也在 Turn 内按 canonical 参数和 mutation epoch 复用，命中时只返回首次 call、
结果 hash、原始字节数和有界摘要，不把大结果再次注入模型。任意工具只要通过 typed
`ToolCachePolicy` 明确声明结果可缓存，同一次 provider response 内 canonical 参数完全相同的
后续调用就直接返回 `duplicateSuppressed` receipt；每个 provider call id 仍各自获得配对结果，
但重复 receipt 不再复制首次输出或缓存摘要。`read_file` 额外记录实际返回的行区间；后续请求完全
落在已读区间内时返回 `coveredReadRange` receipt，只有请求包含未读行时才重新执行。shell、写工具、
进程控制和外部副作用工具不参与这种自动复用，工具名不同的调用也不做语义猜测。

与 Codex 的 tool-call 配对语义一致，不可缓存工具的每个不同 call id 都是独立调用；即使同一
provider response 中工具名与参数完全相同，`apply_patch`、`spawn_agent`、`exec`、动态 MCP 和其他
副作用工具也不得由通用 dispatcher 按参数吞并。运行时只拒绝同一 call id 的冲突回放。需要幂等的
产品操作必须在自身 durable repository 中用稳定业务键实现，例如 Task executor allocation 使用
requested call id，并可把仍 active 的同 assignment allocation 解析为既有 WorkUnit；这类复用仍
执行工具 handler，并为每个 provider call id 返回各自的 canonical 结果。共享 runtime 不按项目
路径、任务内容或命令文本猜测。

dispatcher 为每个 provider response 记录工具调用总数、可并行候选数、实际并发调用数、批次
wall-clock、各调用执行时长之和、最长执行时长、模型可见结果估算 token、缓存命中与重复抑制数。
这些指标只用于推导批处理节省和 token 收益，不影响调度顺序、结果预算或 retry 决策。

Task executor 达到 30 分钟 `WallClock` 预算时，预算事实仍作为当前 Turn 的 typed terminal 保存，
但 WorkUnit 不立即失败。Task coordinator 对同一 executor、Thread 和 worktree 强制执行一次
`WallClockRollover` compaction，并用确定性 hidden input 开启下一 Turn。一个 tranche 最多四个
30 分钟切片；第四次耗尽后 WorkUnit 进入 `needsAttention`，由 Planner 停止或拆分恢复。planner
用统一的 `send_message`（parent→direct-child）向子代理下发调度消息；不再有 Task 专用 send_message，
也不在发消息时隐式重置 WorkUnit tranche。非 wall-clock budget、用户停止、Task 取消和压缩失败
都不自动续轮。pending continuation 必须持久化并使用 WorkUnit/来源 Turn 组成的幂等键恢复。

一般 `budgetLimited` 不因 UserInput fresh-turn 机制获得自动续轮；只有上述 Task executor
`WallClockRollover` 状态机可以创建预算 continuation，避免把死循环或永久预算耗尽掩盖成无界 Turn。

## 运行时错误分类

模型与 turn 边界使用结构化 `TurnFailure` 保存失败事实。失败包含稳定类别、
provider code、HTTP status、用户可读消息与 `RetryDisposition`；可重试变体可附带
`retryAfterMs`。`TurnResult` 和 `AgentTurnOutcome` 必须携带同一份结构化失败，宿主产品
不得通过解析 `reason` 文本判断是否重试。provider 内部仅在模型流尚未产生可见或 canonical
事件、且工具副作用尚未发生时重放完整模型请求；重试耗尽后把原始瞬态语义交给宿主调度器。
`RetryDisposition` 只描述当前 provider 请求的 replay 安全性；Studio 的
`TaskFailureDisposition` 是独立产品决策。普通工具执行失败和 required finalization 验证失败必须
作为模型可见 tool failure 返回，不能因为 Turn 使用了 required tool 就升级成 fatal tool runtime。
只有工具 runtime invariant、join failure 或历史污染进入 fatal `TurnFailureCategory::Tool/Internal`。

Turn 终态投影必须把同一份 typed failure 写入 durable Turn；failed、interrupted 和
budget-limited Turn 的 terminal trace 同时生成带错误文本的 durable Timeline Item，作为没有
assistant 正文时的用户可见回退。Flutter 不得只依赖进程内 terminal 通知或 Composer 临时状态，
否则重启和历史加载会把已经持久化的失败表现为“输入没反应”。

Responses WebSocket、Responses HTTP/SSE 与 Chat Completions HTTP 的限流、连接容量、
`server_is_overloaded`、429、5xx、连接/超时，以及响应开始前的 connection reset、aborted、
broken pipe 或 EOF 错误统一映射为可重试 provider
failure。WS 只在首个 canonical 流事件前重放一次，失败后对当前 transport session 熔断到 HTTP；
HTTP 只在流对象建立前重放两次，两个 transport 都不得在流开始后 replay。重试使用带稳定
0.9–1.1 抖动的 200ms 指数退避，provider `Retry-After` 优先并按 30 秒封顶；鉴权、权限、输入
验证、请求构造、请求体和协议错误保持永久失败。错误正文仍用于展示和日志，但不再承担控制流协议。

工具调度层使用轻量 runtime envelope 统一执行结果：

- `ToolInvocation` 保存工具名、实际复用或创建的 trace part id、provider item id、Responses call id、payload 和执行上下文；工具 runtime 发出的结果 delta 必须使用同一个 trace part id。
- `ToolPayload` 区分 `Function(serde_json::Value)` 和 `Custom(String)`，避免 custom/freeform 工具被 JSON function 回放吞掉。
- `ToolOutputEnvelope` 区分模型可见文本、timeline 展示文本、完整输出文件、退出码和 timeout 标记。
- `ToolExecutionError::RespondToModel` 表示模型可恢复错误，必须写 tool result；`ToolExecutionError::Fatal` 表示内部 invariant、join failure 或历史污染，当前 turn 以 `ToolError` 失败。

provider 输出的 function tool arguments 必须是合法 JSON，并由 `pl-model` 统一解析为 `serde_json::Value` 后进入 `ToolCall`。非流式响应和流式 accumulator 都不得在解析失败时静默丢弃 tool call，也不得把失败的 JSON 降级为字符串参数；该情况属于 provider 协议错误，当前模型调用必须失败并把错误暴露到 turn。

`Tool` trait 为了支持运行时注册表和 MCP 动态工具，暂时保留 dyn-compatible `BoxFuture` 返回值。这是 trait object 边界的例外，不引入 `#[async_trait]`，也不扩散到新增业务 trait。

## 静态 function tool 定义

内建工具和产品静态工具的参数类型、反序列化规则与模型可见 JSON Schema 必须来自同一个
Rust typed definition。输入类型使用 `Deserialize + JsonSchema`，字段名和枚举 wire 值由 Serde
属性声明，模型可见说明由 rustdoc 或 Schemars 属性声明；`FunctionToolDefinition<Input>` 统一生成
并规范化 function schema、拒绝未知顶层字段、反序列化 arguments，并把 handler 注册为普通
`RegisteredTool`。静态工具不得手写 `properties`、`required` 或 `additionalProperties`，也不得
在各工具内遍历或修补生成后的 JSON Schema。

所有静态 function tool 发送到 provider 的参数 schema 根必须显式为 `type: "object"`。普通
struct 根继续统一关闭未知顶层字段；internally tagged enum 等由 `oneOf` / `anyOf` 表达且每个
分支都是 object 的联合输入只在根补 `type: "object"`，不得在没有根 `properties` 时补
`additionalProperties: false`。这类联合输入的未知字段由各 typed 变体的 Serde 约束拒绝，避免
根层既无法列举字段又把所有合法 action 参数一并拒绝。

同一工具族中语义、字段名、约束和描述都一致的字段组应抽成小型 typed component；顶层输入可用
`#[serde(flatten)]` 组合这些组件以保持既有扁平 camelCase wire。共享类型默认限制在工具族模块内，
不得仅因字段类型相同就复用，也不得用包含大量无关 `Option` 的万能输入类型。flattened component
只能使用无重名字段的命名 struct；其未知顶层字段拒绝由统一 typed adapter 根据生成后的 root
properties 执行，不能散落手写 JSON key 校验。

`serde_json::Value` 只保留在确实运行时动态的边界：provider wire、模型返回的原始 arguments、
远端 MCP schema、动态 `RegisteredTool::new`、运行时 role/target enum，以及 Custom、
Programmatic Tool Calling 和 Hosted Web Search 协议类型；hosted `tool_search` 与 Namespace
工具 wire 类型已随客户端 Tool Search 移除。MCP schema 由 rmcp 提供，
PL 只做保证可注册所需的最小 normalize，不得把第三方动态 schema 强制改成 PL 静态 strict schema。

测试应覆盖 typed 输入的必填字段、未知字段、enum、范围和业务校验，以及注册、权限、缓存、取消、
事件和 backend 错误映射。逐工具检查 `properties`/`required`/`additionalProperties` 的 JSON 树测试
应删除；Schema 形状只在统一生成器契约、动态 MCP 转换和 provider wire 边界集中验证。

工具族内部的聚合模块对数量较多的同领域 `use` / `pub use` 使用 `::*`，不保留只转发或只改名的
函数、re-export 薄层。glob 出口以 item visibility 为公共 API 边界：实现细节和输入类型保持私有或
`pub(crate)`；跨领域且具有筛选含义的稳定出口继续显式列出。

## 结果回传

工具结果进入模型上下文时仍使用字符串内容。失败结果必须包含稳定前缀和原始错误文本：

- 未知工具：`Unknown tool: {name}`
- 策略或用户拒绝：`Tool execution denied: {reason}`
- 本地执行错误：`Tool execution error: {error}`

这些结果必须写入模型上下文对应的 toolCall Item，即使工具被禁用、未知或拒绝。后续模型可以
据此恢复、改用其他工具或解释失败原因。

`apply_patch` 的解析或上下文匹配失败属于本地执行错误，仍使用 `Tool execution error: {error}` 前缀写回模型上下文。错误文本应包含可恢复提示：不要重复同一个失败 patch；先重新读取目标文件当前内容，再生成更小、更精确的 Codex 风格 patch 重试。失败前已经应用到工作区的 hunk 必须在错误文本中列出 applied changes，不能使用会被误解为 Git commit 的 committed 表述，方便后续模型只处理仍未应用的改动。

`exec` 和 `write_stdin` 成功执行时，写回模型上下文的 result 是一个紧凑 JSON 字符串，而不是完整原始输出。字段包括：

- `status`：`running`、`completed`、`failed`、`timedOut` 或 `interrupted`。
- `processId`：后台进程 id；仅当命令仍可继续观察或写入时存在。
- `exitCode`：进程已退出时的退出码，无法取得时为 `null`。
- `timedOut`：是否因 `timeoutSeconds` 触发终止。
- `stdout` / `stderr`：按 `maxOutputChars` 或默认 head/tail 预算截断后的新增文本。首次
  `exec` 快照领取启动后已产生的输出；后续 `write_stdin` 只领取前一模型快照之后的增量，
  不重复返回累计正文。
- `outputFile`：完整 stdout/stderr 文件路径。
- `message`：面向模型的下一步提示。

当 `exec` 在 `yieldTimeMs` 内未完成时，result 使用 `running` 状态并带 `processId`。后续模型必须用 `write_stdin` 携带该 `processId` 发送输入或传空 `chars` 轮询，不应重复执行同一条 `exec` 命令。命令管理器只有在子进程退出且 stdout/stderr 管道都已读到 EOF、完整输出文件已写入 workspace 后，才返回 `completed`、`failed`、`timedOut` 或 `interrupted` 终态并释放 `processId`；如果子进程已退出但尾部输出仍在排空，结果仍保持 `running` 并提示继续轮询。需要完整输出时，模型应使用文件读取工具读取 `outputFile`，不要要求命令工具把大输出完整塞回上下文。`write_stdin` 找不到 live process、进程数量达到上限、stdin 写入失败或后台命令已被终止时，应返回可恢复错误，让模型等待、轮询或解释当前状态。

空 `chars` 的正数等待统一至少为 10 秒，并在进程终态时提前返回；显式 `0` 仍用于立即快照。
写入非空 stdin 保留调用方请求的短等待，避免交互命令失去响应性。该退避属于通用工具运行时，
不由提示词或验收 harness 猜测项目命令。

模型快照领取增量与 stdout/stderr reader 使用同一进程状态锁；并发轮询至多由一个调用领取同一
片段，不重复也不丢失。增量缓冲保持有界 head/tail，超过模型预算的正文只在 `outputFile` 保留。
进程只有在 child 退出且两个输出流关闭后才进入终态，因此最后一次终态快照仍会领取尚未返回的
尾部输出。累计字节数、output revision 和 artifact 收集继续基于完整进程输出，而不是增量片段。

Studio 可在 `exec` 子进程仍运行时看到 stdout/stderr chunk。命令管理器更新内存截断缓冲并
递增 Item revision，再把 chunk 作为原 `exec` toolCall Item 的 `tool.result` delta 发布，同时
异步追加完整输出文件。terminal payload 的 revision 不低于最后一个 chunk。后台进程新增输出
始终归属最初启动它的 Item，不在父 Timeline 复制正文。

Studio Thread stream 只发送 typed Turn/Item/Interaction/runtime notification。Item delta 驻留
ThreadActor 内存；Turn/Item terminal 和 Interaction 变化必须在单库事务提交后广播。发生 lag
发送 `Lagged`，客户端重新订阅并以 authoritative snapshot 覆盖；不存在 durable cursor 或
replay journal。

Windows 本地 backend 上 `exec.command` 的默认宿主 shell 是 PowerShell：运行时先查找 `pwsh.exe`，再查找 `powershell.exe`，都不可用时才使用 `cmd.exe /C`。PowerShell 命令以 `-NoProfile -Command` 执行，并注入 UTF-8 输出设置；这只影响命令字符串的宿主 shell，不改变 `exec` / `write_stdin` 的公开 schema、审批策略或 JSON 结果字段。

Windows 的安全校验可以在内部使用 canonical path，但传给本地子进程的 workspace 与 cwd 必须
规范为 native non-verbatim 路径，不能把 `\\?\` / `\\?\UNC\` 表示泄漏给依赖当前目录的构建
工具。规范化沿用 Codex 的 `dunce::canonicalize` 语义：保留 canonical identity，同时消除普通
drive/UNC 路径的 extended prefix；权限判断仍在规范化前后使用同一可信 workspace 边界。

`spawn_agent`、`report_progress`、`send_message`、`interrupt_agent`、`list_agents`、
`wait_agents`、`read_agent_session` 和 `close_agent` 的模型可见输出必须由 pl-core
collaboration adapter 从 runtime typed snapshot 构造；宿主只通过 lifecycle、repository 与
event sink 提供产品资源和持久化事实，不手写共享状态形状。协作工具不接受 sessionId；
runtime 从目标 agent 的唯一 ThreadId 解析并验证同一 Thread 树、权限与 lifecycle。
`send_message` 永不取消：运行中作为 steer，空闲时启动明确的新 turn；`interrupt_agent`
只取消当前 turn。`wait_agents` 先订阅 Agent Directory watch 再读取 canonical snapshot，
没有 timeout 或轮询，只由目标 progress、interaction、terminal 或调用方取消结束，并以
`{ reason, messages }` 返回本次变化 agent 的最新 progress message 和精简状态，不重复完整
directory。`list_agents` 保留完整 canonical 目录查询，仅用于目标发现、重启对账或诊断；wait
返回后不要求再次 list。Studio
把 owner lifecycle/progress 投影到 root Thread 的 Agent Directory；每个 child Thread 只记录
该 owner 主动执行的协作 Item，Todo 作为 `ThreadRuntimeSnapshot` 的完整 replacement，不伪装成
Timeline Item。

`wait_agents` 的 `messages` 是破坏性新协议；旧模型历史、工具结果和派生缓存不做迁移或兼容
转换，协议切换后由产品重建新会话和相关 fixture。

产品 harness 的 spawn 契约不扩展通用 `spawn_agent` schema。Task 的
`task_spawn_executor` 接收 required `taskName/message` 与 optional `scopeHints`；hint 只描述
关注路径，不限制 worktree 内合法修改，也不阻止并发或 completion。runtime spawn 前只校验
hint 是规范仓库相对路径，并将可信内部 intent 交给 Studio lifecycle；
`task_request_delivery_review` 与 `task_request_integrated_review` 分别固定 completion
revision 和 Task HEAD 的 reviewer intent。这些工具都创建只属于新 agent 的 child Thread，
不使用 `spawn_agent.forkTurns`，但仍复用 AgentRuntime 的容量、repository、lifecycle saga、
Turn 启动与失败补偿。

Task executor 使用 fresh session，不复制 planner transcript。Task harness 在 allocation 后构造
版本化 `TaskExecutorHandoffV1`，把确认后的 Task plan、assignment、WorkUnit/HEAD、scope、验收
条件、依赖、带定位信息的证据、typed 验证命令和交付契约写入
`studio.task_executor_handoff` pinned section；该
section 与 child 初始 session 一起持久化。后续 Turn 必须从 durable session 和 WorkUnit 校验该
handoff，缺失、损坏或 owner/HEAD 不匹配时 fail closed，不回退到重新读取 planner 历史。

`update_todo_list` 是 Codex `update_plan` 风格的内置 checklist 工具，root agent 与 subagent 都可用，
且不代表 Plan Item。工具输入是完整快照：`explanation?: string` 与
`items: [{ step, status }]`，其中 `status` 只允许 `pending | inProgress | completed`，且最多一个
`inProgress`。成功后只返回 `{ status: "updated" }` 给模型，并替换 Thread runtime todo；
Flutter 只展示最新值，不按 patch 合并，也不渲染为 Timeline Item。

Thread 笔记是独立于模型历史和 pinned context 的持久化文本。它作为内部
`ModelContextItem` 随 Thread 保存，在 Thread 删除时一并删除；正文不进入
provider request、token 估算、附件物化或 compaction 输入。每个 turn 只注入一段有界提示，
说明当前 revision、字节数、行数和可用笔记工具。child Thread 使用独立笔记，不继承父 Thread。

会话笔记工具由 `read_session_note`、`search_session_note`、`write_session_note` 和
`apply_session_note_patch` 四个内置工具组成。读取按一基行号和有界行数返回，调用方可在取得
足够信息后停止，不要求读到 EOF。搜索是单行、ripgrep 风格的字面量或正则搜索，支持大小写、
上下文、结果上限和 revision 绑定游标；修改笔记后旧游标必须报过期。写入和 patch 使用
`expectedRevision` 做乐观并发控制，成功后 revision 单调递增；正文上限为 1 MiB。
`apply_session_note_patch` 复用 `pl-patch`，只接受虚拟路径 `session-note.md`，在内存副本完成
全部 hunk 后一次提交，拒绝移动和其他路径，任何失败都不得留下部分更新。

MCP tool 结果统一由一个 rmcp `CallToolResult` 转换器生成 `ToolOutput`。模型可见字符串按 MCP
content 顺序合并；JSON 或非文本 content 序列化为紧凑 JSON。完整 content、
`structuredContent`、`resultType`、`isError` 和 response meta 同时保留为 typed 审计 payload，
不得在 adapter 中丢弃。MCP `isError` 或 transport/protocol 错误按本地执行错误处理，使用
`Tool execution error: {error}` 前缀写回模型上下文，同时在产品 timeline 中展示失败原因。
transport/protocol 错误只把对应 server 的 availability 标记为 `unavailable`，不得污染其他
server。新 turn 不再获得该 server 的工具，持有旧 lease 的 turn 仍按固定 generation 收尾。
分页、SSE 与 JSON-RPC/MCP wire 细节全部由 rmcp 处理；PL MCP runtime 只按 connector、worker、
generation、RegisteredTool builder 和结果转换器拆分。

文件修改工具不向 schema 暴露语义模糊的 bool 参数。`delete_path` 使用 `mode: "file" | "emptyDirectory" | "recursiveDirectory"`；`copy_path` 和 `move_path` 使用 `collision: "failIfExists" | "overwrite"`。运行期不再保留 `recursive` / `overwrite` 旧 bool 字段的读取路径，历史会话或手写输入若使用旧字段会被 schema 校验拒绝，工具描述只暴露 `mode` / `collision`。

模型工具集合不提供独立文件内容搜索 schema、缓存、分页、programmatic eligibility 或 backend 契约。`list_files.glob` 只在列目录语境下表示条目路径过滤，不参与文件内容搜索。内容搜索使用 `exec` + `rg`，文件发现使用 `exec` + `rg --files`；这两类命令属于 Process effect，不进入确定性只读工具缓存。

## Studio 展示

Studio Timeline 直接按 ThreadItem ordinal 投影。每个工具调用是一个独立 toolCall Item；
Item 首次插入时固定 id、类型、ordinal 和位置，后续只更新 revision、内容与状态。Flutter 对排序后的
Item 单次扫描，只在视觉层合并相邻 toolCall；任何非工具 Item 立即结束分组。工具组详情显示工具名、
状态、关键路径或命令摘要；失败、拒绝、中断和预算限制必须显示结构化原因。

工具集合变化不迁移或删除历史 ThreadItem。历史会话中的已移除工具调用继续作为普通 toolCall Item
显示，Flutter 使用通用工具展示逻辑读取其名称、参数和既有结果，不尝试重新注册或执行该工具。

工具、命令、文件修改和子代理协作活动的用户可读文本由 Flutter 根据结构化 Item 生成；Todo
由独立侧栏读取 runtime snapshot。后端不新增本地化 `activityText`。固定标签和状态说明由 Flutter
i18n 负责，工具名、agent path、工作目录、路径、命令摘要和模型名保持原始领域值。

父 timeline 默认只展示 Planner 自己执行的子代理高层协作事实，例如 spawn、send、
interrupt、list、read、wait 和 close。子代理协作活动可按 `callId` 合并 begin/end 状态；
Todo replacement 只进入执行该调用的 Thread runtime snapshot。子代理内部普通工具 Item 不自动
灌入父 Timeline，细节保留在 child Thread。owner lifecycle/status/progress 只更新 Agent
Directory，不作为 Timeline Item。

## 会话尾部恢复

会话恢复只改变后续 provider 可见的 transcript，不回滚已经发生的世界状态。Turn、Item、usage、
Completion、Review、Merge、文件修改、Git commit 与外部工具副作用保持不可变审计记录。被恢复排除
的 Turn/Item 由查询投影标记为 `rolledBack`，仍按原 ordinal 出现在 Timeline，但不再参与后续模型
请求或有效进度判断；usage 与费用也不因恢复扣除。

`rewindTail` 只允许在 Thread idle、没有 active/pending input 和 pending Interaction 时执行。调用方
选择连续的 Turn 后缀；runtime 根据这些 Turn 实际消费的 mailbox input，精确匹配 transcript 尾部
user message 的 canonical hash，并从最早选中输入之前建立 replacement baseline。截断必须保留
完整的 assistant tool call 与 tool result 配对，不得留下孤立 call/output。hash 或配对不匹配时
必须拒绝，不能静默扩大回退范围。

`rebuildThread` 用于 compaction 或历史损坏使安全前缀不可证明的情况。它清空普通 transcript，
但保留 handoff、Evidence Ledger、session note 与其他 working state。两种模式都清空旧 Todo/进度
投影，写入版本化 recovery pinned context，要求模型以当前 Task repository 和 workspace/Git 现场
为事实源；同时以 `contextRecovered` 推进 prompt generation 并创建新的物理 model transport
session。恢复本身绝不重放工具调用，也不撤销工作区副作用。

## Web 搜索工具规划

`web_search` 是 `ToolEffect::Read` 且允许并行的内置函数工具。`plan_web_search()` 只读取解析后的
`ProviderServiceCapabilities`、协议、模型能力、凭据和产品 Web Search 配置，不识别 provider 或
preset id。planner 按当前 provider、角色路由和 provider 配置顺序确定性寻找 standalone candidate；
当前模型支持 function calling 时优先形成 additive standalone 工具，否则在当前 provider 同时支持
Responses hosted search 和模型 native search 时形成 exclusive hosted tool。

独立工具输入与 OpenAI `/alpha/search` commands 对齐，支持网页/图片查询、open、click、find、PDF screenshot、finance、weather、sports、time 和 response length。请求上下文只保留最近两个 user message 及其间受限的 assistant text，排除 system、environment 和 tool 消息。响应 `output` 写回模型，`results` 只进入 trace/persistence/UI，`encrypted_output` 不进入模型上下文。

`WebSearchPlan` 明确返回 `Additive | Exclusive | Unavailable` 可见性以及脱敏 resolution；无有效
candidate 时区分 disabled、缺凭据、provider 不支持和模型不支持。产品只能通过 PL 统一安装入口
应用 plan，不能改变候选优先级，也不能直接构造独立或 hosted Web Search 工具。执行器再次校验
plan 中解析出的 provider，保证错误配置或历史 tool call 不能绕过门控发起远程请求。
