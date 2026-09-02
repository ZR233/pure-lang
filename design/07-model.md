# 07 - 模型层

## 7.1 职责

`pl-model` 是模型目录、Provider endpoint 与模型协议适配层，负责把核心层的统一请求转为具体 API 请求，并把流式结果归一化为 provider 无关的模型事件和 `CompletionResponse`。`pl-core` 是宿主唯一使用的高层 runtime facade；产品不直接构造 provider、transport 或 stream accumulator。

`pl-model` 不维护产品 agent/session 历史，不解析 CLI，也不决定产品阶段。它只维护
`ModelSession`：与一个 `AgentSession` 同生命周期的物理连接、脱敏 fingerprint 和
Responses continuation 状态。
`pl-model` 可以消费已经解析好的自定义模型列表，但不读取配置文件。

内部按四个职责组织：

- `model`：模型元数据、能力、参数、transport、价格和内置目录。
- `provider`：唯一的 Provider 配置、preset/catalog 与解析后的 endpoint。
- `completion`：provider 无关的请求、响应、工具、usage、compaction 和 canonical model event。
- `runtime`：绑定一个已解析模型的 `ModelRuntime`，以及唯一 OpenAI-compatible HTTP/SSE/WS adapter。

`runtime` 内部的 OpenAI-compatible codec 只负责把 provider 私有 wire event 映射为
`completion` 的 canonical event。OpenAI Responses WebSocket、Responses HTTP/SSE、OpenAI
Chat、DeepSeek 和 Zhipu/GLM 兼容接口在进入唯一 accumulator 前必须统一为 response
started/id、文本、思考、reasoning summary、工具参数、工具 ready/done、usage 和完成/失败事件。
核心层、Studio timeline 和外部集成方不解析 provider 原始 JSON。

同一协议族的 usage alias、cache details、reasoning details 与工具 identity 只能由
`runtime/openai` 的共享 typed normalizer 解释一次。SSE、WebSocket 与仅供 fixture 使用的非流式
response parser 只负责提取各自 envelope，不能各自维护字段优先级或 fallback；同一 usage/tool
fixture 经过不同 transport 入口必须得到完全相同的 canonical `TokenUsage` 与工具身份。

底层 event stream、decoder 和 accumulator 只服务 `ModelRuntime` 与 `pl-core` 的 trace projector，不是宿主 API。外部宿主通过 `pl-core::TurnEngineBuilder::from_route` 或 `pl-core::ModelTurnClient::from_route` 执行模型请求，避免复刻 provider adapter。

`completion` 内部 stream 状态机负责稳定工具调用 identity。工具调用一经解码即具有必填的
`ToolCallIdentity { item_id, call_id }`：Responses 使用事件携带的 `item.id` 与 `call_id`，
两者都缺失是协议错误；Chat Completions 以 chunk index 构造 `stream_id` / `item_id`，并确定性
赋 `call_id = item_id`。不存在 optional `call_id` 或 id 与 call_id 互为 fallback 的规范化路径。
Responses 可能先发送只有 provider `item_id` 的 `output_item.added`，后续 delta 或 done 才补
独立 `call_id`；同一个工具调用一旦通过 `stream_id`、`item_id` 或 `call_id` 中任一非空身份
进入 accumulator，后续 late metadata 必须升级原 accumulator 并合并到同一个 open tool，
不得拆成第二个 tool call 或第二个 trace part。trace 的 tool part id 以最早稳定的 provider
item/runtime tool id 为锚，`call_id` 只作为 metadata 写入 tool snapshot，用于协议回放和
provider tool result 匹配。

## 7.2 依赖

```text
pl-protocol
    ↑
pl-model
    ↑
pl-core
```

`pl-model` 的公共消息和错误边界依赖 `pl-protocol`，内部流式事件边界依赖 `pl-trace`：

- `Message`
- `PureError`
- `Result`
- `ToolSpec`
- `pl_trace::AgentEventSender`

provider 适配实现可以依赖 `async-openai`、`reqwest`、`tokio-tungstenite` 和 `serde`。这些依赖只用于 `pl-model` 内部 transport、typed protocol request 和 typed stream event 解析，不向 `pl-core` 暴露。

## 7.3 模型能力

`ModelInfo` 的能力声明使用结构化能力矩阵，不再使用 bitflag 或旧的 `input_modalities` 列表作为主协议。配置和运行时只接受新的 `capabilities` 对象：

- 基础能力：`streaming`、`temperature`、`reasoning`、`web_search`。
- 输入/输出模态：`input`、`output`，取值为 `text`、`image`、`audio`、`video`、`pdf`。
- 工具能力：`function_calling`、`parallel_tool_calls`、`custom_tools`、`freeform_tools`、
  `programmatic_tool_calling`。
- 推理交错字段：`interleaved.field`，当前支持 `reasoning`、`reasoning_content`、`reasoning_details`。

`pl-core` 只读取这些 provider 无关能力来做本地校验和 UI 展示：图片输入必须要求模型声明 `input = ["image"]`，工具调用必须匹配工具能力，推理请求必须匹配 `reasoning = true`。provider 私有差异不扩散到 `pl-core`。

提示词缓存不是由模型 slug 隐式推断的基础能力。`ProviderServiceCapabilities.prompt_cache`
声明 endpoint 的缓存 dialect，模型目录声明该模型是否报告缓存写入 token；核心层把 provider
声明、wire protocol 与模型声明合成为穷尽的 `EffectivePromptCachePolicy`。未声明能力的自定义
Responses/Chat endpoint 默认不发送任何缓存专属字段。

