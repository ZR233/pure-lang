# 工具调用运行时

## 目标

工具调用运行时负责把模型返回的 tool call 转换为本地工具执行、审批、Studio message/part 事件和下一轮模型输入。它必须保证同一个工具调用身份稳定、终态 snapshot 唯一、失败结果可回传给模型，并在 Studio 中保留用户可读的错误原因。

## 身份字段

`ToolCall.id` 是 provider 返回的工具调用 item id。Chat Completions 历史回放时，assistant 消息中的 `tool_calls[].id` 和后续 tool 消息的 `tool_call_id` 必须使用该值。

`ToolCall.call_id` 是 Responses API 的调用 id。Responses 历史回放时，`function_call_output` 和 `custom_tool_call_output` 的 `call_id` 必须优先使用该值；缺失时才回退到 `ToolCall.id`。

Core 会话中的 tool result metadata 同时保存两个字段：

- `tool_call_id`：写入 `ToolCall.id`，供 Chat Completions tool message 使用。
- `tool_call_call_id`：写入 `ToolCall.call_id`，供 Responses output item 使用。
- `tool_call_kind`：写入 `function` 或 `custom`，供下一轮请求按原始工具种类回放。
- `tool_name` / `tool_call_arguments`：写入工具名和原始参数，供历史校验、调试和兼容旧会话读取。

这些 metadata 必须通过 typed helper 写入和读取，不允许在 `pl-core`、`pl-model` 之间散落字符串 key。新增会话消息缺少 `tool_call_kind`、`tool_call_id` 或 Responses `call_id` 时，provider request 构造一律返回协议错误——运行期不保留任何缺字段兼容路径。unknown `tool_call_kind` 或缺失 `tool_call_kind` 都是协议错误，不能静默回退到 function。

Studio 工具 trace item id 使用最早稳定的 provider item id 或 runtime tool id 作为相关性锚点，再按 turn 做命名空间隔离；如果 provider 没有 item id，才回退到 `ToolCall.call_id` 或本地 fallback id。后续 delta/done 才补 `call_id` 或 provider item id 时，运行时必须把新身份作为同一工具调用的 correlation alias，继续更新原 trace item，不能把同一个工具调用拆成第二个 trace item。`StudioPart.partId` 不再透传该 trace id，而是在 trace part 首次进入 Studio runtime 时由 turn timeline actor 分配；`StudioToolPart.toolCallId` 表示 runtime 工具展示/执行 id，provider item id 使用 `StudioToolPart.providerItemId`，Responses call id 使用 `StudioToolPart.callId`。core trace 中的 `TracePart` 来自内部 `pl-trace` crate，只允许作为诊断输入，经 Studio runtime 转换为 actor-owned `message.part.updated` / `message.part.delta` 后才能进入 UI。

## 模型流完成语义

`pl-model` 只有在收到 provider-independent `StreamEvent::Completed` 后，才能把一次模型调用视为成功。SSE parse error、transport error 或 EOF-before-completed 都必须返回稳定 `PureError`，并让 turn 失败，不能把已累计的局部内容当作成功 assistant 响应写入历史。`Completed` 或 `Failed` 是单次 provider stream 的终态；终态之后到达的 text、reasoning summary、raw reasoning、tool 或 usage 事件都是 provider 协议错误，不得继续追加到 response 内容、trace part 或 Studio timeline。

Chat Completions provider 如果没有 Responses 风格的 completed event，protocol mapper 必须在终止 chunk 合成 `StreamEvent::Completed`。聚合层允许在缺少显式 tool complete event 时兜底完成已聚合工具调用，但该工具调用必须已经有稳定工具名，以及 `id` 或 `call_id`。缺少工具名的残留 tool accumulator 表示 provider/protocol 可恢复错误，不得执行空工具名。

## 生命周期

每个工具调用先广播并持久化一个 `message.part.updated` 工具 snapshot，表示模型已请求该工具。随后运行时执行以下流程：

1. 检查当前模式是否允许该工具。
2. 查找工具注册表。
3. 对声明路径语义的工具输入执行统一路径解析和访问分类。
4. 计算权限策略，必要时请求用户审批或 reviewer 审批。
5. 对批准的工具执行本地实现；对禁用、未知或拒绝的工具直接生成工具结果。
6. 在统一收尾阶段写入唯一终态 `message.part.updated` snapshot。

