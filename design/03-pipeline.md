# 03 - 编译流程

## 3.1 总览

当前编译流程是一个单轮 turn 编排：

```text
purec args
  → TurnRequest
  → CoreSession
  → PureCore
  → CompletionRequest
  → pl-model provider
  → AgentEvent stream
  → TurnResult
```

## 3.2 输入

`purec` 接收自然语言 prompt，并将 CLI 模式转换为 `CompileMode`：

- 默认：`CompileMode::Plan`
- `--plan`：`CompileMode::Plan`
- `--auto`：`CompileMode::Auto`

`Auto` 只影响模型提示词，使输出更偏执行导向；当前版本仍不会执行命令、写文件或调用沙箱。

## 3.3 核心 turn

`PureCore::run_turn(...)` 的职责：

- 将用户 prompt 追加到 `CoreSession`。
- 根据 `CompileMode` 生成系统 instructions。
- 构造 `CompletionRequest`。
- 调用 `pl-model` provider。
- 将 provider 的流式输出作为 `AgentEvent` 推送。
- 将模型结果追加为 assistant 消息。
- 返回 `TurnResult`。

## 3.4 输出

`TurnResult` 包含：

- `content`
- `reasoning_content`
- `model`
- `usage`
- `mode`
- `session_message_count`

`purec` 首版只渲染最终内容；后续可消费 `AgentEvent` 实现实时渲染。