模型级 provider override 使用 `ModelRequestProfile` 表达，包括 `api_model`、`headers`、`body`、`chat_parallel_tool_calls`、`responses_programmatic_tool_calling`、`max_tokens_field` 和 `responses_max_tokens_field`。`body` 作为唯一的动态 base body 注入请求体（如 DeepSeek 固定的 `thinking.type = enabled`）；不再保留未被 wire 消费的通用 `options` 袋。其余可变字段（如 effort 透传的 `reasoning_effort`、GLM `thinking.clear_thinking`）由 `ModelInfo.parameters` 声明驱动（见 7.8）。这些字段只由 `pl-model` 的 provider adapter 消费；核心编排层不得读取或拼接这些私有字段。Chat Completions 只有在模型 profile 显式声明 `chat_parallel_tool_calls = true` 时才发送 `parallel_tool_calls`，并把核心层本轮计算出的 `true` 或 `false` 原样写入；未声明的 OpenAI-compatible endpoint 默认省略该字段。Programmatic Tool Calling 同时要求模型能力、Responses profile 和 endpoint 服务能力；不满足时不向该 agent 注册 hosted tool。Chat Completions 的最大输出 token 字段默认写入 `max_tokens`；OpenAI-compatible provider 若要求新字段（如 MiMo 的 `max_completion_tokens`）可在模型 profile 中声明。Responses endpoint 默认不发送最大输出 token 字段，以匹配 Codex 常规 Responses 请求；Responses-like 代理若要求限制字段，可在模型 profile 中把 `responses_max_tokens_field` 设置为 `max_output_tokens`、`max_tokens` 或 `max_completion_tokens`。

## 7.4 Provider 与 runtime

当前所有支持的供应商共用一种 OpenAI-compatible 协议族，因此不建立 `ModelProvider` trait、厂商 runtime 子类或共享 provider wrapper。`ResolvedModelRoute` 解析出唯一的 `ProviderEndpoint + ModelInfo`，`ModelRuntime` 在构造时绑定该模型；后续请求不再携带 model，provider 也不保存 default model 或完整模型目录。

`ProviderConfig` 是持久化配置和 catalog binding 的唯一来源；`ProviderEndpoint` 只包含运行时 endpoint、解析后的凭证、headers、tool wire policy 与服务能力。protocol 和 connection mode 只来自绑定模型的 `ModelTransportProfile`，避免 provider 与 model 两份事实漂移。
所有自定义 OpenAI-compatible endpoint 使用同一个通用构造入口；不能按 Responses/Chat 名义复制
只改名的构造器。模型能力只由 `ModelInfo.capabilities` 表达，endpoint 仅叠加真实的服务约束，
runtime 不再维护一份始终为全能力的 provider bitflag 或异步空操作 credential façade。
运行路径不匹配 OpenAI、DeepSeek、Zhipu、MiMo 等 ID，也不为这些厂商建立穷尽枚举分发。
未来只有在实际支持第二种协议族时才引入新的 typed codec；当前不保留 Anthropic 占位或预先抽象。

每个 provider 实例保存 preset 身份、endpoint override、凭证、headers、tool wire policy、
服务能力和 catalog binding；具体模型由角色 route 选择，不保存第二份 provider default model。
模型目录可以按 slug 保存当前连接方式 override。reasoning/thinking/effort
的 wire 规则由模型 `parameters` 声明驱动（见 7.8）。模型目录通过
`ProviderConfig::effective_models()` 解析，不从全局列表兜底。

OpenAI-compatible 自定义模型必须显式提供完整 `ModelTransportProfile`。Chat 模型只能声明
`ChatCompletions + Http`；Responses 模型可以声明 `Responses + Http`，或同时支持 HTTP/WS 并指定
默认模式。具体 base URL、headers、tool wire policy 与 endpoint 能力由 provider 提供。

Zhipu Coding Plan 是 catalog preset，默认使用 `https://open.bigmodel.cn/api/coding/paas/v4`，
并引用 Zhipu 模型目录；它不新增 runtime 变体。MiMo API 与 Token Plan 同样是两个 preset，
共同引用一个 `mimo` catalog。

## 7.5 Protocol API

`pl-model` 当前支持：

- Responses API
- Chat Completions API

不同 protocol API 的差异保持在 `pl-model` 内部，核心层只看到绑定模型的 runtime、精简后的 completion request/response 和 provider 无关的 model event。

所有当前 preset 都复用 `runtime` 私有 OpenAI-compatible codec 的两种 wire API。协议与连接模式是正交维度：
Responses 支持 `web_socket | http`，Chat Completions 只支持 `http`。协议由模型的
`ModelTransportProfile.protocol` 声明，不再由 provider 实例统一决定。内置 OpenAI preset 的所有模型
使用 Responses，模式顺序固定 WS、HTTP，默认 WS；选择 HTTP 时仍调用 `/responses` 并消费 SSE，
绝不切换到 Chat Completions。DeepSeek 内建模型（Flash 与 Pro）使用 Responses HTTP；MiMo、Zhipu
使用 Chat Completions HTTP。同一 provider 实例下的不同模型可以使用不同协议。

`ModelTransportProfile` 是 `ModelInfo` 的必填字段，包含 `protocol`、
`supported_connection_modes` 与 `default_connection_mode`。provider 的模型目录可按模型 slug 保存
`connection_overrides`；解析后的模型把 override 投影为本次请求的最终连接方式。Chat + WS、空支持
列表、默认模式不在支持列表，以及 override 指向未知或不支持模式的模型，都在配置加载/保存时拒绝。
Web 与 Flutter 只渲染模型目录返回的 transport 和当前 override，不按 preset ID 推断。