路径类工具不要求模型提供绝对路径。运行时把相对路径按 `workspaceRoot` 解析，规范化为绝对路径后再进入权限判断和实际执行；文件工具、`apply_patch`、`exec.cwd`、`lsp_query_*` 的 `filePath` 和权限 precheck 必须复用同一个 resolver，避免审批看到 workspace 内而执行时解析到 workspace 外。`WorkspaceOnly` 模式拒绝 `..`、Windows drive-relative、越界绝对路径、越界 UNC / verbatim 路径和符号链接越界；`full-access` 允许本地 backend 解析 workspace 外路径，但宿主注入的容器或远程 backend 可以保持更严格的隔离边界。

模型只看到环境无关的 `exec`、`write_stdin`、`read_file`、`search_files` 和 `apply_patch`。`ToolSetBuilder` 分别接收 `CommandBackend` 与 `WorkspaceFileBackend`；Studio 注入本地实现，Mai 等宿主注入容器或远程实现。PL 统一拥有 schema、权限、进程表、stdin、超时、取消、输出截断和 turn 清理，backend 只负责 cwd 映射、启动/终止进程、发布完整输出和生成宿主 artifact。低层容器复制能力只服务文件 backend 与输出同步，不注册为模型工具。新请求不得暴露 `bash`、`container_exec`、`run_in_container` 或 `container_copy`；恢复旧会话时仅在持久化历史边界把前三种旧命令调用名称规范化为 `exec`。

`list_files` 的 `glob` 既可以匹配 workspace-relative 路径，也可以匹配 `path` 参数之下的相对条目；`includeDirs=true` 时目录候选按带尾随 `/` 的形式参与匹配。`**/` 表示零层或多层目录，因此 `**/Cargo.toml` 必须同时匹配 workspace 根下和子目录下的 `Cargo.toml`。

本地 `list_files` / `search_files` 递归遍历不得跟随符号链接或 Windows reparse point，也不把这些入口作为普通文件或目录返回。链接是 workspace 文件边界，不应让 Flutter plugin symlink、目录联接或不可访问的挂载目标使整次搜索失败；跳过链接之外的真实目录读取和元数据错误仍须显式失败。

MCP tools 由 `McpRuntimeHandle` 的 turn lease 注册。`McpRuntime<H>` 的泛型 worker 持有具体
`McpRuntimeHost`，产品和工具只持有非泛型 handle；`McpTurnLease` 固定 generation、tool schema、
resource 入口和调用 backend。Simple executor 可以使用 available tools；Task planner、explorer、
reviewer 只暴露 effect 策略明确允许的动态工具，未知 effect 默认拒绝。内置 Zhipu 工具统一声明为
`ToolEffect::Read`。

内置 Zhipu Coding Plan MCP server 优先复用 Zhipu Coding Plan provider 的 `bearer_token`，并兼容回退到普通 Zhipu provider 的 `bearer_token`。缺少 token 时内置 server 处于 `missingCredential`，不参与后台探测，也不应导致普通 turn 或 subagent 启动失败；检测到 token 后进入后台探测流程，只有探测成功的 server 会被主会话和 subagent runner 注册。HTTP 内置 server 在 transport 层直接发送 bearer token；stdio Vision server 在启动进程时注入 `Z_AI_API_KEY` 和 `Z_AI_MODE=ZHIPU`。

每个 turn 开始时，运行时会把经过当前模式过滤后实际暴露给模型的工具名快照保留为 core 内部 `TracePart`，并在需要时通过 typed Studio part snapshot 展示。该记录只包含 turn id、模式和工具名列表，用于诊断工具可见性，不进入模型上下文；旧 `timeline_events` 表及其 migration、运行期读写路径均已删除。

`plan_exit` 是 Task planning 阶段专用的内置协调工具，schema 只包含 `content: string`。它表示“计划已完成，请 Studio 发起确认交互”，不是执行工具；确认实施后会话保持 Task，由 coordinator 推进后续阶段。`<proposed_plan>` 不再是协议入口。

后台 stdio 子进程（MCP server、`exec` 命令、LSP server）由运行时显式持有生命周期。正常路径必须通过 async shutdown / terminate 请求关闭 stdin、终止进程树并等待退出；Drop 只能做 best-effort 兜底。容器 backend 必须同时终止宿主 transport 进程和容器内进程组，不能只杀 Docker CLI 后留下孤儿任务。Windows GUI 进程中启动这些后台子进程和兜底终止命令时不得显示额外终端窗口。

