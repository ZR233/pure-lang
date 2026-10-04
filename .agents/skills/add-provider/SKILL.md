---
name: add-provider
description: Use when adding an LLM provider or preset to the 糊来帮 single route, catalog, and ModelRuntime architecture.
metadata:
  category: guides
  platforms: ["windows", "linux", "macos"]
---

# Add an LLM Provider or Preset

糊来帮当前只有一条模型执行路径：`ResolvedModelRoute -> ModelRuntime`。新增兼容供应商时通过
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
   `design/06-model.md` 与 `design/20-config.md`。

## 修改清单

### 1. `code/pl-model/src/model/catalog/` — canonical 模型目录

- OpenAI/DeepSeek 的默认名单、推荐模型和已记录价格只来自各自嵌入的 `assets/models/<catalog>/model.json`，复用 `ModelInfo`；不在 Rust slug 数组、preset 或初始角色路由复制名单。其他静态目录优先复用 `ModelFamily`。
- 在线发现始终查询配置 `base_url` 的路径前缀 `/models`，不插入额外 `/v1` 或固定 Codex 后端。typed 适配支持 `data/id` 与 `models/slug`；缺失输入模态参照 Codex 默认文本与图片，明确模态与空列表优先。其他 ID-only 缺失参数保持未知，仅同精确 ID 可补已有声明，显式空候选不补回。
- 成功名单替换 API 名单，不把未列出的默认模型补回；手工附加模型保留优先权，explicit 目录不自动转为在线目录。API 价格一律忽略，价格按 provider 绑定目录内大小写敏感精确 ID 关联，缺价用 `Unknown` 保留用量而非零费用。
- 用 `ModelTransportProfile` 声明 protocol、支持的连接模式与默认连接模式。
- 用 `ModelRequestProfile`、`ModelParameter` 和 `ParameterWire` 表达 body/header/effort 差异。
- 先按 `test-quality` 检查已有能力、transport、价格及 request profile 的完整行为证明。仅增加遵循既有规则的模型条目不新增名称、数量或默认值清单镜像；解析、选择、计价或 wire 规则改变时，优先用合成样例增强真实行为测试。需要真实 provider 兼容证据时使用显式 opt-in 验收，不以清单回读替代。

### 2. `code/pl-model/src/provider/mod.rs` — endpoint 数据

- 只有出现新的 canonical endpoint 默认值或服务能力时才增加构造函数。
- `ProviderEndpoint` 不保存默认模型、完整模型目录、protocol 或 connection mode。
- 不按 provider ID、preset ID、slug 或 URL 在 runtime 中推断能力。

### 3. `code/pl-model/src/config/catalog.rs` — preset/catalog 注册

- 注册 `ProviderPreset` 和其绑定的 `ModelCatalogId`。
- 多个套餐可共享同一个模型 catalog，不增加执行分支。
- 确认 custom endpoint override 后 hosted-tool 能力按设计关闭或由显式配置提供。

### 4. `pl-model` 路由与 Studio 装配

- 使用 `ProviderConfig::effective_models()` 作为唯一目录解析入口。
- 使用 `AgentModelConfig::resolve()` 生成 `ResolvedModelRoute`。
- Web Search 从 `plan_web_searches()` 的统一编排入口扩展数据输入或能力矩阵；OpenAI 与
  DeepSeek 的具体规划分别保留在对应函数中，Studio 消费 model 的统一 resolver，工具层不导入模型配置。

### 5. `pl-studio-runtime` 与 Flutter

- Studio first-run 使用默认定义，配置编辑器和模型选择器只消费实例完整 canonical 有效模型 snapshot，不由 Flutter 重新合并默认目录覆盖在线名单。
- 每次启动各支持实例独立探测一次，首屏先用同身份成功缓存或默认 JSON；失败不清空成功缓存，不按 TTL/普通程序升级拒绝回退。成功缓存独立位于 Studio home 的 v2/model-catalogs 下，不写入 config.toml 或 additional_models。
- desired Settings revision 与目录 revision 分开；自动结果按实例 identity/generation 合并到当前状态，别家成功不能让本家 stale。所有解析仍经 effective_models；启动在 route/Profile 校验前装配缓存。外部模型暂不可用保留用户选择并报告 unavailable，不触发全局数据恢复。
- 探测任务登记生命周期 owner，shutdown/启动取消等待网络与提交终态；目录通知仅更新受影响 provider 的安全绑定，不重算在途请求或历史价格。
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
cargo check -p pl-studio-runtime --tests
cargo test -p pl-model
cargo test -p pl-studio-runtime
```

涉及 Studio 或 bridge 时执行 `cargo xtask verify-gui`，GUI 行为变更按根 `AGENTS.md` 补充
integration 验收。需要真实服务时再显式启用 `live-tests`；Studio 可见变更还要使用隔离数据目录运行
`cargo xtask run-gui --driver`，核对配置、模型选择、usage/billing/cache 与运行时错误。
