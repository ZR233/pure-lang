# 07 - 模型层

## 7.1 职责

`pl-model` 是 LLM provider 与模型协议适配层，负责把核心层的统一请求转为具体 provider 的 API 请求，并把流式结果转换为 provider 无关的 `CompletionStreamEvent`、`pl-trace::AgentEvent` 和 `CompletionResponse`。

`pl-model` 不维护产品 agent/session 历史，不解析 CLI，也不决定产品阶段。它只维护
`ModelTransportSession`：与一个 `AgentSession` 同生命周期的物理连接、脱敏 fingerprint 和
Responses continuation 状态。
`pl-model` 可以消费已经解析好的自定义模型列表，但不读取配置文件。

内部按三层组织：

- `provider`：按 wire protocol 执行的通用 provider runtime，不按厂商枚举分发。
- `protocol`：API 协议编解码，当前实现 OpenAI Responses / Chat Completions，Anthropic 仅保留占位。
- `stream`：provider 无关的 public canonical stream event、工具调用合并、plan 提取和 timeline 投影。

`protocol` 只负责把 provider 私有流事件映射为 `stream` 的 canonical event。OpenAI Responses WebSocket、Responses HTTP/SSE、OpenAI Chat、DeepSeek 和 Zhipu/GLM 兼容接口在进入 accumulator 前必须统一为 response started/id、文本、思考、reasoning summary、工具参数、工具 ready/done、usage 和完成/失败事件。核心层、Studio timeline 和外部集成方不解析 provider 原始 JSON。

`CompletionEventStream` 是 `pl-model` 的公开流式边界，元素类型为 `Result<CompletionStreamEvent>`。`ModelProvider::stream_events` 返回该流，供 `mai-team` 等调用方原生消费 provider 无关事件；`stream_complete` 保留为兼容 API，但必须通过同一条 public stream API 累计出 `CompletionResponse`，避免在核心层或外部仓库复刻 provider adapter。

`stream` 层负责稳定工具调用 identity。OpenAI Responses 的非流式 item、SSE added/done 和 delta
在协议边界统一规范化身份：`item_id = id ?? call_id`，`call_id = call_id ?? item_id`。兼容
provider 只返回一个非空身份时，该身份同时成为 canonical item id 与 Responses call id；两者
都缺失时仍是协议错误。Responses 可能先发送只有 provider `item_id` 的
`output_item.added`，后续 delta 或 done 才补独立 `call_id`；Chat Completions 也可能只依赖
chunk index 作为 `stream_id`。同一个工具调用一旦通过 `stream_id`、`item_id` 或 `call_id`
中任一非空身份进入 accumulator，后续 late metadata 必须升级原 accumulator 的 fallback
`call_id` 并合并到同一个 open tool，不得拆成第二个 tool call 或第二个 trace part。trace 的
tool part id 以最早稳定的 provider item/runtime tool id 为锚，`call_id` 只作为 metadata 写入
tool snapshot，用于协议回放和 provider tool result 匹配。

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
- `pl_trace::AgentEventSender`

provider 适配实现可以依赖 `async-openai`、`reqwest`、`tokio-tungstenite` 和 `serde`。这些依赖只用于 `pl-model` 内部 transport、typed protocol request 和 typed stream event 解析，不向 `pl-core` 暴露。

## 7.3 模型能力

`ModelInfo` 的能力声明使用结构化能力矩阵，不再使用 bitflag 或旧的 `input_modalities` 列表作为主协议。配置和运行时只接受新的 `capabilities` 对象：

- 基础能力：`streaming`、`temperature`、`reasoning`、`web_search`。
- 输入/输出模态：`input`、`output`，取值为 `text`、`image`、`audio`、`video`、`pdf`。
- 工具能力：`function_calling`、`parallel_tool_calls`、`custom_tools`、`freeform_tools`。
- 推理交错字段：`interleaved.field`，当前支持 `reasoning`、`reasoning_content`、`reasoning_details`。

`pl-core` 只读取这些 provider 无关能力来做本地校验和 UI 展示：图片输入必须要求模型声明 `input = ["image"]`，工具调用必须匹配工具能力，推理请求必须匹配 `reasoning = true`。provider 私有差异不扩散到 `pl-core`。

