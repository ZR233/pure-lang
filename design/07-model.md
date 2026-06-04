# 07 - 模型层

## 7.1 职责

`pl-model` 是 LLM provider 层，负责把核心层的统一请求转为具体 provider 的 API 请求，并把流式结果转换为 `AgentEvent` 和 `CompletionResponse`。

`pl-model` 不维护会话，不解析 CLI，也不决定编译阶段。
`pl-model` 可以消费已经解析好的自定义模型列表，但不读取配置文件。

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

provider 适配实现可以依赖 `async-openai`、`reqwest` 和 `serde`。这些依赖只用于 `pl-model` 内部的 OpenAI-compatible transport、typed wire request 和 typed stream event 解析，不向 `pl-core` 暴露。

## 7.3 Provider 抽象

`ModelProvider` 封装 provider 特定逻辑：

- `info()`
- `capabilities()`
- `stream_complete(...)`
- `auth_token()`
- `model_info(...)`
- `default_model()`

异步 trait 方法使用原生 RPITIT，并显式声明 `Send` bound。

## 7.4 Wire API

`pl-model` 当前支持：

- Responses API
- Chat Completions API

不同 wire API 的差异保持在 `pl-model` 内部，核心层只看到 `CompletionRequest`、`CompletionResponse` 和事件流。

OpenAI-compatible provider 使用 `async-openai` 的 client/stream 能力发送请求。请求体不再由散落的 `serde_json::json!` 直接拼接，而是先转换为 `pl-model` 内部强类型 request，再由 serde 序列化。内部 request 类型对齐 OpenAI Responses 和 Chat Completions wire shape，并用本地强类型扩展补齐 custom reasoning effort、`reasoning_content`、`thinking`、custom/freeform tool 和兼容 provider 私有 usage detail。

`CompletionRequest.stream` 不改变 `stream_complete` 的 wire 行为；`stream_complete` 始终发起流式请求。该字段只保留为统一请求类型的一部分。

## 7.5 自定义模型

`pl-core` 从 `~/.pure/config.toml` 读取完整模型配置后，将 provider 配置和模型列表传给 `pl-model`。

配置模型会覆盖或补充 bundled model；`used_fallback` 仍是运行时状态，不从 TOML 读取。

模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、摘要 prompt、历史替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。
