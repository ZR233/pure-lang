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

这些 metadata 必须通过 typed helper 写入和读取，不允许在 `pl-core`、`pl-model` 之间散落字符串 key。新增会话消息缺少 `tool_call_kind`、`tool_call_id` 或 Responses `call_id` 时，provider request 构造应返回协议错误；只有历史兼容路径可以把缺少 `tool_call_kind` 的旧 tool result 当作 function 读取。unknown `tool_call_kind` 一律是协议错误，不能静默回退到 function。

Studio 工具 part id 使用可展示、可去重的运行时 id：优先取 `ToolCall.call_id`，否则取 `ToolCall.id`，再按 turn 做命名空间隔离。`StudioPart.partId` 和 `StudioToolPart.toolCallId` 表示 runtime 展示 id；provider item id 使用 `StudioToolPart.providerItemId`；Responses call id 使用 `StudioToolPart.callId`。core trace 中的 `TracePart` 只允许作为内部诊断输入，经 Studio runtime 转换为 `message.part.updated` / `message.part.delta` 后才能进入 UI。

## 模型流完成语义

`pl-model` 只有在收到 provider-independent `StreamEvent::Completed` 后，才能把一次模型调用视为成功。SSE parse error、transport error 或 EOF-before-completed 都必须返回稳定 `PureError`，并让 turn 失败，不能把已累计的局部内容当作成功 assistant 响应写入历史。

Chat Completions provider 如果没有 Responses 风格的 completed event，protocol mapper 必须在终止 chunk 合成 `StreamEvent::Completed`。聚合层允许在缺少显式 tool complete event 时兜底完成已聚合工具调用，但该工具调用必须已经有稳定工具名，以及 `id` 或 `call_id`。缺少工具名的残留 tool accumulator 表示 provider/protocol 可恢复错误，不得执行空工具名。

## 生命周期

每个工具调用先广播并持久化一个 `message.part.updated` 工具 snapshot，表示模型已请求该工具。随后运行时执行以下流程：

1. 检查当前模式是否允许该工具。
2. 查找工具注册表。
3. 对声明路径语义的工具输入执行统一路径解析和访问分类。
4. 计算权限策略，必要时请求用户审批或 reviewer 审批。
5. 对批准的工具执行本地实现；对禁用、未知或拒绝的工具直接生成工具结果。
6. 在统一收尾阶段写入唯一终态 `message.part.updated` snapshot。

路径类工具不要求模型提供绝对路径。运行时把相对路径按 `workspaceRoot` 解析，规范化为绝对路径后再进入权限判断和实际执行；文件工具、`apply_patch`、`bash.workingDirectory`、`lsp_query_*` 的 `filePath` 和权限 precheck 必须复用同一个 resolver，避免审批看到 workspace 内而执行时解析到 workspace 外。`WorkspaceOnly` 模式拒绝 `..`、Windows drive-relative、越界绝对路径、越界 UNC / verbatim 路径和符号链接越界；`full-access` 允许 workspace 外路径，但仍要求目标或最近存在父目录可解析。

MCP tools 由进程内 MCP runtime registry 的当前可用快照注册，工具名固定为 `mcp__{server_id}__{tool_name}`。effective MCP server 包含用户配置的 `[mcp_servers]` 和运行时合成的内置 server；`enabled` 只表示用户启用意图，不表示本轮一定可调用。Studio 启动、保存 provider 或 MCP 配置后，后台对配置启用且凭据完整的 server 执行 connect、initialize、initialized、tools/list 探测，并把结果记录为内存 availability。普通 turn 和 subagent runner 不等待探测完成，只加载当前 `available` server 的缓存 tools；`checking`、`unavailable`、`disabled`、`missingCredential` 都不会在本轮暴露给模型。启用 MCP server 表示用户显式信任该 server，因此已可用 MCP tools 在 Auto Mode 和 Plan Mode 中直接暴露并执行，不再触发额外审批或 reviewer 审批。MCP tool 的注册、执行和错误仍复用同一生命周期与 tool result 规则。

内置 Zhipu Coding Plan MCP server 优先复用 Zhipu Coding Plan provider 的 `bearer_token`，并兼容回退到普通 Zhipu provider 的 `bearer_token`。缺少 token 时内置 server 处于 `missingCredential`，不参与后台探测，也不应导致普通 turn 或 subagent 启动失败；检测到 token 后进入后台探测流程，只有探测成功的 server 会被主会话和 subagent runner 注册。HTTP 内置 server 在 transport 层直接发送 bearer token；stdio Vision server 在启动进程时注入 `Z_AI_API_KEY` 和 `Z_AI_MODE=ZHIPU`。

每个 turn 开始时，运行时会把经过当前模式过滤后实际暴露给模型的工具名快照保留为 core 内部 `TracePart`，并在需要时通过 typed Studio part snapshot 展示。该记录只包含 turn id、模式和工具名列表，用于诊断工具可见性，不进入模型上下文，也不写入旧 `timeline_events` 运行期表。

`plan_exit` 是 Plan Mode 专用的内置协调工具，schema 只包含 `content: string`。它表示“计划已完成，请 Studio 发起确认交互”，不是普通执行工具：运行时只在 Plan Mode 暴露它，Auto Mode 不暴露；工具执行成功后返回紧凑状态文本，不在工具内部等待用户、不切换模式、不写计划文件。`pl-core` 在工具成功后从原始参数读取 `content`，生成或补齐当前 turn 的 plan part snapshot。Plan turn 完成后，Studio runtime 继续复用既有 `PlanConfirmation` pending interaction 和 fresh-context handoff 流程。兼容的 `<proposed_plan>` 流式块仍可生成 plan part，但同一 turn 内 `plan_exit.content` 是确认计划的优先来源。

