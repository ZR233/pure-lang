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
  → Tauri event
  → React UI 实时渲染
  → pl-core StudioStore 持久化消息和按策略产生的工具审批记录
```

## 3.2 输入

`pure-studio` 的 UI 操作进入 `pl-core` 前转换为明确类型，例如 `CompileMode`：

- 默认：`CompileMode::Plan`
- `--plan`：`CompileMode::Plan`
- `--auto`：`CompileMode::Auto`

`CompileMode` 只影响模型提示词和行为边界，工具可用性由 `PureCore` 已注册的工具集合和 `TurnOptions` 审批策略决定。`Plan` 模式也可以使用工具做探索、读取和验证，但不应执行会修改工作区或外部环境的动作。`Auto` 模式更偏执行导向，可在审批策略允许时使用工具完成更主动的子任务。

普通 prompt 默认使用 `ModelRole::Planner`。`RoleConfig` 提供 provider、model 和 effort。配置缺失某个角色时，运行时按默认模型补齐：按配置 key 顺序取首个 provider，并使用该 provider 的 `default_model`。

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

`pure-studio` 当前通过 `pl-core` 使用 `AutoAllow`，用于先放开已注册工具的执行。`Manual` 审批流程和前端事件仍保留，后续可以切回手动审批或接入更细粒度策略。

`subagent` 工具可接收 `role` 参数，值为 `explorer`、`planner`、`executor` 或 `reviewer`。未传 `role` 时默认使用 `executor`。子代理使用所选角色的 provider/model/effort 创建独立会话，不沿用父会话的 provider。

Plan 和 Auto 模式进行项目探索时，应优先把可独立观察的子组件拆给 `role = "explorer"` 的 subagent。例如 Rust workspace 有多个 crate、前后端分层、或同一任务涉及若干目录时，父代理负责拆分边界和汇总结论，多个 explorer subagent 分别读取对应子组件并返回结构、关键文件、风险点和建议入口。探索子代理默认只做读取和分析，不应擅自修改文件。

`subagent` 运行时使用固定状态机：`queued`、`awaitingApproval`、`running`、`awaitingToolApproval`、`succeeded`、`failed`、`denied`。GUI 只观察状态和摘要，不把子代理的完整 text/thinking delta 混入主聊天流。

子代理内部允许注册完整默认工具，包括 `bash` 和嵌套 `subagent`。为避免递归失控，嵌套 subagent 最大深度固定为 3；超过限制时子代理进入 `failed` 状态。

## 3.4 输出

`TurnResult` 包含：

- `content`
- `reasoning_content`
- `model`
- `usage`
- `mode`
- `session_message_count`
- 角色使用的 provider/model/effort 由配置决定。

`pure-studio` 必须通过 Tauri event 把 `AgentEvent` 转发给 React 前端，实时渲染 `TextDelta`、`ThinkingDelta`、工具调用状态、审批状态、subagent 状态和错误。

`pure-studio` 还必须维护会话运行态快照，用于底部状态栏展示：

- 当前模型和模型上下文窗口。
- 最新请求上下文 token 数。
- 会话累计输入、输出、缓存读取 token 和缓存命中率。
- 按模型可选价格字段估算的费用和货币单位。
- 配置声明的已激活 Skill / MCP 列表。

运行态快照来自 `TurnResult.usage`、角色解析后的模型配置和 `[runtime]` 配置声明。费用只按配置的每百万 token 单价估算，不做货币转换；价格字段缺失时费用显示为未配置。