提示词缓存不是由模型 slug 隐式推断的基础能力。`ProviderServiceCapabilities.prompt_cache`
声明 endpoint 的缓存 dialect，模型目录声明该模型是否报告缓存写入 token；核心层把 provider
声明、wire protocol 与模型声明合成为穷尽的 `EffectivePromptCachePolicy`。未声明能力的自定义
Responses/Chat endpoint 默认不发送任何缓存专属字段。

模型级 provider override 使用 `ModelRequestProfile` 表达，包括 `api_model`、`headers`、`body`、`options`、`max_tokens_field` 和 `responses_max_tokens_field`。`body` 作为 base body 注入请求体（如 DeepSeek 固定的 `thinking.type = enabled`）；其余可变字段（如 effort 透传的 `reasoning_effort`、GLM `thinking.clear_thinking`）由 `ModelInfo.parameters` 声明驱动（见 7.8）。这些字段只由 `pl-model` 的 provider adapter 消费；核心编排层不得读取或拼接这些私有字段。Chat Completions 的最大输出 token 字段默认写入 `max_tokens`；OpenAI-compatible provider 若要求新字段（如 MiMo 的 `max_completion_tokens`）可在模型 profile 中声明。Responses endpoint 默认不发送最大输出 token 字段，以匹配 Codex 常规 Responses 请求；Responses-like 代理若要求限制字段，可在模型 profile 中把 `responses_max_tokens_field` 设置为 `max_output_tokens`、`max_tokens` 或 `max_completion_tokens`。

## 7.4 Provider 抽象

`ModelProvider` 封装协议执行逻辑：

- `info()`
- `capabilities()`
- `stream_events(...)`
- `stream_complete(...)`
- `auth_token()`
- `model_info(...)`
- `list_models()`
- `effective_model_capabilities(...)`
- `default_model()`

异步 trait 方法使用原生 RPITIT，并显式声明 `Send` bound。

`SharedModelProvider` 是 `Arc<OpenAiProvider>`。这里的 `OpenAiProvider` 是 Responses / Chat
Completions 协议执行器，不代表供应商身份。`ProviderInfo.protocol` 选择 wire API，
`connection_mode` 选择 WS/HTTP；endpoint、模型目录与 wire policy 来自解析后的 provider 实例。
运行路径不匹配 OpenAI、DeepSeek、Zhipu、MiMo 等 ID，也不为这些厂商建立穷尽枚举分发。
未来若引入协议真正不同的供应商（如 Anthropic），才新增独立 typed adapter。

每个 provider 实例保存 endpoint override、凭证、headers、tool wire policy、catalog binding 和
连接方式；具体模型由角色 route 选择，不保存第二份 provider default model。reasoning/thinking/effort
的 wire 规则由模型 `parameters` 声明驱动（见 7.8）。模型目录通过
`ProviderConfig::effective_models()` 解析，不从全局列表兜底。

OpenAI-compatible Chat 供应商使用 `ProviderWireProtocol::ChatCompletions +
ProviderConnectionMode::Http` 表达，不为 MiMo 等兼容供应商新增 runtime struct。Responses-compatible
自定义供应商使用 `Responses + Http`（默认）或显式 `Responses + WebSocket`。具体 base URL、headers、
tool wire policy 与模型能力由 `ProviderInfo` 和 `ModelInfo` 提供。

Zhipu Coding Plan 是 catalog preset，默认使用 `https://open.bigmodel.cn/api/coding/paas/v4`，
并引用 Zhipu 模型目录；它不新增 runtime 变体。MiMo API 与 Token Plan 同样是两个 preset，
共同引用一个 `mimo` catalog。

## 7.5 Protocol API

`pl-model` 当前支持：

- Responses API
- Chat Completions API

不同 protocol API 的差异保持在 `pl-model` 内部，核心层只看到 `CompletionRequest`、`CompletionResponse` 和 provider 无关的 timeline 事件流。

所有当前 preset 都复用 `protocol::openai` 的两种 wire API。协议与连接模式是正交维度：
Responses 支持 `web_socket | http`，Chat Completions 只支持 `http`。内置 OpenAI preset 使用
Responses，模式顺序固定 WS、HTTP，默认 WS；选择 HTTP 时仍调用 `/responses` 并消费 SSE，
绝不切换到 Chat Completions。MiMo、DeepSeek、Zhipu 使用 Chat Completions HTTP。

