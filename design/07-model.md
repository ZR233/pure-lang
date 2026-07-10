# 07 - 模型层

## 7.1 职责

`pl-model` 是 LLM provider 与模型协议适配层，负责把核心层的统一请求转为具体 provider 的 API 请求，并把流式结果转换为 provider 无关的 `CompletionStreamEvent`、`pl-trace::AgentEvent` 和 `CompletionResponse`。

`pl-model` 不维护会话，不解析 CLI，也不决定编译阶段。
`pl-model` 可以消费已经解析好的自定义模型列表，但不读取配置文件。

内部按三层组织：

- `provider`：一等供应商运行时，当前只包含 OpenAI、DeepSeek 和 Zhipu。
- `protocol`：API 协议编解码，当前实现 OpenAI Responses / Chat Completions，Anthropic 仅保留占位。
- `stream`：provider 无关的 public canonical stream event、工具调用合并、plan 提取和 timeline 投影。

`protocol` 只负责把 provider 私有 SSE chunk 映射为 `stream` 的 canonical event。OpenAI Responses、OpenAI Chat、DeepSeek 和 Zhipu/GLM 兼容接口在进入 accumulator 前必须统一为 response started/id、文本、思考、reasoning summary、工具参数、工具 ready/done、usage 和完成/失败事件。核心层、Studio timeline 和外部集成方不解析 provider 原始 JSON。

`CompletionEventStream` 是 `pl-model` 的公开流式边界，元素类型为 `Result<CompletionStreamEvent>`。`ModelProvider::stream_events` 返回该流，供 `mai-team` 等调用方原生消费 provider 无关事件；`stream_complete` 保留为兼容 API，但必须通过同一条 public stream API 累计出 `CompletionResponse`，避免在核心层或外部仓库复刻 provider adapter。

`stream` 层负责稳定工具调用 identity。OpenAI Responses 可能先发送只有 provider `item_id` 的 `output_item.added`，后续 delta 或 done 才补 `call_id`；Chat Completions 也可能只依赖 chunk index 作为 `stream_id`。同一个工具调用一旦通过 `stream_id`、`item_id` 或 `call_id` 中任一非空身份进入 accumulator，后续 late metadata 必须合并到同一个 open tool，不得因为 `call_id` 后到而拆成第二个 tool call 或第二个 trace part。trace 的 tool part id 以最早稳定的 provider item/runtime tool id 为锚，`call_id` 只作为 metadata 写入 tool snapshot，用于协议回放和 provider tool result 匹配。

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

provider 适配实现可以依赖 `async-openai`、`reqwest` 和 `serde`。这些依赖只用于 `pl-model` 内部 transport、typed protocol request 和 typed stream event 解析，不向 `pl-core` 暴露。

## 7.3 模型能力

`ModelInfo` 的能力声明使用结构化能力矩阵，不再使用 bitflag 或旧的 `input_modalities` 列表作为主协议。配置和运行时只接受新的 `capabilities` 对象：

- 基础能力：`streaming`、`temperature`、`reasoning`、`web_search`。
- 输入/输出模态：`input`、`output`，取值为 `text`、`image`、`audio`、`video`、`pdf`。
- 工具能力：`function_calling`、`parallel_tool_calls`、`custom_tools`、`freeform_tools`。
- 推理交错字段：`interleaved.field`，当前支持 `reasoning`、`reasoning_content`、`reasoning_details`。

`pl-core` 只读取这些 provider 无关能力来做本地校验和 UI 展示：图片输入必须要求模型声明 `input = ["image"]`，工具调用必须匹配工具能力，推理请求必须匹配 `reasoning = true`。provider 私有差异不扩散到 `pl-core`。

模型级 provider override 使用 `ModelRequestProfile` 表达，包括 `api_model`、`headers`、`body`、`options`、`max_tokens_field` 和 `responses_max_tokens_field`。`body` 作为 base body 注入请求体（如 DeepSeek 固定的 `thinking.type = enabled`）；其余可变字段（如 effort 透传的 `reasoning_effort`、GLM `thinking.clear_thinking`）由 `ModelInfo.parameters` 声明驱动（见 7.8）。这些字段只由 `pl-model` 的 provider adapter 消费；核心编排层不得读取或拼接这些私有字段。Chat Completions 的最大输出 token 字段默认写入 `max_tokens`；OpenAI-compatible provider 若要求新字段（如 MiMo 的 `max_completion_tokens`）可在模型 profile 中声明。Responses endpoint 默认不发送最大输出 token 字段，以匹配 Codex 常规 Responses 请求；Responses-like 代理若要求限制字段，可在模型 profile 中把 `responses_max_tokens_field` 设置为 `max_output_tokens`、`max_tokens` 或 `max_completion_tokens`。

## 7.4 Provider 抽象

`ModelProvider` 封装 provider 特定逻辑：

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

