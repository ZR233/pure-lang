# 07 - 模型层

## 7.1 职责

`pl-model` 是 LLM provider 层，负责把核心层的统一请求转为具体 provider 的 API 请求，并把流式结果转换为 `AgentEvent` 和 `CompletionResponse`。

`pl-model` 不维护会话，不解析 CLI，也不决定编译阶段。

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
