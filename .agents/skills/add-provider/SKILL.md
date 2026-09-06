---
name: add-provider
description: Use when adding an LLM provider or preset to Pure-Lang's single route, catalog, and ModelRuntime architecture.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Add an LLM Provider or Preset

Pure-Lang 当前只有一条模型执行路径：`ResolvedModelRoute -> ModelRuntime`。新增兼容供应商时通过
endpoint、catalog、model profile 和 preset 数据表达差异，不新增 provider class、runtime trait、
factory dispatch 或兼容 wrapper。

## 前置确认

以下信息先从用户需求、现有配置和供应商文档核实，不是逐项向用户审批；仅在无法核实且影响
实现选择时按根 `AGENTS.md` 询问。已授权的新增工作连续完成到相应验证与交付。

1. 确定 wire API：Responses 或 Chat Completions。
2. 确定模型 transport：Responses 可声明 HTTP/WS；Chat 只允许 HTTP。
3. 确定 endpoint：base URL、headers、凭据、tool wire policy 与服务能力。
4. 确定模型目录：slug、能力、价格、上下文、request profile、参数 wire 和基础指令。
5. 确定是否只是新增 preset。共享同一 endpoint 形态和 catalog 的产品套餐通常只需新 preset。
6. 只有现有 OpenAI-compatible codec 无法表达 wire 时，才设计第二种 typed codec；先同步
   `design/07-model.md` 与 `design/10-config.md`。

## 修改清单

### 1. `code/pl-model/src/model/catalog/` — canonical 模型目录

- 优先复用 `ModelFamily`，单模型只声明 slug、显示信息、窗口、价格等差异字段。
- 用 `ModelTransportProfile` 声明 protocol、支持的连接模式与默认连接模式。
- 用 `ModelRequestProfile`、`ModelParameter` 和 `ParameterWire` 表达 body/header/effort 差异。
- 更新参数化 catalog 测试，覆盖能力、transport、价格和 request profile。

### 2. `code/pl-model/src/provider/mod.rs` — endpoint 数据

- 只有出现新的 canonical endpoint 默认值或服务能力时才增加构造函数。
- `ProviderEndpoint` 不保存默认模型、完整模型目录、protocol 或 connection mode。
- 不按 provider ID、preset ID、slug 或 URL 在 runtime 中推断能力。

### 3. `code/pl-core/src/model_config/catalog.rs` — preset/catalog 注册

- 注册 `ProviderPreset` 和其绑定的 `ModelCatalogId`。
- 多个套餐可共享同一个模型 catalog，不增加执行分支。
- 确认 custom endpoint override 后 hosted-tool 能力按设计关闭或由显式配置提供。

### 4. `pl-core` 配置与规划

- 使用 `ProviderConfig::effective_models()` 作为唯一目录解析入口。
- 使用 `AgentModelConfig::resolve()` 生成 `ResolvedModelRoute`。
- Web Search 从 `plan_web_searches()` 的统一编排入口扩展数据输入或能力矩阵；OpenAI 与
  DeepSeek 的具体规划分别保留在对应函数中，不在 Studio 复制 resolver。

### 5. `pl-studio-runtime` 与 Flutter

- Studio first-run/config editor 只消费 canonical preset/catalog snapshot。
- `default_model` 仅是 Studio 新建/编辑 provider 时生成角色 route 的投影，不进入 runtime provider。
- Flutter 只渲染 bridge 返回的 transport、能力、价格和参数候选，不按 preset ID 推断。

### 6. 新 wire API（仅确有需要时）

- 在 `code/pl-model/src/runtime/` 增加私有 typed codec，并先归一化为同一 raw event/error。
- 继续复用 canonical request/history/tool 转换、stream lifecycle、tool identity、accumulator、
  error classification、retry budget 和凭证脱敏。
- 协议差异通过穷尽的 `ProviderWireProtocol` / `ProviderConnectionMode` 分派；不要建立厂商 runtime。

## 禁止事项

- 不新增 `ModelProvider`、`SharedModelProvider`、`ProviderRuntime` 或厂商 provider class。
- 不新增 `create_provider*` 工厂或 provider-specific decoder。
- 不把 model、stream、store、continuation、trace 或 transport session 放回 `CompletionRequest`。
- 不把 raw content、finish reason、trace events 或 sequence 放回 `CompletionResponse`。
- 不为旧配置或旧 API 增加 alias、shim 或双轨实现。

## 验证

模型与配置路径变更先运行受影响 crate 的检查与测试；以下为入口，按实际影响选择，提交前
完整门禁以根 `AGENTS.md` 为准：

```powershell
cargo check -p pl-model --tests
cargo check -p pl-core --tests
cargo test -p pl-model
cargo test -p pl-core
```

涉及 Studio 或 bridge 时执行 `cargo xtask verify-gui`，GUI 行为变更按根 `AGENTS.md` 补充
integration 验收。需要真实服务时再显式启用 `live-tests`；Studio 可见变更还要使用隔离数据目录运行
`cargo xtask run-gui --driver`，核对配置、模型选择、usage/billing/cache 与运行时错误。