内建矩阵固定为：全部 GPT 使用 Responses，支持 WS/HTTP且默认 WS；DeepSeek V4 Flash、V4 Pro 与
V4 Flash Vision Exp 使用 Responses/HTTP；全部 GLM 和全部 MiMo 使用 Chat Completions/HTTP。
runtime 必须按当前模型选择对应 endpoint path，同一 provider 实例可以路由不同协议的模型。

Responses WebSocket 使用 `/responses` 握手和 `response.create` 帧，并强制发送 `store: false`；continuation 只依赖当前物理连接，不能把响应持久化到供应商侧。物理连接属于 `AgentSession` 持有的 `ModelSession`：同一会话跨 turn 复用，不同会话绝不共享，持久化恢复后重新建立。模型目录中该模型的 WebSocket 选择表示首选连接模式；尚未产出 canonical 流事件的一次完整历史重放仍遇到瞬态 WS 错误时，当前 `ModelSession` 必须熔断到 Responses HTTP，并在该 session 后续 turn 保持 HTTP，避免重复发送大体积完整历史。这个运行期 fallback 不修改持久化模型 override，新建、fork 或持久化恢复后的 AgentSession 会重新尝试用户选择的 WebSocket。WS session、HTTP fallback 与 transport fingerprint 必须同时包含模型 slug、模型协议和最终连接方式，避免同一 provider 下不同模型共享错误状态。`previous_response_id` continuation 只在 WebSocket 模式启用，因为该状态与物理连接绑定；HTTP/SSE 始终发送完整 canonical history，不依赖连接级 continuation。

WebSocket 建连通过系统 DNS 解析全部目标地址，并以 250ms 间隔交错竞争 IPv4/IPv6；首个成功的 TCP 连接继续使用原始域名完成 SNI、证书校验和 WebSocket 握手。单次完整握手保持 15 秒上限，超时保留 transient 分类并进入同一 WS 模式的一次完整重试。无效 continuation 也必须在尚未产出 canonical 流事件时退出当前流并消费这同一个重试预算，由外层在新 WS 上发送完整历史；transport 内部不得再嵌套第二套 full replay。收到首个 canonical 流事件后，任何断线或 provider 失败都直接返回原错误，不重放当前请求；若该失败仍是瞬态 WS 故障，则必须熔断当前 session 的 WS，使宿主显式创建的下一 Turn 使用 Responses HTTP 完整历史继续，而不能再次连接同一不稳定 WS 路径。可重试失败采用带 0.9–1.1 稳定抖动的有界指数退避，provider `Retry-After` 优先且不加抖动。唯一 WS 重试仍失败时立即启用 session-scoped HTTP fallback，不继续制造 full replay 风暴；日志和 inference diagnostics 必须记录 fallback 原因、作用范围与来源连接模式。

effort 等可调参数的 wire 写入由通用透传机制驱动，协议层不再为每供应商硬编码 reasoning/thinking 映射。`build_request` 接收当前 `ModelInfo`，先序列化强类型核心字段（model、messages、stream、tools 等）为 JSON 对象，再依次注入：base body（`ModelRequestProfile.body`，如 DeepSeek 固定的 `thinking.type = enabled`），以及 parameter wire（用户选中的候选值按模型 `parameters` 声明写入或移除字段，见 7.8）。覆盖优先级为 parameter wire > base body > 协议默认字段。

OpenAI Responses 的 `reasoning.summary` 仍按 Codex wire 语义发送（`Auto` 和兼容层的 `Enabled` 都发送 `auto`，`Disabled` 不发送 summary 字段），由 `ReasoningConfig.summary` 独立驱动，不进入 parameter wire。模型返回的 `reasoning_content` 进入 canonical reasoning event；历史回放时仍通过 assistant message 的 `reasoning_content` 字段写回 Chat Completions。

核心层提交的 `CompletionRequest` 始终带完整 canonical input，且不携带 model、stream、store、previous response、trace 或 transport session。runtime 固定使用流式请求；Responses 固定 `store: false`。prompt cache 和 trace 属于单次 invocation context，continuation 只由 `ModelSession` 管理。`pl-core` 的宿主 façade 在 invocation 内创建事件 sink；`ModelTurnOptions` 只承载宿主可控的取消状态，不暴露 `pl-trace` 类型。
`ModelSession` 在相同连接和 fingerprint 下由上次完整请求前缀计算增量，在 transport
内部克隆请求并只对 Responses WebSocket 帧设置 `previous_response_id`。断线、取消、未完整消费、配置变化或无效 continuation
都会关闭旧连接；只有首个 canonical 流事件前的瞬态失败可在新 WS 上用完整历史重试一次。该重试
仍失败时，同一 session 从当前请求开始切换到 Responses HTTP，不再创建新的 WS full replay；已经
产出事件的请求不得切换 transport 后重放，但瞬态 WS 失败仍会把 session 的后续 Turn 熔断到 HTTP。
Responses HTTP/SSE 和 Chat Completions 始终发送完整历史。

`CompletionRequest.messages` 中的 `MessageRole::System` 表示本轮临时前置指令或开发者上下文。Responses endpoint 序列化为 input message role `developer`，避免发送不被部分 Responses 兼容服务接受的 `system` role；Chat Completions 仍序列化为 `system` role。

`CompletionRequest.tools` 使用 `pl_protocol::ToolSpec`，它是 provider-neutral 的唯一 wire 事实。
每个 model step 携带当前 `ToolPlan` 的完整可见工具列表；OpenAI adapter 只把 frozen specs 转为
Responses/Chat typed body，不自行发现、过滤或注入 agent 工具。

