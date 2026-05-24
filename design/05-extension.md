# 05 - 扩展点

## 5.1 Provider 扩展

新增模型 provider 时优先扩展 `pl-model`：

- 新增 `ProviderInfo` 构造或配置来源。
- 在 `~/.pure/config.toml` 中新增 provider 和完整 models 列表。
- 实现或复用 `ModelProvider`。
- 适配目标 API 的 request/response wire 格式。

公共消息、事件和错误类型继续来自 `pl-protocol`。

配置里的模型信息会覆盖或补充 bundled model，使用户可以接入自定义模型。

## 5.2 核心流程扩展

需要影响 turn、session、store 或编译阶段时扩展 `pl-core`。

扩展时保持入口层薄：

- UI 输入只在 `pure-studio` 中收集。
- 进入核心层前转换为明确 enum 或 options struct。
- 避免把 bool 参数暴露到核心 API。

需要影响角色、provider/model 路由或配置持久化时扩展 `pl-core` 的配置模块。

## 5.3 前端扩展

`pure-studio` 是当前前端。后续可以增加 CLI、Web 或 IDE 前端，但都应调用 `pl-core`，并复用 `pl-protocol` 的事件和消息类型。

`pure-studio` 使用 Slint 实现跨平台桌面 GUI。桌面端状态通过 `pl-core::StudioStore` 纯异步写入 SQLite，业务配置仍通过 `pl-core::ConfigStore` 读写 `~/.pure/config.toml`。

## 5.4 执行能力扩展

命令执行、文件编辑、工具系统和沙箱能力必须以独立策略接入，并通过权限模型和事件流暴露给核心流程。

首版桌面端允许注册 `bash` 和 `subagent` 工具，但必须使用手动审批。审批结果通过 `AgentEvent` 和 Studio SeaORM 状态记录，拒绝时将拒绝原因作为 tool result 写回会话。
