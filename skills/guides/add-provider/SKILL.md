---
name: add-provider
description: Use when adding a new LLM provider/vendor to Pure-Lang. Covers cross-crate registration chain, ProviderKind/ProviderInfo/ProviderRuntime patterns, protocol choice, and default models.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Add a New LLM Provider

在 Pure-Lang 中添加新供应商时，必须按顺序修改以下所有 crate 中的对应位置。

## 前置确认

1. **确定 API 协议族**：OpenAI-compatible Chat Completions（走 `OpenAiTransportProvider` + `OpenAiProtocol::chat(…)`）、OpenAI Responses API（走 `OpenAiProtocol::responses()`），或全新协议（需要新 transport + protocol 实现）。
2. **确定基础 URL**：供应商的 API 入口。
3. **确定默认模型列表**：至少一个默认模型 slug。
4. **确定 reasoning/thinking 风格**：是否需要 `ChatReasoningStyle` 变体，序列化格式是否兼容现有三种（Plain / DeepSeek / Zhipu）。
5. **确定 tool wire policy**：`NativeCustomTools`（原生工具调用）还是 `FunctionFallback`（函数回退）。
6. **确定命名约定**：ProviderKind 变体名（PascalCase）、ProviderTemplateKind key（snake_case）、UI 显示名。

## 修改清单

### 1. `pl-model/src/provider_info.rs` — ProviderKind + ProviderInfo 工厂

- 向 `ProviderKind` 枚举添加新变体。
- 添加 `ProviderInfo::new_provider(base_url: Option<String>) -> Self` 工厂方法。
- 更新测试（验证 base_url、default_model、provider_kind）。

### 2. `pl-model/src/default_models.rs` — 默认模型列表

- 添加模型 slugs 常量（`NEW_PROVIDER_DEFAULT_MODEL_SLUGS`）。
- 添加 `new_provider_default_model_slugs()` 访问函数。
- 添加 `new_provider_reasoning_efforts` 常量（如果适用）。
- 在 `default_models()` 中添加模型条目（`ModelInfo::new(…)` 或 `ModelInfo::text(…)` 等工厂）。
- 在测试中验证新模型列表。

### 3. `pl-model/src/provider/<new_provider>.rs` — Provider 运行时

- 创建新 provider 模块文件，例如 `provider/zhipu_coding_plan.rs`。
- 如果复用 `OpenAiTransportProvider`：包装为结构体，所有方法委托给 `inner`。
- 初始化时传入：
  - `bundled_models(new_provider_default_model_slugs())`
  - `configured_models`（由外部注入）
  - 对应的 `OpenAiProtocol` 模式
  - `ProviderCapabilities::all()` 或按需位组合
- 如果使用全新协议：实现 `ModelProvider` trait 的所有方法（`info`、`capabilities`、`stream_complete`、`auth_token`、`model_info`、`list_models`、`effective_model_capabilities`、`default_model`）。

### 4. `pl-model/src/provider.rs` — ProviderRuntime 枚举 + 工厂 dispatch

- 向 `ProviderRuntime` 枚举添加新变体。
- 在 `create_provider` / `create_provider_with_models` 的 `match provider_kind` 中添加新分支。
- 实现所有 trait 方法的 match 分支（`info`、`capabilities`、`stream_complete`、`auth_token`、`model_info`、`list_models`、`effective_model_capabilities`、`default_model`）。
- 更新测试。

### 5. `pl-model/src/lib.rs` — 公开导出

- 添加 `pub use provider::NewProvider;`。
- 添加 `pub use default_models::new_provider_default_model_slugs;`（如果适用）。

### 6. `pl-core/src/first_run.rs` — ProviderTemplateKind

- 向 `ProviderTemplateKind` 枚举添加新变体。
- 更新 `all()` 的返回数组大小（`[Self; N+1]`）。
- 在 `from_key()`、`key()`、`key_prefix()`、`display_name()`、`provider_info()`、`default_model_slugs()` 中添加新分支。
- 更新相关测试。

### 7. `pl-core/src/config_editor.rs` — 模板推断

- 在 `infer_provider_template_kind()` 和所有测试 mock/构造中添加新变体的 match 分支。

### 8. Flutter provider settings mapping

- 确认 `pl-studio-bridge` 返回的 provider/template JSON 与 Flutter Settings 页使用的字符串映射一致。

### 9. `design/10-config.md` — 文档

- 在 `provider_kind` 可选值列表中添加新值。
- 在配置模板示例部分添加新供应商模板示例。

## 跨 crate 协调提示

- 所有 match 语句必须穷尽，否则编译失败。建议用 `cargo check -p pl-model -p pl-core` 验证所有 match 覆盖完整。
- `ProviderRuntime` 是 `Arc` 包装的动态分发；新变体必须实现 `Debug + Send + Sync`。
- 如果新供应商的 API wire 格式与现有 `OpenAiProtocol` 不兼容，需要新增 `OpenAiProtocol` 变体或在 `protocol/` 下新建子模块。

## 参考

- 项目记忆：crate 命名（`pl-` 前缀）、参数设计（避免裸 bool）、模块导出（`pub use`）、API 边界序列化（`camelCase`）、文档同步流程。