Core 的运行时工具统一为 `DynTool`，内部持有 `Arc<dyn ToolExecutor>`；definition、policy、execution
owner 与 executor 在注册时组成一个不可拆分的执行对象。Rust 内置工具和宿主静态工具实现
`StaticTool`，由 `From<T: StaticTool> for DynTool` 经过 typed adapter 擦除；MCP、插件和 hosted
adapter 直接实现对象安全的 `ToolExecutor`。`ToolPlan` 只冻结 `DynTool`，不存在按 builtin、MCP、
插件或 hosted 来源分派的第二条执行链。provider 只能看到从 `DynTool::definition()` 投影出的
`ToolSpec`，看不到 handler、policy、group generation 或注册来源。

`pl-core` 的 crate 根同时公开工具契约、typed builder、安装组，以及文件、命令、Git、Skill、LSP、
控制和搜索等可复用内置工具的实现类型与构造入口。下游可自由选择其中任意子集注册；默认 installer
只是这些公共工具的预设组合，不能依赖私有构造器或维护另一份 registry。

工具名称在 core 内使用 `ToolName { namespace, name, wire_name }`。registry 以稳定 wire name 接收
provider 回传调用，同时保留结构化 identity；MCP 的 wire name 继续使用既有
`mcp__<server>__<tool>` 规则，不能从归一化后的 wire name 反解析执行目标。trace、历史和现有 UI
协议继续记录 wire name 字符串。

注册组显式声明 `ToolExposure::Direct | Deferred`。builtin、LSP、控制类和普通宿主静态工具默认
Direct；MCP、插件和 App 动态目录默认 Deferred。若当前冻结目录存在 Deferred 工具，core 以普通
function tool 形式加入 `tool_search`；搜索结果通过 typed runtime directive 更新当前
`AgentSession` 的 revealed identities，匹配工具从下一 model step 开始进入完整 `ToolSpec` 列表。
该机制是 provider-neutral 的普通工具调用，不使用 Responses 私有 hosted tool-search wire，Chat 与
Responses 继续消费同一 `CompletionRequest.tools`。revealed 状态只在当前 AgentSession 持久化，
子 agent 默认从空状态开始；deferred definition 或 source generation 变化会使旧 reveal 失效。

每个安装组可以提供集中式 developer instructions。只有本 step 最终可见且通过执行策略过滤的组才
注入其说明；组说明与工具 definition/executor 来自同一个冻结 `ToolPlan`。说明内容参与固定 prompt
section hash，但 group identity、注册顺序和 executor generation 仍不参与 tool wire fingerprint。

Programmatic Tool Calling 通过 hosted `programmatic_tool_calling` 与工具的 `allowed_callers` 声明。
首期只允许稳定本地读工具、LSP 查询和 effect 被可信配置明确标为 Read 的 MCP；命令、文件写入、
Git mutation、审批/交互和 agent-control 始终只能 direct 调用。结构化 eligible 工具必须携带
`output_schema`。嵌套 function/custom call 的 `caller` 在结果回传时原样保留。由于 runtime 固定
`store: false`，session 必须按 provider 顺序持久化 reasoning、program、嵌套 call、
call output 与 program output，并在 HTTP 重放、WebSocket full replay 和恢复后完整重建；Chat
Completions 遇到这些 Responses 原生 item必须显式拒绝，不能降级成普通 assistant/tool message。

Responses hosted tools 属于 endpoint 服务能力，不由 URL 字符串在运行时猜测。官方 OpenAI preset
的 canonical `base_url` 可以声明 `programmatic_tool_calling`；preset 实例覆盖为自定义 `base_url`
时默认关闭，自定义 provider 也默认关闭。只有 manager 在当前 agent scope 中注册了对应
`ProviderHosted` Tool，provider adapter 才发送 hosted tool type。核心编排必须同时检查模型能力、
模型 request profile 和 endpoint 服务能力，任一缺失都拒绝冻结该 hosted Tool。该边界保证 OpenAI-compatible
Responses endpoint 不会因未支持的 `programmatic_tool_calling` 返回 400。

provider transport 层把第三方 API 错误统一转换为 `PureError` 时必须先脱敏。错误文本中不得包含 bearer token、API key 或形如 `sk-...` 的密钥片段；鉴权失败、配额不足、模型不存在等服务端错误可以保留 status、错误类型、code 和可读原因，但密钥值必须替换为稳定占位。
Responses HTTP/SSE 与 Chat Completions HTTP 必须和 WebSocket 一样，用 Serde typed error DTO 保留结构化 provider code、HTTP status、message 与可选 retry hint；请求级 HTTP 错误、SSE `response.failed` 与顶层 `type:error` 必须进入同一个分类器，进入控制流后不得把 DTO 降级成待解析字符串。408/409/425/429、5xx、建流前的瞬态网络错误以及 `server_is_overloaded` 等容量错误只允许在流对象建立前最多重试两次，并采用同一有界指数退避与抖动；一旦 HTTP 流已经产生首个可见或 canonical 事件，transport 不得因为后续流错误自动重放并制造重复输出；尚未产生任何事件的流中断（如连接被掐断、响应体解码失败）按瞬态处理，允许在既有重试预算内完整重放，与 13 的运行时错误分类一致。WS 切换 HTTP 后使用独立的 HTTP 重试预算，但不会再回到 WS；因此仅在两个 transport 都未产出流事件的最坏情况下，单次请求最多产生两次 WS 发送和三次 HTTP 发送。