终态事件只允许出现一次。`completed` 表示工具成功执行，`failed` 表示工具实现或注册失败，`denied` 表示模式、策略或审批拒绝，`interrupted` 和 `budgetLimited` 表示 turn 控制层中断或预算限制。`approved` 可作为执行前的非终态状态展示，但不能替代最终 `completed` 或 `failed`。

## 运行时错误分类

模型与 turn 边界使用结构化 `TurnFailure` 保存失败事实。失败包含稳定类别、
provider code、HTTP status、用户可读消息与 `RetryDisposition`；可重试变体可附带
`retryAfterMs`。`TurnResult` 和 `AgentTurnOutcome` 必须携带同一份结构化失败，宿主产品
不得通过解析 `reason` 文本判断是否重试。provider 内部仅在工具副作用尚未发生时重放
完整模型请求；重试耗尽后把原始瞬态语义交给宿主调度器。

Responses WebSocket 的限流、连接容量、`server_is_overloaded`、408/409/425/429、5xx
及瞬态网络错误统一映射为可重试 provider failure。鉴权、权限、输入验证和协议错误保持
永久失败；错误正文仍用于展示和日志，但不再承担控制流协议。

工具调度层使用轻量 runtime envelope 统一执行结果：

- `ToolInvocation` 保存工具名、实际复用或创建的 trace part id、provider item id、Responses call id、payload 和执行上下文；工具 runtime 发出的结果 delta 必须使用同一个 trace part id。
- `ToolPayload` 区分 `Function(serde_json::Value)` 和 `Custom(String)`，避免 custom/freeform 工具被 JSON function 回放吞掉。
- `ToolOutputEnvelope` 区分模型可见文本、timeline 展示文本、完整输出文件、退出码和 timeout 标记。
- `ToolExecutionError::RespondToModel` 表示模型可恢复错误，必须写 tool result；`ToolExecutionError::Fatal` 表示内部 invariant、join failure 或历史污染，当前 turn 以 `ToolError` 失败。

provider 输出的 function tool arguments 必须是合法 JSON，并由 `pl-model` 统一解析为 `serde_json::Value` 后进入 `ToolCall`。非流式响应和流式 accumulator 都不得在解析失败时静默丢弃 tool call，也不得把失败的 JSON 降级为字符串参数；该情况属于 provider 协议错误，当前模型调用必须失败并把错误暴露到 turn。

`Tool` trait 为了支持运行时注册表和 MCP 动态工具，暂时保留 dyn-compatible `BoxFuture` 返回值。这是 trait object 边界的例外，不引入 `#[async_trait]`，也不扩散到新增业务 trait。

## 结果回传

工具结果进入模型上下文时仍使用字符串内容。失败结果必须包含稳定前缀和原始错误文本：

- 未知工具：`Unknown tool: {name}`
- 策略或用户拒绝：`Tool execution denied: {reason}`
- 本地执行错误：`Tool execution error: {error}`

这些结果必须作为 tool result 写入会话历史，即使工具被禁用、未知或拒绝。后续模型可以据此恢复、改用其他工具或向用户解释失败原因。

`apply_patch` 的解析或上下文匹配失败属于本地执行错误，仍使用 `Tool execution error: {error}` 前缀写回模型上下文。错误文本应包含可恢复提示：不要重复同一个失败 patch；先重新读取目标文件当前内容，再生成更小、更精确的 Codex 风格 patch 重试。成功前已提交的 hunk 必须在错误文本中列出 committed delta，方便后续模型只处理剩余改动。

`exec` 和 `write_stdin` 成功执行时，写回模型上下文的 result 是一个紧凑 JSON 字符串，而不是完整原始输出。字段包括：

- `status`：`running`、`completed`、`failed`、`timedOut` 或 `interrupted`。
- `processId`：后台进程 id；仅当命令仍可继续观察或写入时存在。
- `exitCode`：进程已退出时的退出码，无法取得时为 `null`。
- `timedOut`：是否因 `timeoutSeconds` 触发终止。
- `stdout` / `stderr`：按 `maxOutputChars` 或默认 head/tail 预算截断后的文本。
- `outputFile`：完整 stdout/stderr 文件路径。
- `message`：面向模型的下一步提示。

