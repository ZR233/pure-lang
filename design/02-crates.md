# 02 - Crate 设计

## 2.1 pl-protocol

**职责**：定义跨 crate 共享的协议类型。

包含：

- `PureError` / `Result`
- `Message` / `MessageContent` / `MessageRole`
- `AgentEvent` / `AgentEventSender` / `AgentEventReceiver`
- `PermissionLevel`

依赖：

- `serde`
- `serde_json`
- `thiserror`
- `tokio`

`pl-protocol` 不依赖任何内部 crate。

## 2.2 pl-model

**职责**：LLM provider 层。

包含：

- `ProviderInfo`
- `ModelProvider`
- `CompletionRequest` / `CompletionResponse`
- `ModelsManager`
- OpenAI-compatible provider
- Responses API / Chat Completions API wire 适配

依赖：

- `pl-protocol`

`pl-model` 不编排 turn、session 或 store，也不直接承担 CLI 行为。

## 2.3 pl-core

**职责**：核心逻辑层。

包含：

- `CompileMode::{Plan, Auto}`
- `TurnRequest`
- `TurnResult`
- `CoreSession`
- `StudioStore`
- `ProjectRecord`
- `SessionRecord`
- `PureCore::run_turn(...)`
- `PureCore::run_turn_with_options(...)`
- `TurnOptions`
- `ToolApprovalPolicy`
- `PureConfig`
- `ConfigStore`
- `ModelRole`
- `RoleConfig`

`pl-core` 负责：

- 保存和读取核心会话消息。
- 读取和保存 `~/.pure/config.toml`。
- 使用 SeaORM 纯异步读写 `~/.pure/studio/studio_1.sqlite`。
- 校验固定角色到 provider/model/effort 的路由。
- 将自然语言 prompt 转换为模型请求。
- 调用 `pl-model` provider。
- 接收并转发 `AgentEvent`。
- 按 `TurnOptions` 管理工具注册和工具审批。
- 汇总模型返回为 `TurnResult`。
- 提供设置页使用的纯配置构造逻辑。

配置文件由 `pure-studio` 设置页的确认保存动作写入。工具执行必须经过明确的审批策略；当前版本不提供独立沙箱层。

## 2.4 pure-studio

**职责**：Slint 桌面前端。

能力：

- 渲染多个项目和多个会话。
- 订阅 `AgentEvent` 实时渲染文本、思考、错误和工具审批状态。
- 通过原生目录选择器或手动路径把用户输入传给 `pl-core`。

`pure-studio` 只依赖 `pl-core` 和必要 UI 依赖，不直接调用 `pl-model`，也不拥有数据库逻辑。Studio 状态由 `pl-core` 使用 SeaORM 保存到：

```text
~/.pure/studio/studio_1.sqlite
```

`~/.pure/config.toml` 仍是 provider/model/role 配置的唯一来源。

## 2.5 Workspace

```toml
[workspace]
members = [
    "code/pl-protocol",
    "code/pl-model",
    "code/pl-core",
    "code/pure-studio",
]
resolver = "3"

[workspace.dependencies]
pl-protocol = { path = "code/pl-protocol" }
pl-model = { path = "code/pl-model" }
pl-core = { path = "code/pl-core" }
slint = "1.16.1"
slint-build = "1.16.1"
sea-orm = "1.1.20"
rfd = "0.17.2"
toml = "0.8"
```