`ProviderFailureKind` 固定为 authentication、authorization、capacity、configuration、transport、
protocol 与 unknown。`RetryDisposition` 只回答同一次模型请求是否能在尚无副作用时安全重放，
不得被宿主解释为 Task 生命周期。401/`invalid_api_key`、403、无效模型/endpoint/请求配置、
provider 协议错误和未知永久错误均保持 permanent；408/409/425/429、5xx、连接与超时错误保持
retryable。Studio 另行从 typed failure 派生 Task disposition，任何层都不得解析 message 或 code
字符串来决定 Task 是否终结。

提示词分层由 `pl-core` 决定，`pl-model` 只消费已经组装好的 `CompletionRequest`。`CompletionRequest.instructions` 表示 base/system 层，并在 Responses 和 Chat Completions 请求中作为最前面的 system 内容发送。`messages` 可以包含核心层临时插入的 system/user 前置消息；`pl-model` 不区分它们是否来自 developer 或 user context，也不把任何提示词写回会话。

请求体不再由散落的 `serde_json::json!` 直接拼接，而是先转换为 `pl-model` 内部强类型 request，再由 serde 序列化。动态 JSON 只允许出现在 JSON Schema、工具参数、provider 返回的任意 JSON 参数和协议扩展 escape hatch。

`CompletionResponse` 只保留 canonical 内容、reasoning、tool calls、Responses context、usage、orchestration、
实际模型和可选 inference timing。raw text、finish reason、hosted-search 汇总、trace event 与 sequence
不再重复存入 response；trace 和 hosted-search 状态分别由 `pl-core` projector 与 canonical
context/event 维护。

inference timing 由 `ModelRuntime` 在 canonical stream 边界使用单调毫秒时钟测量，不能由 provider
adapter、核心层或 Flutter 估算。`startedAt` 在一次逻辑 inference 首次发送前确定；transport retry、
WebSocket full replay 与 HTTP fallback 继续属于同一次 inference，等待时间计入 TTFT，最终成功只
产生一份 timing。`firstTokenAt` 只由首个非空 text、reasoning 或 tool-input delta 确定，
`ResponseStarted`、block open 和空 delta 不计；`completedAt` 只在 canonical 成功完成时确定。
`TTFT = firstTokenAt - startedAt`，`decodeMs = completedAt - firstTokenAt`，
`t/s = completionTokens × 1000 / decodeMs`。completion token 已包含 reasoning token，不得重复相加；
timing、usage 不完整或 `decodeMs == 0` 时不生成吞吐样本。失败与取消不携带成功 timing。

真实验收 harness 可在显式启用 wire capture 时额外记录少量 transport 阶段回执：
请求已落盘、HTTP 流已建立、首个 provider 事件以及流终止或失败。回执使用同一
capture id、单调 elapsed 和墙上时间，不记录 prompt、密钥或 provider 错误原文；它只用于
区分请求组装、响应头等待与 SSE 等待的耗时，不改变生产超时、重试或成功 timing
语义。wire capture 顶层可同时记录 invocation trace 的 `sessionId`、`turnId` 与
`inferenceId`；这些字段只用于把多次 inference 可靠归属于同一会话，不进入 provider 请求体，
也不从提示词正文或工具行为反推 Agent 身份。普通运行未提供 trace 时省略这些字段。仅出现请求
落盘回执不能被解释为 provider 已接收或已返回响应。

## 7.6 多模态消息

`MessageContent` 只有一个有序 multipart 形态：`parts: Vec<ContentPart>`。持久协议仅允许
`ContentPart::Text` 与 `ContentPart::Attachment { attachment_id, modality }`；attachment modality
为 image、video 或 file，PDF 属于 file。持久消息不得保存本地路径、外部 URL、Base64、provider
file id 或请求期 data URL，也不保留 text/multipart 双形态。

`ModelCapabilities.input` 使用 `ModelInputCapability`，同时声明 modality、允许的输入来源与
格式/数量、单项字节、批次总字节和图片宽高限制。`ModelRequestProfile.media` 为每种 modality 声明
有序的发送表示、provider wire 映射和混合规则；表示是封闭枚举，例如 `RemoteUrl`、`ProviderFile`
与 `DataUrl`。能力声明
必须至少有一条当前输入可用的首发路线和一条基于持久快照的重放路线；未知模型或缺少完整 profile
的 modality 按不支持处理，不按模型名、provider 名或 wire 宽松程度推断。

`pl-core` 在进入 provider adapter 前把稳定 attachment ref materialize 为 `pl-model` 私有的
`PreparedContentPart`。它只携带已校验 bytes、当前首发允许使用的瞬时 URL 或 provider file id；
`pl-model` 不读取 Studio 存储，也不解析本地路径。同一 modality 批次选择同一种表示，provider
文件上传失败只能在推理请求发出前整批切换到下一条 profile 路线，流建立后不得自动重发。

代理主动读取图片使用独立的 `view_image` 工具和 `ModelContextItem::ToolMedia`。MCP typed image
result 只有在调用它的精确模型同样声明 image 输入、完整快照 replay profile，且当前 Thread 安装
attachment runtime 时才进入相同通道；否则图片块只产生有界诊断文本，不持久化为仅供 UI 使用的
附件。工具成功结果仍先以
普通 typed tool result 闭合 provider tool call；同一批次的全部结果闭合后，core 再追加一个按
call 顺序排列的 `ToolMedia`，每项只保存 call id、安全展示标签与 thread-owned attachment metadata。
它不是用户消息，也不进入用户 Timeline；Responses 与 Chat adapter 都把该上下文投影为一个内部
user multipart，并按「标签文本、图片」顺序发送。这样 Chat 的并行 tool message 保持连续，同时
两种协议复用同一份 durable history。图片 bytes 仍由宿主 attachment loader 在请求期 materialize，
同 Turn 后续 inference、失败重试和恢复不得读取原始 workspace 路径。