`SharedModelProvider` 是 `Arc<OpenAiProvider>`。三个供应商（OpenAI、DeepSeek、Zhipu）共享同一个 OpenAI 兼容 transport，差异仅在 endpoint 选择、bundled 模型目录和 `ProviderCapabilities`，由 `ProviderKind` 在构造时一次决定；不使用 `dyn ModelProvider`，也不引入 `async_trait`，不再为每供应商单独定义 struct 或穷尽枚举分发。未来若引入协议真正不同的供应商（如 Anthropic），再新增独立 provider struct 与分发枚举。

每个 provider 拥有自己的 profile：默认 base URL、默认模型、模型目录、tool wire policy 和协议 endpoint policy。reasoning/thinking/effort 的 wire 规则不再由 provider struct 携带，而是由模型 `parameters` 声明驱动（见 7.8），provider 层与协议层都不再包含任何 reasoning policy 硬编码。模型目录只包含该 provider 的 bundled/configured 模型，不从全局模型列表兜底生成 provider 列表。

OpenAI-compatible Chat 供应商使用通用 `ProviderKind::OpenAiCompatibleChat` 表达，不为 MiMo 等兼容供应商新增 runtime struct。它复用 OpenAI transport、Chat Completions endpoint、配置模型目录和通用 parameter/profile wire；具体 base URL、默认模型、headers、tool wire policy 与模型能力由 `ProviderInfo` 和 `ModelInfo` 提供。

Zhipu Coding Plan 是 Zhipu profile 的配置模板，默认使用 `https://open.bigmodel.cn/api/coding/paas/v4`，模型列表与现有 Zhipu 模板完全一致；它不新增 `ProviderRuntime` 变体，也不改变 `ProviderKind::Zhipu` 的协议边界。

## 7.5 Protocol API

`pl-model` 当前支持：

- Responses API
- Chat Completions API

不同 protocol API 的差异保持在 `pl-model` 内部，核心层只看到 `CompletionRequest`、`CompletionResponse` 和 provider 无关的 timeline 事件流。

OpenAI、DeepSeek 和 Zhipu 都复用 `protocol::openai`。OpenAI 默认使用 Responses endpoint；DeepSeek 和 Zhipu 使用 Chat Completions endpoint。Zhipu Coding Plan 作为 Zhipu 模板同样使用 Chat Completions endpoint。

effort 等可调参数的 wire 写入由通用透传机制驱动，协议层不再为每供应商硬编码 reasoning/thinking 映射。`build_request` 接收当前 `ModelInfo`，先序列化强类型核心字段（model、messages、stream、tools 等）为 JSON 对象，再依次注入：base body（`ModelRequestProfile.body`，如 DeepSeek 固定的 `thinking.type = enabled`），以及 parameter wire（用户选中的候选值按模型 `parameters` 声明写入或移除字段，见 7.8）。覆盖优先级为 parameter wire > base body > 协议默认字段。

OpenAI Responses 的 `reasoning.summary` 仍按 Codex wire 语义发送（`Auto` 和兼容层的 `Enabled` 都发送 `auto`，`Disabled` 不发送 summary 字段），由 `ReasoningConfig.summary` 独立驱动，不进入 parameter wire。模型返回的 `reasoning_content` 进入 canonical reasoning event；历史回放时仍通过 assistant message 的 `reasoning_content` 字段写回 Chat Completions。

OpenAI Responses continuation 字段由 `CompletionRequest` 承载：`store`、`previous_response_id`、`prompt_cache_key`。这些字段只序列化到 Responses 请求体；Chat Completions 请求体不得发送这些字段。

`CompletionRequest.messages` 中的 `MessageRole::System` 表示本轮临时前置指令或开发者上下文。Responses endpoint 序列化为 input message role `developer`，避免发送不被部分 Responses 兼容服务接受的 `system` role；Chat Completions 仍序列化为 `system` role。

provider transport 层把第三方 API 错误统一转换为 `PureError` 时必须先脱敏。错误文本中不得包含 bearer token、API key 或形如 `sk-...` 的密钥片段；鉴权失败、配额不足、模型不存在等服务端错误可以保留 status、错误类型、code 和可读原因，但密钥值必须替换为稳定占位。

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

`pl-core` 从 `~/.pure/config.toml` 读取完整模型配置后，将 provider 配置和模型列表传给 `pl-model`。

配置模型会覆盖或补充 bundled model；`used_fallback` 仍是运行时状态，不从 TOML 读取。旧配置里的 `capabilities = [...]` 和 `input_modalities = [...]` 不再兼容；读取失败时要求用户按新的能力矩阵重写配置或让 Studio 重新生成配置。

模型信息中的 `base_instructions` 是模型级基础提示词来源，进入 `pl-core` 的 instruction assembler；配置中的 `[instructions].base_override` 可以完整替换它。模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、摘要 prompt、历史替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。

## 7.8 模型可调参数

effort（推理强度）不再是固定的全局枚举，而是「模型声明的可调参数」。该机制是通用的——effort 是首个应用，类型设计可容纳未来 thinking、verbosity 等参数。各供应商自由定义候选值域，并由模型自身声明选中值如何写入 API 请求体，协议层据此通用透传，不包含任何供应商特定代码。

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
| OpenAI | `medium` / `low` / `high` / `xhigh` | `reasoning.effort` = 值 | — |
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