连接模式由 `ProviderConnectionMode` 表达并持久化在每个 provider 实例上，不由厂商身份隐式
推断。preset 在 catalog 中同时声明协议、允许模式顺序和默认模式，Web 与 Flutter 只渲染
catalog 返回的选项。相同 preset 可以创建多个实例，每个实例独立保存 endpoint、凭证、
连接模式、附加模型和路由；唯一性只约束 `ProviderId`。

Responses WebSocket 使用 `/responses` 握手和 `response.create` 帧，并强制发送 `store: false`；continuation 只依赖当前物理连接，不能把响应持久化到供应商侧。物理连接属于 `AgentSession` 的运行期 transport session：同一会话跨 turn 复用，不同会话绝不共享，持久化恢复后重新建立。HTTP 模式和连接重建都不能偷偷改变用户选择；握手或流错误按所选模式直接报告。`previous_response_id` continuation 只在 WebSocket 模式启用，因为该状态与物理连接绑定；HTTP/SSE 始终发送完整 canonical history，不依赖连接级 continuation。

WebSocket 建连通过系统 DNS 解析全部目标地址，并以 250ms 间隔交错竞争 IPv4/IPv6；首个成功的 TCP 连接继续使用原始域名完成 SNI、证书校验和 WebSocket 握手。单次完整握手保持 15 秒上限，超时保留 transient 分类并进入同一 WS 模式的既有重试。耗尽重试后的错误给出可操作诊断：检查 WebSocket 网络可达性，或在 Studio Provider 设置中显式切换为 HTTP；运行时不得自动回退连接模式。

effort 等可调参数的 wire 写入由通用透传机制驱动，协议层不再为每供应商硬编码 reasoning/thinking 映射。`build_request` 接收当前 `ModelInfo`，先序列化强类型核心字段（model、messages、stream、tools 等）为 JSON 对象，再依次注入：base body（`ModelRequestProfile.body`，如 DeepSeek 固定的 `thinking.type = enabled`），以及 parameter wire（用户选中的候选值按模型 `parameters` 声明写入或移除字段，见 7.8）。覆盖优先级为 parameter wire > base body > 协议默认字段。

OpenAI Responses 的 `reasoning.summary` 仍按 Codex wire 语义发送（`Auto` 和兼容层的 `Enabled` 都发送 `auto`，`Disabled` 不发送 summary 字段），由 `ReasoningConfig.summary` 独立驱动，不进入 parameter wire。模型返回的 `reasoning_content` 进入 canonical reasoning event；历史回放时仍通过 assistant message 的 `reasoning_content` 字段写回 Chat Completions。

核心层提交的 `CompletionRequest` 始终带完整 canonical input，且不计算 continuation。
所有核心 agent、权限审查和本地压缩请求显式使用 `store: false`，避免把会话持久化到供应商侧；
Chat Completions wire 忽略该 Responses 专属字段。低层 `pl-model` API 仍可表达显式 store 策略，
但产品 runtime 不能依赖供应商存储来维持历史。
`ModelTransportSession` 在相同连接和 fingerprint 下由上次完整请求前缀计算增量，在 transport
内部克隆请求并只对 Responses WebSocket 帧设置 `previous_response_id`。断线、取消、未完整消费、配置变化或无效 continuation
都会关闭旧连接；最多一次在新 WS 上用完整历史重试。Responses HTTP/SSE 和 Chat Completions
始终发送完整历史。

`CompletionRequest.messages` 中的 `MessageRole::System` 表示本轮临时前置指令或开发者上下文。Responses endpoint 序列化为 input message role `developer`，避免发送不被部分 Responses 兼容服务接受的 `system` role；Chat Completions 仍序列化为 `system` role。

provider transport 层把第三方 API 错误统一转换为 `PureError` 时必须先脱敏。错误文本中不得包含 bearer token、API key 或形如 `sk-...` 的密钥片段；鉴权失败、配额不足、模型不存在等服务端错误可以保留 status、错误类型、code 和可读原因，但密钥值必须替换为稳定占位。
Responses HTTP/SSE 与 Chat Completions HTTP 必须和 WebSocket 一样，用 Serde typed error DTO 保留结构化 provider code、HTTP status、message 与可选 retry hint；进入控制流后不得把 DTO 降级成待解析字符串。408/409/425/429、5xx 以及 `server_is_overloaded` 等容量错误只允许在尚未开始消费流式输出时有限重放完整请求；一旦 HTTP 流已经建立，transport 不得因为后续流错误自动重放并制造重复输出。