`AttachmentRuntime` 是 core 与宿主之间唯一的附件运行时边界：batch writer 在工具结果进入 history
前原子提交同一结果中的全部规范化图片并返回有序 opaque metadata，单图写入只是该边界的便利入口；
loader 按 attachment id 批量返回受限的
`MaterializedAttachment`。没有完整 image capability、快照 replay profile、writer 或 loader 时
不得注册 `view_image`，stale 调用也必须在文件 IO 前拒绝。工具图片、对应 tool results 与前导
assistant tool calls 在 compaction、rewind 和恢复校验中是一个不可拆分单元。

MCP image content 在持久化前必须先检查编码长度、严格 Base64 解码、校验声明 MIME 与真实文件头，
再复用 `view_image` 的格式、解码、尺寸和模型限制。一个 MCP result 的图片批次任一项无效或写入
失败时整批不发布；`isError` result 在图片解码和写入前短路。tool result、trace、audit 与 SQLite
不得保存 typed image 的原始 Base64，只保留有界占位文本、摘要、尺寸、MIME 与 attachment id。

OpenAI Responses 的已实现图片路线使用 `input_image`；OpenAI Chat 使用 `image_url`。Zhipu Chat
codec 还定义 `video_url` 与 `file_url`，但模型只有在精确请求契约、限制与快照重放路线都经过验证后
才声明对应能力。GLM-5.3-Flash 当前只声明 text/image：远程图片首发优选 URL，本地图片以及历史、
重试和恢复统一使用 Data URL。未声明相应 modality 的模型必须在任何附件 IO 或凭据读取前拒绝。

DeepSeek V4 Flash Vision Exp 当前声明 text/image，并通过 Responses `input_image` 发送：远程图片
首发优选 URL，本地图片、历史、重试和恢复使用 Data URL。支持的快照格式固定为 JPEG、PNG、GIF、
WebP。官方接口还提供 Files API，但 Pure 在 provider file 上传、瞬时 file id 与快照回放生命周期
全部实现前不声明该表示。官方对少于 15 张与至少 15 张图片使用不同边长上限；canonical profile
选择全批次均可成立的 4096 像素保守上限，并以 32 MiB snapshot 批次总字节上限保证 Data URL
重放不会越过接口的 48 MiB 请求体边界。该保守子集不按模型名在 adapter 中特判。

## 7.7 自定义模型

产品宿主使用 serde 读取自己的完整配置，调用 `pl-core::AgentModelConfig::validate/resolve` 后，
只把 `ResolvedModelRoute` 交给 `pl-core::TurnEngineBuilder::from_route` 或
`pl-core::ModelTurnClient::from_route`。宿主不直接调用 `pl-model`；`pl-core` 与 `pl-model` 都不读取
`~/.pure/config.toml`。

Bundled catalog 只读，配置只能通过 `additional_models` 追加不冲突 slug；完全自定义 provider
使用 `Explicit { models }`。附加与显式模型都必须声明 transport；模型目录的
`connection_overrides` 只保存当前模式选择，不修改模型声明的支持矩阵。`used_fallback` 仍是运行时
状态，不从配置读取。

模型信息中的 `base_instructions` 是模型级基础提示词来源，进入 `pl-core` 的 instruction assembler；配置中的 `[instructions].base_override` 可以完整替换它。模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、历史保留、原子替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。

`CompletionRequest.input` 使用 provider 无关的有序 `ModelContextItem`，包括普通 `Message` 和专用 `Compaction { encryptedContent }`；`.messages(...)` 只是不含 checkpoint 的便捷构造器。Responses request 可以把 compaction item 映射为原生输入，Chat Completions 必须明确拒绝。绑定单模型的 `ModelRuntime::compact_context` 接收 instructions、有序上下文、工具、parallel tool calls、reasoning 和 prompt cache key，并返回经过 provider 解析的上下文项与可选 usage。远程协议能力由绑定模型的 `ProviderWireProtocol::Responses` 决定，不依赖 preset ID；远程 compaction 固定走独立 HTTP 请求。

## 7.8 模型可调参数

effort（推理强度）不再是固定的全局枚举，而是「模型声明的可调参数」。该机制是通用的——effort 是首个应用，类型设计可容纳未来 thinking、verbosity 等参数。各供应商自由定义候选值域，并由模型自身声明选中值如何写入 API 请求体，协议层据此通用透传，不包含任何供应商特定代码。

模型目录独占参数的候选值、显示名、默认候选与 wire 规则；角色路由只保存当前选择，
不得复制或重新定义候选。模型声明非空 effort 候选时，产品角色必须保存其中一个候选；
模型没有声明 effort 参数时，角色选择必须为空，`CompletionRequest` 不携带 effort，最终
请求体也不得制造默认字符串或字段。当前选择从角色路由进入统一 `ReasoningConfig`，
Responses 与 Chat Completions 均只由下述 `ParameterWire` 写入供应商请求体。

`ModelInfo` 的 `reasoning_efforts: Vec<String>` 字段被替换为：

- `parameters: Vec<ModelParameter>`：模型声明的可调参数列表。

核心类型签名（wire 使用 `camelCase`，Rust 侧 `snake_case`）：

