# 03 - 编译流程（方案乙）

## 3.1 总览

方案乙将流程明确分成“桥接层 -> 应用层 -> 领域核心 -> 适配器”：

```text
React action
  -> Tauri commands
  -> pl-core application service (StudioRuntime)
  -> interfaces ports
  -> infrastructure adapters (sqlite/config/fs/event/tool)
  -> PureCore turn pipeline
  -> AgentEvent / TraceEvent
  -> Tauri event
  -> reducer action
  -> UI rendering
```

`main.rs` 不承载流程逻辑，只负责注册。

## 3.2 输入与策略

运行输入统一为新 DTO 契约（camelCase wire）。

- `compileMode`：`plan | auto`
- `turnOptions.toolApprovalPolicy`：默认固定 `autoAllow`
- `prompt`、`sessionId`、`workspaceRoot` 等进入 application service

策略约束：

- 方案乙不保留旧命令别名和旧字段兜底
- `ToolApprovalPolicy::AutoAllow` 为默认且主路径
- 手动审批接口保留在系统能力中，但不作为默认流程

## 3.3 核心 turn 编排

`StudioRuntime` 只做 use case 编排：

1. 读取 session/project/config
2. 构造 `TurnRequest` 与 `TurnOptions`
3. 组装 `PureCore`（含工具注册）
4. 执行 `run_turn_with_trace`
5. 事务化批量落库：message + trace + runtime snapshot
6. 输出命令响应 DTO 与 timeline DTO

持久化原则：

- 消息和 trace 采用事务批量写入，避免逐条写放大
- timeline 读取以 `sequence` 为单调游标

## 3.4 事件管线

`drain_events` 使用显式分支处理广播通道状态：

- `Ok(event)`：正常转发
- `Err(Lagged(n))`：记录丢帧指标并继续 drain，不退出
- `Err(Closed)`：结束循环

这保证高频 delta 下 UI 不会因为 lagged 直接断流。

## 3.5 Turn 收尾语义

turn 生命周期持久化语义固定：

- `started`
- `completed`
- `failed`
- `interrupted`

用户停止属于 `interrupted`，不可被延迟完成覆盖。

## 3.6 输出模型

命令输出统一采用新 DTO：

- `bootstrapResponse`
- `projectSelectionResponse`
- `sessionSelectionResponse`
- `runPromptResponse`
- `sessionTimelineResponse`

前端 reducer 只消费 action 输入类型，不再由事件监听器直接拼装复杂 UI 状态。
