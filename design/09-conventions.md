# 09 - 约定

## 9.1 Crate 命名

- 库 crate 使用 `pl-` 前缀。
- Flutter bridge crate 使用 `pl-studio-bridge`，Flutter app package 使用 `pure_studio_flutter`。
- 公共协议类型放入 `pl-protocol`。

## 9.2 依赖方向

```text
pl-protocol
    ↑
pl-trace
    ↑
pl-model
    ↑
pl-core
    ↑
pl-studio-bridge
    ↑
pure-studio-flutter
```

允许 `pl-core` 同时直接依赖 `pl-protocol`、`pl-trace`、`pl-model` 和 `pl-lsp`。

禁止 `pl-model` 依赖 `pl-core`，避免循环依赖。

## 9.3 异步 Trait

禁止使用 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。

异步 trait 方法使用原生 RPITIT，并显式声明 `Send` bound：

```rust
pub trait ModelProvider: Send + Sync {
    fn stream_complete(&self, request: CompletionRequest, event_tx: AgentEventSender)
        -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;
}
```

## 9.4 参数设计

核心 API 不暴露语义模糊的 `bool` 或 `Option<bool>`。

前端输入应在 `pure-studio-flutter` 边界转换为明确类型，例如 `CompileMode`。

## 9.5 模块和导出

模块默认私有。公开 API 通过 crate 根明确 `pub use`。

`pl-core` 可以重导出常用 `pl-protocol` 类型，方便核心层用户使用；raw `pl-trace` 类型只作为内部运行事件边界，不应作为 Studio wire 或前端事实源。

## 9.6 文档口径

- 项目名：Pure-Lang。
- 桌面编译器前端：`pure-studio-flutter`。
- 核心逻辑层：`pl-core`。
- LLM provider 层：`pl-model`。
- 公共协议层：`pl-protocol`。
- 内部 trace 协议层：`pl-trace`。

当前版本不承诺独立沙箱。工具系统必须由明确 `PermissionMode` 和工具访问分类控制；默认模式为 `request-approval`。旧 `ToolApprovalPolicy::AutoAllow | Manual | DenyAll` 只作为兼容构造存在，新增设计不得再把 `AutoAllow` 描述为无条件直接执行。

## 9.7 配置约定

- 配置文件固定为 `~/.pure/config.toml`。
- 本地 TOML 使用 `snake_case`。
- 不设置 `active_provider`。
- 固定角色 key：`explorer`、`planner`、`executor`、`reviewer`。
- 普通对话默认使用 `planner`。
- provider 必须持久化完整 models 列表，以支持用户自定义模型。