```rust
pub struct ModelParameter {
    pub name: String,                          // 参数键，effort 的 name = "effort"
    pub label: Option<String>,                 // 面向用户的显示名，缺失回退 name
    pub candidates: Vec<String>,               // 候选值域，首项为默认值
    pub wire: BTreeMap<String, ParameterWire>, // 每个候选值 → 写入规则
}

pub struct ParameterWire {
    pub set: Vec<WireAssignment>,   // 设置字段（dot 路径）
    pub remove: Vec<String>,        // 移除字段（dot 路径）
}

pub struct WireAssignment {
    pub path: String,   // 嵌套路径，如 "reasoning.effort"、"thinking.type"
    pub value: String,  // 透传的字符串值
}

impl ParameterWire {
    pub fn apply_to(&self, body: &mut serde_json::Map<String, Value>);
}
```

`wire` 用 `BTreeMap<String, ParameterWire>` 而非动态 `Map<String, Value>`，避免运行时反序列化并保持 TOML 友好。`apply_to` 按 dot 路径逐层写入或移除嵌套 JSON 对象字段；移除不存在的字段静默忽略。

各供应商的 effort 声明形态：

| 供应商 | candidates | wire.set（选中值 → 字段） | wire.remove |
| --- | --- | --- | --- |
| OpenAI（GPT-5.5 / GPT-5.4 / GPT-5.4-Mini） | `medium` / `low` / `high` / `xhigh` | `reasoning.effort` = 值 | — |
| OpenAI（GPT-5.6 Sol） | `low` / `medium` / `high` / `xhigh` / `max` | `reasoning.effort` = 值 | — |
| OpenAI（GPT-5.6 Terra / Luna） | `medium` / `low` / `high` / `xhigh` / `max` | `reasoning.effort` = 值 | — |
| DeepSeek | `high` / `max` | `reasoning_effort` = 值（`thinking.type = enabled` 作为 base body） | — |
| Zhipu 普通 | `enabled` / `none` | `thinking.type` = 值 | — |
| GLM-5.2 | `high` / `max` / `none` | `high`/`max`：`reasoning_effort` + `thinking.type = enabled` + `thinking.clear_thinking = false`；`none`：`thinking.type = disabled` | `none` 移除 `reasoning_effort` |
| GLM-5.3 / GLM-5.3-Flash | `high` / `low` / `max` | 三档均为 `reasoning_effort` + `thinking.type = enabled` + `thinking.clear_thinking = false` | — |
| MiMo | `enabled` / `disabled` | `thinking.type` = 值 | — |

GLM-5.2 的「一个选择联动多个字段」和「none 时移除字段」由 wire 的多条 `set` 与 `remove` 完整表达，无需协议层特判。GLM-5.3 系列始终启用思考，不提供禁用思考的 `none` 候选；effort 选择只改变 `reasoning_effort` 值。

`ModelInfo` 提供 helper 避免调用点手动遍历参数列表，复用于配置校验、默认角色补齐和 GUI 渲染：

- `effort_parameter() -> Option<&ModelParameter>`
- `supported_efforts() -> Vec<String>`
- `default_effort() -> Option<String>`

## 7.9 模型家族预设

同供应商的模型共享大量元数据（capabilities、truncation_policy、effort 参数声明、base body）。`default_models` 不再为每个模型独立构造完整 `ModelInfo`，而是用 `ModelFamily` 预设封装共享部分，具体模型仅以差异字段实例化。

类型签名：

```rust
pub struct ModelFamily {
    pub id: &'static str,
    pub capabilities: ModelCapabilities,
    pub truncation_mode: TruncationMode,
    pub truncation_limit: u64,
    pub parameters: Vec<ModelParameter>,
    pub request_profile: ModelRequestProfile,
    pub base_instructions: String,
}

impl ModelFamily {
    pub fn instantiate(
        &self,
        slug: &str,
        display_name: &str,
        description: &str,
        context_window: u64,
        max_context_window: u64,
        max_output_tokens: Option<u64>,
        pricing: ModelPricing,
    ) -> ModelInfo;
}

pub struct ModelPricing {
    pub currency: Option<String>,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub cache_read_per_mtok: Option<f64>,
}
```

`pl-model` 的内建 family 预设按供应商与模型线划分：`openai_family`、`openai_gpt56_family`、
`deepseek_family`、`deepseek_vision_family`、`mimo_family`、`zhipu_text_family`、
`zhipu_glm52_family`、`zhipu_glm53_family`、`zhipu_glm53_flash_family` 与
`zhipu_vision_family`。共享能力矩阵由 `openai_capabilities` / `deepseek_capabilities` /
`deepseek_vision_capabilities` / `mimo_capabilities` / `zhipu_capabilities` 构造并供各 family 复用；
family 之间的差异集中在 effort 候选值域、request profile 与 typed input capabilities。
DeepSeek Vision Exp 复用 V4 Flash 的 effort、thinking、上下文与计费事实，只增加经过官方文档确认的
Responses image profile。GLM-5.3 与 GLM-5.2 复用同一条「启用思考」wire 组合，差异只在候选值域：
GLM-5.3 为 `high` / `low` / `max`，且不提供禁用思考候选。GLM-5.3-Flash 复用 GLM-5.3 的始终思考
wire 与候选值域，并声明 image 的 local/data-url 与 remote-url/snapshot 路线；不得从相邻视觉模型
推断 video/file 能力。

## 7.10 Prompt 缓存

核心层按 prompt generation 组装请求，唯一顺序是：模型基础指令、平台与全局配置、模式与
角色、Skill、Workspace/项目文档组成的固定 instructions 与 prelude，随后是 durable model
transcript，最后附加至多一条本 Turn 冻结的 working-context message。model、instructions、
tool choice、reasoning、输出 schema 和 service tier 在同一 generation 内不得变化；工具由每个
model step 冻结的 `ToolPlan` 提供，列表不变时必须序列化为 byte-identical 前缀，列表变化时形成
新的 request header；
transcript 只包含 user、assistant、tool result 与 provider compaction checkpoint。pinned sections、
Evidence Ledger、session note 和 prompt generation 状态属于可替换 `AgentWorkingState`，不得作为
append-only `ModelContextItem` 写入 transcript。