当 `exec` 在 `yieldTimeMs` 内未完成时，result 使用 `running` 状态并带 `processId`。后续模型必须用 `write_stdin` 携带该 `processId` 发送输入或传空 `chars` 轮询，不应重复执行同一条 `exec` 命令。命令管理器只有在子进程退出且 stdout/stderr 管道都已读到 EOF、完整输出文件已写入 workspace 后，才返回 `completed`、`failed`、`timedOut` 或 `interrupted` 终态并释放 `processId`；如果子进程已退出但尾部输出仍在排空，结果仍保持 `running` 并提示继续轮询。需要完整输出时，模型应使用文件读取工具读取 `outputFile`，不要要求命令工具把大输出完整塞回上下文。`write_stdin` 找不到 live process、进程数量达到上限、stdin 写入失败或后台命令已被终止时，应返回可恢复错误，让模型等待、轮询或解释当前状态。

Studio 实时展示层可以在 `exec` 子进程仍运行时看到 stdout/stderr chunk。命令管理器读取管道后先更新内存截断缓冲并分配输出 revision，再通过 trace delta 把 chunk 投影到原 `exec` tool part 的 `tool.result` live overlay，同时异步追加完整输出文件；delta revision 从该 part 已有 revision 继续递增，终态 JSON snapshot 的 revision 不低于最后一个输出 chunk。`write_stdin` 负责写入或轮询后台进程，返回自己的紧凑 JSON 结果；后台进程新增输出仍归属最初启动它的 `exec` tool part，不在父 timeline 中复制成新的工具输出正文。

Studio runtime 的 live-only event 通道只允许发送 `MessagePartDelta` 和 `Stale`。turn、message、part snapshot、agent snapshot、interaction、runtime usage 等 durable 事件必须先通过 store transaction 校验、分配 durable sequence、写入 projection 并持久化后再广播，不能误用 live-only 通道，否则前端 durable cursor 与历史回放会分叉。

Windows 本地 backend 上 `exec.command` 的默认宿主 shell 是 PowerShell：运行时先查找 `pwsh.exe`，再查找 `powershell.exe`，都不可用时才使用 `cmd.exe /C`。PowerShell 命令以 `-NoProfile -Command` 执行，并注入 UTF-8 输出设置；这只影响命令字符串的宿主 shell，不改变 `exec` / `write_stdin` 的公开 schema、审批策略或 JSON 结果字段。

`spawn_agent`、`send_input`、`wait_agent`、`list_agents` 和 `close_agent` 的模型可见输出必须由 pl-core collaboration adapter 从 runtime typed snapshot 构造；宿主只通过 lifecycle、repository 与 event sink 提供产品资源和持久化事实，不手写共享状态形状。`wait_agent` 在目标 `Idle` 且队列为空时返回 `{ target, timedOut: false, snapshot, lastTurn }`，超时仅返回 `{ target, timedOut: true }`；需要树级状态时调用 `list_agents` 获取 compact snapshot。后续输入由 `send_input.delivery` 的 `QueueOnly | Start | InterruptThenStart` 明确表达，不存在单独的 resume 命令。Studio 展示依赖持久化后的 `AgentChanged` latest snapshot 和 `SubAgentActivity` / `TodoListUpdated` append-only timeline。`spawn_agent.forkTurns` 的历史继承只复制过滤后的父会话消息，不复制工具结果、工具调用 metadata、reasoning 内容或运行时调度提示。

`update_todo_list` 是 Codex `update_plan` 风格的内置 checklist 工具，root agent 与 subagent 都可用，且不代表 Plan Mode 的 `plan` part。工具输入是完整快照：`explanation?: string` 与 `items: [{ step, status }]`，其中 `status` 只允许 `pending | inProgress | completed`，且同一快照最多一个 `inProgress`。工具成功后只返回紧凑 `{ status: "updated" }` 给模型，同时发送 `TodoListUpdated` agent timeline event；后端不维护 latest todo cache，也不按 patch 增量合并。

会话笔记是独立于模型历史和 pinned context 的持久化文本。它作为隐藏的
`ModelContextItem` 随 canonical session 保存，在 session 删除时一并删除；正文不进入
provider request、token 估算、附件物化或 compaction 输入。每个 turn 只注入一段有界提示，
说明当前 revision、字节数、行数和可用笔记工具。child agent fork 创建独立 session，
不继承父 session 的笔记。

会话笔记工具由 `read_session_note`、`search_session_note`、`write_session_note` 和
`apply_session_note_patch` 四个内置工具组成。读取按一基行号和有界行数返回，调用方可在取得
足够信息后停止，不要求读到 EOF。搜索是单行、ripgrep 风格的字面量或正则搜索，支持大小写、
上下文、结果上限和 revision 绑定游标；修改笔记后旧游标必须报过期。写入和 patch 使用
`expectedRevision` 做乐观并发控制，成功后 revision 单调递增；正文上限为 1 MiB。
`apply_session_note_patch` 复用 `pl-patch`，只接受虚拟路径 `session-note.md`，在内存副本完成
全部 hunk 后一次提交，拒绝移动和其他路径，任何失败都不得留下部分更新。

