# 07 - 模型层

## 7.1 职责

`pl-model` 是 LLM provider 与模型协议适配层，负责把核心层的统一请求转为具体 provider 的 API 请求，并把流式结果转换为 `AgentEvent` 和 `CompletionResponse`。

`pl-model` 不维护会话，不解析 CLI，也不决定编译阶段。
`pl-model` 可以消费已经解析好的自定义模型列表，但不读取配置文件。

内部按三层组织：

- `provider`：一等供应商运行时，当前只包含 OpenAI、DeepSeek 和 Zhipu。
- `protocol`：API 协议编解码，当前实现 OpenAI Responses / Chat Completions，Anthropic 仅保留占位。
- `stream`：provider 无关的流式事件聚合、工具调用合并、plan 提取和 timeline 投影。

## 7.2 依赖

```text
pl-protocol
    ↑
pl-model
    ↑
pl-core
```

`pl-model` 的公共协议边界只依赖 `pl-protocol`，使用其中的：

- `Message`
- `AgentEventSender`
- `PureError`
- `Result`

provider 适配实现可以依赖 `async-openai`、`reqwest` 和 `serde`。这些依赖只用于 `pl-model` 内部 transport、typed protocol request 和 typed stream event 解析，不向 `pl-core` 暴露。

## 7.3 Provider 抽象

`ModelProvider` 封装 provider 特定逻辑：

- `info()`
- `capabilities()`
- `stream_complete(...)`
- `auth_token()`
- `model_info(...)`
- `list_models()`
- `effective_model_capabilities(...)`
- `default_model()`

异步 trait 方法使用原生 RPITIT，并显式声明 `Send` bound。

`SharedModelProvider` 是 `Arc<ProviderRuntime>`。`ProviderRuntime` 是穷尽枚举分发，当前变体为 OpenAI、DeepSeek 和 Zhipu；不使用 `dyn ModelProvider`，也不引入 `async_trait`。

每个 provider 拥有自己的 profile：默认 base URL、默认模型、模型目录、tool wire policy、reasoning/thinking policy 和协议 endpoint policy。模型目录只包含该 provider 的 bundled/configured 模型，不从全局模型列表兜底生成 provider 列表。

Zhipu Coding Plan 是 Zhipu profile 的配置模板，默认使用 `https://open.bigmodel.cn/api/coding/paas/v4`，模型列表与现有 Zhipu 模板完全一致；它不新增 `ProviderRuntime` 变体，也不改变 `ProviderKind::Zhipu` 的协议边界。

## 7.4 Protocol API

`pl-model` 当前支持：

- Responses API
- Chat Completions API

不同 protocol API 的差异保持在 `pl-model` 内部，核心层只看到 `CompletionRequest`、`CompletionResponse` 和事件流。

OpenAI、DeepSeek 和 Zhipu 都复用 `protocol::openai`。OpenAI 默认使用 Responses endpoint；DeepSeek 和 Zhipu 使用 Chat Completions endpoint。Zhipu Coding Plan 作为 Zhipu 模板同样使用 Chat Completions endpoint。OpenAI Responses 的 `reasoning.summary` 按 Codex wire 语义发送：`Auto` 和兼容层的 `Enabled` 都发送 `auto`，`Disabled` 不发送 summary 字段。DeepSeek/Zhipu 的 `reasoning_effort`、`thinking`、`clear_thinking`、`reasoning_content` 等私有扩展由 provider profile 通过强类型 options 注入 OpenAI protocol，不作为独立 wire variant 存在。

provider transport 层把第三方 API 错误统一转换为 `PureError` 时必须先脱敏。错误文本中不得包含 bearer token、API key 或形如 `sk-...` 的密钥片段；鉴权失败、配额不足、模型不存在等服务端错误可以保留 status、错误类型、code 和可读原因，但密钥值必须替换为稳定占位。

提示词分层由 `pl-core` 决定，`pl-model` 只消费已经组装好的 `CompletionRequest`。`CompletionRequest.instructions` 表示 base/system 层，并在 Responses 和 Chat Completions 请求中作为最前面的 system 内容发送。`messages` 可以包含核心层临时插入的 system/user 前置消息；`pl-model` 不区分它们是否来自 developer 或 user context，也不把任何提示词写回会话。

请求体不再由散落的 `serde_json::json!` 直接拼接，而是先转换为 `pl-model` 内部强类型 request，再由 serde 序列化。动态 JSON 只允许出现在 JSON Schema、工具参数、provider 返回的任意 JSON 参数和协议扩展 escape hatch。

`CompletionRequest.stream` 不改变 `stream_complete` 的 wire 行为；`stream_complete` 始终发起流式请求。该字段只保留为统一请求类型的一部分。

## 7.5 自定义模型

`pl-core` 从 `~/.pure/config.toml` 读取完整模型配置后，将 provider 配置和模型列表传给 `pl-model`。

配置模型会覆盖或补充 bundled model；`used_fallback` 仍是运行时状态，不从 TOML 读取。

模型信息中的 `base_instructions` 是模型级基础提示词来源，进入 `pl-core` 的 instruction assembler；配置中的 `[instructions].base_override` 可以完整替换它。模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、摘要 prompt、历史替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。