提示词分层由 `pl-core` 决定，`pl-model` 只消费已经组装好的 `CompletionRequest`。`CompletionRequest.instructions` 表示 base/system 层，并在 Responses 和 Chat Completions 请求中作为最前面的 system 内容发送。`messages` 可以包含核心层临时插入的 system/user 前置消息；`pl-model` 不区分它们是否来自 developer 或 user context，也不把任何提示词写回会话。

请求体不再由散落的 `serde_json::json!` 直接拼接，而是先转换为 `pl-model` 内部强类型 request，再由 serde 序列化。动态 JSON 只允许出现在 JSON Schema、工具参数、provider 返回的任意 JSON 参数和协议扩展 escape hatch。

`CompletionRequest.stream` 不改变 `stream_complete` 的 wire 行为；`stream_complete` 始终发起流式请求。该字段只保留为统一请求类型的一部分。

## 7.6 多模态消息

`MessageContent` 支持一等 multipart 内容。文本使用 `ContentPart::Text`，图片使用 `ContentPart::Image`。图片 source 分为：

- `Attachment { attachment_id }`：Studio 或核心持久化消息中的稳定引用。
- `InlineBase64 { data }`：模型请求前由 `pl-core` materialize 后传给 `pl-model` 的临时内容。

`pl-model` 不读取 Studio 存储，也不解析附件路径。进入 provider adapter 前，`CompletionRequest` 中的图片必须已经 materialize 为 `InlineBase64`；如果 adapter 收到未 materialize 的附件引用，应返回本地协议错误。

OpenAI Responses 使用 `input_text` 与 `input_image` data URL；OpenAI Chat、DeepSeek、Zhipu/GLM 使用 content array 的 `text` 与 `image_url`。DeepSeek 或未声明视觉输入的模型在本地拒绝图片请求。

## 7.7 自定义模型

产品宿主使用 serde 读取自己的完整配置，调用 `pl-core::AgentModelConfig::validate/resolve` 后，
把 `ResolvedModelRoute` 中的 provider 信息和 effective model 传给 `pl-model`。`pl-core` 与
`pl-model` 都不读取 `~/.pure/config.toml`。

Bundled catalog 只读，配置只能通过 `additional_models` 追加不冲突 slug；完全自定义 provider
使用 `Explicit { models }`。`used_fallback` 仍是运行时状态，不从配置读取。

模型信息中的 `base_instructions` 是模型级基础提示词来源，进入 `pl-core` 的 instruction assembler；配置中的 `[instructions].base_override` 可以完整替换它。模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、历史保留、原子替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。

`CompletionRequest.input` 使用 provider 无关的有序 `ModelContextItem`，包括普通 `Message` 和专用 `Compaction { encryptedContent }`；`.messages(...)` 只是不含 checkpoint 的便捷构造器。Responses request 可以把 compaction item 映射为原生输入，Chat Completions 必须明确拒绝。`ModelProvider::compact_context` 接收模型、instructions、有序上下文、工具、parallel tool calls、reasoning 和 prompt cache key，并返回经过 provider 解析的上下文项与可选 usage。远程协议能力由 `ProviderWireProtocol::Responses` 决定，不依赖 preset ID；远程 compaction 固定走独立 HTTP 请求。

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

四个供应商的 effort 声明形态：

| 供应商 | candidates | wire.set（选中值 → 字段） | wire.remove |
| --- | --- | --- | --- |
| OpenAI（GPT-5.5 / GPT-5.4 / GPT-5.4-Mini） | `medium` / `low` / `high` / `xhigh` | `reasoning.effort` = 值 | — |
| OpenAI（GPT-5.6 Sol） | `low` / `medium` / `high` / `xhigh` / `max` | `reasoning.effort` = 值 | — |
| OpenAI（GPT-5.6 Terra / Luna） | `medium` / `low` / `high` / `xhigh` / `max` | `reasoning.effort` = 值 | — |
| DeepSeek | `high` / `max` | `reasoning_effort` = 值（`thinking.type = enabled` 作为 base body） | — |
| Zhipu 普通 | `enabled` / `none` | `thinking.type` = 值 | — |
| GLM-5.2 | `high` / `max` / `none` | `high`/`max`：`reasoning_effort` + `thinking.type = enabled` + `thinking.clear_thinking = false`；`none`：`thinking.type = disabled` | `none` 移除 `reasoning_effort` |

