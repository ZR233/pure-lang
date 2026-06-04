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

`pl-model` 只依赖 `pl-protocol`，使用其中的：

- `Message`
- `AgentEventSender`
- `PureError`
- `Result`

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

## 7.5 自定义模型

`pl-core` 从 `~/.pure/config.toml` 读取完整模型配置后，将 provider 配置和模型列表传给 `pl-model`。

配置模型会覆盖或补充 bundled model；`used_fallback` 仍是运行时状态，不从 TOML 读取。

模型信息中的 `context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值。上下文压缩的触发判断、摘要 prompt、历史替换和持久化都在 `pl-core` 完成，`pl-model` 不维护压缩状态。
