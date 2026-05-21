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

- CLI 参数只在 `purec` 中解析。
- 进入核心层前转换为明确 enum 或 options struct。
- 避免把 bool 参数暴露到核心 API。

需要影响角色、provider/model 路由或配置持久化时扩展 `pl-core` 的配置模块。

## 5.3 前端扩展

`purec` 当前是唯一前端。后续可以增加 TUI、Web 或 IDE 前端，但都应调用 `pl-core`，并复用 `pl-protocol` 的事件和消息类型。

## 5.4 执行能力扩展

命令执行、文件编辑、工具系统和沙箱能力不属于当前版本。后续实现时应以独立策略或专门 crate 接入，并通过权限模型和事件流暴露给核心流程。