后台 stdio 子进程（MCP server、`bash` 命令、LSP server）由运行时显式持有生命周期。正常路径必须通过 async shutdown / terminate 请求关闭 stdin、终止进程树并等待退出；Drop 只能做 best-effort 兜底。Windows GUI 进程中启动这些后台子进程和兜底终止命令时不得显示额外终端窗口。

终态事件只允许出现一次。`completed` 表示工具成功执行，`failed` 表示工具实现或注册失败，`denied` 表示模式、策略或审批拒绝，`interrupted` 和 `budgetLimited` 表示 turn 控制层中断或预算限制。`approved` 可作为执行前的非终态状态展示，但不能替代最终 `completed` 或 `failed`。

## 运行时错误分类

工具调度层使用轻量 runtime envelope 统一执行结果：

- `ToolInvocation` 保存工具名、runtime item id、provider item id、Responses call id、payload 和执行上下文。
- `ToolPayload` 区分 `Function(serde_json::Value)` 和 `Custom(String)`，避免 custom/freeform 工具被 JSON function 回放吞掉。
- `ToolOutputEnvelope` 区分模型可见文本、timeline 展示文本、完整输出文件、退出码和 timeout 标记。
- `ToolExecutionError::RespondToModel` 表示模型可恢复错误，必须写 tool result；`ToolExecutionError::Fatal` 表示内部 invariant、join failure 或历史污染，当前 turn 以 `ToolError` 失败。

`Tool` trait 为了支持运行时注册表和 MCP 动态工具，暂时保留 dyn-compatible `BoxFuture` 返回值。这是 trait object 边界的例外，不引入 `#[async_trait]`，也不扩散到新增业务 trait。

## 结果回传

工具结果进入模型上下文时仍使用字符串内容。失败结果必须包含稳定前缀和原始错误文本：

- 未知工具：`Unknown tool: {name}`
- 策略或用户拒绝：`Tool execution denied: {reason}`
- 本地执行错误：`Tool execution error: {error}`

这些结果必须作为 tool result 写入会话历史，即使工具被禁用、未知或拒绝。后续模型可以据此恢复、改用其他工具或向用户解释失败原因。

`apply_patch` 的解析或上下文匹配失败属于本地执行错误，仍使用 `Tool execution error: {error}` 前缀写回模型上下文。错误文本应包含可恢复提示：不要重复同一个失败 patch；先重新读取目标文件当前内容，再生成更小、更精确的 Codex 风格 patch 重试。成功前已提交的 hunk 必须在错误文本中列出 committed delta，方便后续模型只处理剩余改动。

`bash` 和 `write_stdin` 成功执行时，写回模型上下文的 result 是一个紧凑 JSON 字符串，而不是完整原始输出。字段包括：

- `status`：`running`、`completed`、`failed`、`timedOut` 或 `interrupted`。
- `processId`：后台进程 id；仅当命令仍可继续观察或写入时存在。
- `exitCode`：进程已退出时的退出码，无法取得时为 `null`。
- `timedOut`：是否因 `timeoutSeconds` 触发终止。
- `stdout` / `stderr`：按 `maxOutputChars` 或默认 head/tail 预算截断后的文本。
- `outputFile`：完整 stdout/stderr 文件路径。
- `message`：面向模型的下一步提示。

当 `bash` 在 `yieldTimeMs` 内未完成时，result 使用 `running` 状态并带 `processId`。后续模型必须用 `write_stdin` 携带该 `processId` 发送输入或传空 `chars` 轮询，不应重复执行同一条 `bash` 命令。需要完整输出时，模型应使用文件读取工具读取 `outputFile`，不要要求命令工具把大输出完整塞回上下文。`write_stdin` 找不到 live process、进程数量达到上限、stdin 写入失败或后台命令已被终止时，应返回可恢复错误，让模型等待、轮询或解释当前状态。

`wait_agent` 和 `list_agents` 默认只回传紧凑 agent 摘要，避免把完整 agent snapshot 反复写入模型上下文。调用方显式传入 `includeDetails: true` 时，工具结果可包含完整 `AgentRecord`，用于诊断；普通协作流程应优先依赖精简摘要和最终子代理总结。`spawn_agent.forkTurns` 的历史继承只复制过滤后的父会话消息，不复制工具结果、工具调用 metadata、reasoning 内容或运行时调度提示。

MCP tool 成功结果写回紧凑字符串。文本内容按 MCP content 顺序合并；JSON 或非文本内容序列化为紧凑 JSON。MCP `isError` 或 transport/protocol 错误按本地执行错误处理，使用 `Tool execution error: {error}` 前缀写回模型上下文，同时在 Studio timeline 中展示失败原因。transport/protocol 错误还会把对应 server 的 availability 标记为 `unavailable`，后续 turn 不再暴露该 server 的 tools，直到后台周期重检或保存配置后的 reconcile 恢复。

文件修改工具不向 schema 暴露语义模糊的 bool 参数。`delete_path` 使用 `mode: "file" | "emptyDirectory" | "recursiveDirectory"`；`copy_path` 和 `move_path` 使用 `collision: "failIfExists" | "overwrite"`。运行时仅为旧历史或人工输入兼容读取旧 `recursive` / `overwrite` 字段，新请求和工具描述不得继续暴露这些 bool 字段。

## Studio 展示

Studio timeline 以 item-first 数据为准。工具 entry 和工具组详情必须显示工具名称、状态、关键路径或命令摘要。静默文件工具的成功结果可以隐藏；但失败、拒绝、中断和预算受限时必须展示 result/error 详情，避免用户只看到“工具调用失败”而无法定位原因。