MCP tool 成功结果写回紧凑字符串。文本内容按 MCP content 顺序合并；JSON 或非文本内容序列化为紧凑 JSON。MCP `isError` 或 transport/protocol 错误按本地执行错误处理，使用 `Tool execution error: {error}` 前缀写回模型上下文，同时在产品 timeline 中展示失败原因。transport/protocol 错误只把对应 server 的 availability 标记为 `unavailable`，不得污染其他 server。新 turn 不再获得该 server 的工具，持有旧 lease 的 turn 仍按固定 generation 收尾。HTTP MCP 的 SSE 响应必须先按事件收集完整 `data` payload，再交给 JSON-RPC wire 类型反序列化，不能只解析第一行 `data`。MCP runtime 按 contract、worker、generation、tool adapter、local host 和 wire protocol 拆分；产品 Host 不实现 reconcile、命名或健康状态机。

文件修改工具不向 schema 暴露语义模糊的 bool 参数。`delete_path` 使用 `mode: "file" | "emptyDirectory" | "recursiveDirectory"`；`copy_path` 和 `move_path` 使用 `collision: "failIfExists" | "overwrite"`。运行期不再保留 `recursive` / `overwrite` 旧 bool 字段的读取路径，历史会话或手写输入若使用旧字段会被 schema 校验拒绝，工具描述只暴露 `mode` / `collision`。

文件搜索工具的参数名必须避免把“搜索内容”和“路径过滤”混在一起。`search_files` 使用必填 `pattern` 表示要在 UTF-8 文件内容中查找的 literal text；可选 `filePattern` 仅用于过滤被搜索的文件路径，例如 `*.rs` 或 `src/*`。`search_files` 不暴露 `query` 字段，也不把 `pattern` 解释为路径过滤。`list_files.pattern` 只在列目录语境下表示条目路径过滤，不参与文件内容搜索。

## Studio 展示

Studio timeline 以 message/part projection 派生的 conversation row 为准。后端不创建聚合工具 part；每个工具调用仍作为独立 `StudioPartType::Tool` snapshot/delta 持久化，但 tool part 必须在 Studio wire、FRB DTO 和 `message_parts.activity_group_id` 中携带 `activityGroupId`。该字段由 turn timeline actor 根据 assistant 阅读流边界分配：连续工具复用当前工具活动段，遇到可见 assistant text/commentary/final、reasoning、plan 或 agent row 后关闭当前段，之后工具新开段。Flutter timeline projection 只把相同 `activityGroupId` 的 tool part 合并为一个默认折叠的工具活动组；缺失该字段的历史 tool part 按单工具组展示。工具组详情必须显示工具名称、状态、关键路径或命令摘要。静默文件工具的成功结果可以隐藏在详情中；但失败、拒绝、中断和预算受限时必须在组摘要和详情中展示 result/error，避免用户只看到“工具调用失败”而无法定位原因。

工具、命令、文件修改、子代理协作活动和 todo list 更新的用户可读文本由前端 projection 根据结构化 `StudioPart.tool`、`StudioPart.agent` 与 agent timeline typed payload 生成。后端不新增 `activityText` 之类的本地化文案字段；如果展示层缺少必要事实，应补充结构化字段而不是补一段后端写死文本。固定标签和状态说明由 Flutter i18n 负责，工具名、agent path、工作目录、路径、命令摘要和模型名按原始领域值展示。工具运行时的单工具 start/end/approval/review commentary 属于 verbose/debug 诊断信息，普通模式只保留 turn 级工具批次 commentary，避免 timeline 在已有工具组之外重复出现每个工具的进展文本。

父 timeline 默认只展示子代理高层协作事件，例如 spawn、wait、send/followup、close 和 todo list update。子代理协作活动可按 `callId` 合并 begin/end 状态；todo list update 必须按每次调用新增 row，不参与该合并。子代理内部普通工具 trace 不自动灌入父 timeline；这些细节应保留在子代理详情、状态栏弹层或专门的 agent 视图中。`AgentChanged` 是 latest snapshot merge，适合更新状态栏和活动详情，不应作为每次状态变更的新 timeline row。

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
