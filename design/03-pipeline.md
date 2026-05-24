# 03 - 编译流程

## 3.1 总览

当前编译流程是一个由 `pl-core` 编排的单轮 turn。`pure-studio` 只负责 UI 输入输出：

```text
pure-studio UI action
  → pl-core Studio API
  → ConfigStore 读取 ~/.pure/config.toml
  → StudioStore 通过 SeaORM 读取 SQLite 项目和会话消息
  → TurnRequest + TurnOptions
  → CoreSession
  → PureCore
  → AgentEvent stream
  → Slint UI 实时渲染
  → pl-core StudioStore 持久化消息和工具审批
```

## 3.2 输入

`pure-studio` 的 UI 操作进入 `pl-core` 前转换为明确类型，例如 `CompileMode`：

- 默认：`CompileMode::Plan`
- `--plan`：`CompileMode::Plan`
- `--auto`：`CompileMode::Auto`

`Auto` 只影响模型提示词，使输出更偏执行导向；当前版本仍不会执行命令、写文件或调用沙箱。

普通 prompt 默认使用 `ModelRole::Planner`。`RoleConfig` 提供 provider、model 和 effort。

## 3.3 核心 turn

`PureCore::run_turn(...)` 的职责：

- 将用户 prompt 追加到 `CoreSession`。
- 按角色配置选择 provider/model/effort。
- 根据 `CompileMode` 生成系统 instructions。
- 构造 `CompletionRequest`。
- 调用 `pl-model` provider。
- 将 provider 的流式输出作为 `AgentEvent` 推送。
- 将模型结果追加为 assistant 消息。
- 返回 `TurnResult`。

`PureCore::run_turn_with_options(...)` 在上述流程上额外接收 `TurnOptions`：

- `ToolApprovalPolicy::AutoAllow`：工具调用直接执行。
- `ToolApprovalPolicy::Manual`：工具调用先发出审批请求，前端批准后执行。
- `ToolApprovalPolicy::DenyAll`：工具调用一律作为拒绝结果写回会话。

`pure-studio` 首版通过 `pl-core` 使用 `Manual`。

## 3.4 输出

`TurnResult` 包含：

- `content`
- `reasoning_content`
- `model`
- `usage`
- `mode`
- `session_message_count`
- 角色使用的 provider/model/effort 由配置决定。

`pure-studio` 必须消费 `AgentEvent` 实时渲染 `TextDelta`、`ThinkingDelta`、工具调用状态、审批状态和错误。
