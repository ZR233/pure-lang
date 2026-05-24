# 08 - 流式事件

## 8.1 统一事件层

`AgentEvent` 定义在 `pl-protocol`，是系统统一输出通道。

当前事件来源：

- `pl-core`：`TurnStarted`、`Done`、核心错误事件。
- `pl-model`：文本增量、思考增量、工具调用增量。

当前事件消费者：

- `pure-studio`
- 测试
- 后续日志、审计和 UI 层

## 8.2 数据流

```text
pl-model provider
  → AgentEventSender
  → pl-core 汇总 TurnResult
  → pure-studio 实时渲染
```

`pure-studio` 必须订阅同一 `AgentEventReceiver`，并把事件转换为 Slint UI 状态：

- `TextDelta` 追加到当前 assistant 消息。
- `ThinkingDelta` 追加到当前思考区域。
- `ToolCallDelta` / `ToolCallComplete` 展示工具调用状态。
- `ToolApprovalRequested` / `ToolApprovalGranted` / `ToolApprovalDenied` 展示并记录审批流程。
- `Error` 渲染为可恢复或致命错误提示。

## 8.3 背压与容量

事件通道使用 `tokio::sync::broadcast`。默认容量由调用方创建，目前建议为 `256`。

未来如果需要多消费者，应保持同一事件源，避免为 GUI、日志和测试分别设计不同输出通道。

## 8.4 事件边界

事件类型属于协议层，不应包含 provider 私有结构，也不应绑定具体前端。工具审批事件只承载通用工具名、参数和审批结果，不包含 Slint 或桌面端私有状态。
