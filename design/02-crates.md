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
- `PureCore::run_turn(...)`
- `PureConfig`
- `ConfigStore`
- `ModelRole`
- `RoleConfig`

`pl-core` 负责：

- 保存和读取核心会话消息。
- 读取和保存 `~/.pure/config.toml`。
- 校验固定角色到 provider/model/effort 的路由。
- 将自然语言 prompt 转换为模型请求。
- 调用 `pl-model` provider。
- 接收并转发 `AgentEvent`。
- 汇总模型返回为 `TurnResult`。

配置文件由显式 `purec config` 命令写入。当前版本不执行命令、不写业务文件、不提供独立执行层。

## 2.4 purec

**职责**：命令行编译器前端。

命令：

```powershell
purec "创建 HTTP 服务器"
purec --plan "创建 HTTP 服务器"
purec --auto "创建 HTTP 服务器"
purec config path
purec config show
purec config init
purec --help
```

`purec` 使用 `clap` 解析参数。CLI flag 只存在于入口层，进入核心 API 前会被归一化为 `CompileMode`。
普通对话默认使用 `planner` 角色。

## 2.5 Workspace

```toml
[workspace]
members = [
    "code/*",
]
resolver = "3"

[workspace.dependencies]
pl-protocol = { path = "code/pl-protocol" }
pl-model = { path = "code/pl-model" }
pl-core = { path = "code/pl-core" }
clap = { version = "4", features = ["derive"] }
toml = "0.8"
```