GLM-5.2 的「一个选择联动多个字段」和「none 时移除字段」由 wire 的多条 `set` 与 `remove` 完整表达，无需协议层特判。

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

`pl-model` 内置四个预设：`openai_family`、`deepseek_family`、`zhipu_text_family`、`zhipu_vision_family`。原 `openai_capabilities` / `deepseek_capabilities` / `zhipu_capabilities` 三个能力构造函数的能力矩阵直接编入对应 family，消除重复。`zhipu_text_family` 与 `zhipu_vision_family` 的差异仅在 capabilities 的输入模态（是否含 `image`）和 effort 候选值域。

## 7.10 Prompt 缓存

核心层按 prompt generation 组装请求，唯一顺序是：模型基础指令、平台与全局配置、模式与
角色、Skill、Workspace/项目文档、durable model history。同一 generation 内，model、
instructions、tools、tool choice、reasoning、输出 schema 和 service tier 不得变化；历史只追加
assistant、tool、user 与内部 contextPatch。模型相关运行状态变化在采样前渲染为最小
contextPatch，先持久化再发送，不能作为下一轮消失的临时尾部。

每个指令层分别计算内容 hash；基础、模式角色、Skill、Workspace、工具 schema、provider、
model 或 compaction 变化都给出精确 `PromptPrefixChangedReason` 并提升 generation。工具按模型
可见名称排序，JSON Schema 递归使用确定性字段顺序。MCP lease 与工具 schema 在 Turn 内冻结，
不能为了复用缓存跨 Thread、worktree、Task policy 或权限边界共享。

DeepSeek Chat 使用隐式共同前缀，不发送 `prompt_cache_key`、breakpoint 或 OpenAI options。
内建 OpenAI Responses HTTP/WS 使用 `OpenAiPromptCacheKey` policy：core 用
`ThreadId + prompt scope + generation` 的不可逆 hash 派生稳定 key，同一 generation 复用，
generation 变化立即切换。手工 key override 优先于自动 key。当前实现跟随 Codex，仅发送 key，
不发送显式 breakpoint；自定义兼容 endpoint 只有显式声明能力才可启用。cache key 只是路由
提示，不能代替请求前缀相等。

provider usage 必须分别报告缓存读取和缓存写入。OpenAI GPT-5.6 及以后模型的
`cache_write_tokens` 按当次价格快照计费；目录未给出显式写入价时，只有有效策略为
`openAiPromptCacheKey` 且模型声明写入 token 能力，才按普通输入价的 `1.25 ×` 冻结写入价，
不得仅凭模型名推断。旧模型或未声明写入能力的 provider 不得制造写入 token。DeepSeek
继续按命中/未命中输入分类计费。

缓存诊断只记录 generation、固定前缀/工具/contextPatch 的 hash、token 数和变化原因；不得记录
prompt、工具参数或结果、header、凭据和配置正文。

## 7.11 Web 搜索 Provider 边界

Web 搜索只把 `ProviderTransportSelection::Preset { preset: "openai", .. }` 且 `resolved_bearer_token()` 非空的 provider 实例视为可用 OpenAI 账户。实例 id、显示名或 base URL 可以修改而不改变 preset 身份；普通 custom Responses-compatible provider 即使协议和模型名称相同，也不能获得 OpenAI hosted 或 `/alpha/search` 能力。

Responses 原生搜索通过 `ToolSchema::WebSearch` 表达，并只允许在当前 turn 自身使用上述 OpenAI preset、Responses wire、有效凭据且模型声明 `capabilities.web_search` 时注入。跨 provider 搜索只能走普通函数工具，由该工具使用另一个已解析的 OpenAI provider 调用 `/alpha/search`；不得把 hosted tool 注入非 OpenAI provider 请求。