每个 Turn 在首个 inference 前从 `AgentWorkingState` 冻结一次模型可见 working context；同一
Turn 后续 inference 只追加 transcript/tool result，不因 Todo、receipt 或其他 working state 更新
替换已发送 input。Evidence Ledger 继续有界持久化，但不进入 provider request、token 估算或
context hash；完整工具结果已在 durable transcript 中提供模型恢复所需事实。其他 pinned section
在下一 Turn 才以最新版本重新冻结。working context 内容变化只更新 context hash，不提升 prompt
generation；provider、model、固定指令、工具 schema 或 compaction 变化才提升 generation。
这样 Responses WebSocket continuation 能保持严格追加前缀，固定前缀与 transcript 也能复用
provider prompt cache。

上下文压缩采用 Codex 风格的版本化 replacement：采样前估算完整物化请求，达到 90% 自动阈值
时 replace transcript，再把当前 working context 注入新窗口一次。provider 报告 token 达到阈值时，
下一次采样前执行同样 replacement；压缩不得丢失 tool call/output 配对或当前用户任务。

每个指令层分别计算内容 hash；基础、模式角色、Skill、可见工具组说明、Workspace、wire 工具前缀、provider、
model 或 compaction 变化都给出精确 `PromptPrefixChangedReason` 并提升 generation。工具与
缓存的关系只由 `ToolPlan::wire_fingerprint` 表达：它是实际发送的完整 `ToolSpec` 列表 canonical
哈希。工具按模型可见名称排序，JSON Schema 递归使用确定性字段顺序；registry revision、group
identity、注册顺序和 executor generation 不参与 wire。plan 只在单次 model step/retry 内冻结，
不能为了复用缓存跨 Thread、worktree 或 agent 工具集共享。

DeepSeek 使用隐式共同前缀，不发送 `prompt_cache_key`、breakpoint 或 OpenAI options。工具层不
生成、轮换或参与 provider cache key；宿主显式提供的 session-stable key 仍可由支持的模型调用
透传，但不得由 tool revision 或 wire fingerprint 派生。cache key 只是路由提示，不能代替请求
前缀相等。

provider usage 必须分别报告缓存读取和缓存写入。OpenAI GPT-5.6 及以后模型的
`cache_write_tokens` 按当次价格快照计费；目录未给出显式写入价时，只有有效策略为
`openAiPromptCacheKey` 且模型声明写入 token 能力，才按普通输入价的 `1.25 ×` 冻结写入价，
不得仅凭模型名推断。旧模型或未声明写入能力的 provider 不得制造写入 token。DeepSeek
继续按命中/未命中输入分类计费。

缓存诊断只记录 generation、固定前缀/wire 工具前缀/working context 的 hash、ToolPlan wire
fingerprint、
token 数和变化原因；不得记录 prompt、工具参数或结果、header、凭据和配置正文。

提示词诊断记录完整 tool schema 的估算 token、Programmatic program 数与嵌套调用数。Responses
transport 记录 continuation attempted/used/invalid、
full replay retry 和 HTTP fallback 的稳定原因；compaction 记录替换前后估算 token。这些计数附着于
对应 inference 或 Turn，不能以无法关联的独立日志代替，也不得记录 program 正文。

## 7.11 Web 搜索 Provider 边界

Web 搜索同时维护 OpenAI 与 DeepSeek 两份独立 resolution，再按当前 route 仲裁。OpenAI 路径保留
standalone `/alpha/search` 与 Responses hosted search；DeepSeek 原生搜索只允许当前 route 自身满足：
endpoint 有凭据、模型使用 Responses transport、模型声明 `capabilities.web_search`，且 provider
服务能力声明 `HostedWebSearchDialect::DeepSeekResponses`。DeepSeek 不跨 provider 借用；不满足或
配置关闭时才允许现有 OpenAI resolution 成为回退。

Responses 原生搜索统一通过携带 hosted dialect 的 `ToolSpec::WebSearch` 表达。OpenAI dialect 可发送
external/indexed access、context size、允许域名与近似位置；DeepSeek dialect 严格只序列化
`{"type":"web_search"}`，不得伪装支持官方未承诺的过滤、位置、上下文或 cached/indexed 语义。
tool choice 保持 `auto`。DeepSeek hosted search 是 additive 工具，必须与普通函数、MCP、LSP、文件和
命令工具共存；旧 OpenAI hosted-only 路径仍可按其约束进入 exclusive 模式。

Provider 服务能力同时包含 `hosted_responses` 与 `hosted_dialect`。内置 DeepSeek preset 使用
`DeepSeekResponses`，OpenAI preset 与旧显式配置默认使用 `OpenAiResponses`。preset 实例覆盖
非 canonical `base_url` 时不得继承 hosted search 或其他 Responses hosted 能力；显式 capability
仍可由用户重新声明。Provider catalog schema 9 暴露 dialect，产品层不得从 provider id 或 URL 猜测。

DeepSeek `/responses` 返回的 `web_search_call` 与 OpenAI Responses 共用 canonical SSE decoder、
timeline 和历史回放：`searching` / `completed` 生命周期、search/open/find action 都投影为统一事件；
完整 native item（包括未知字段和 opaque results）作为 Responses context 持久化，并在下一轮按原始
JSON 顺序回放。provider adapter 不自行注入未进入本轮冻结 `ToolPlan` 的 hosted tool。
