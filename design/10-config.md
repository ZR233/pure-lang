# 10 - 持久化配置

## 10.1 配置位置

Pure-Lang 使用用户目录下的 `.pure` 目录保存本地配置：

```text
~/.pure/config.toml
```

Windows 下对应：

```text
%USERPROFILE%\.pure\config.toml
```

`pure-studio` 的桌面端状态单独保存在：

```text
~/.pure/studio/studio_1.sqlite
```

SQLite 只保存 Studio 状态，例如项目、会话、消息、工具审批和应用设置，并由 `pl-core` 通过 SeaORM 纯异步访问。provider/model/role 配置仍只由 `~/.pure/config.toml` 表达。

普通对话运行时读取配置；当配置文件不存在时，`pure-studio` 设置页展示默认配置草稿。写入配置必须由用户显式保存触发。

`pure-studio` 设置页同样必须由用户显式保存后才写入 `~/.pure/config.toml`。

## 10.2 配置职责

配置能力属于 `pl-core`：

- 定义 TOML schema。
- 解析和保存 `~/.pure/config.toml`。
- 校验角色到 provider/model/effort 的路由。
- 将配置转换为运行时 `ProviderInfo` 和模型列表。

`pl-model` 只消费已经解析好的 provider 和模型信息，不负责文件 IO 或路径定位。

## 10.3 角色路由

配置不使用 `active_provider`。系统固定四个模型角色：

| TOML key | 中文角色 | 用途 |
| --- | --- | --- |
| `explorer` | 探索者 | 代码、文档和上下文探索 |
| `planner` | 计划者 | 默认对话和计划生成 |
| `executor` | 执行者 | 后续执行型任务 |
| `reviewer` | 审查者 | 代码审查和结果检查 |

普通桌面对话默认使用 `planner` 角色。

每个角色必须配置：

- `provider`
- `model`
- `effort`

`effort` 使用字符串枚举，首版校验 against 对应模型的 `reasoning_efforts`。

## 10.4 TOML 示例

本地 TOML 使用 `snake_case`，不同于 API wire 格式。

```toml
schema_version = 1

[roles.explorer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.planner]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.executor]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.reviewer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
env_key = "API_KEY_DEEPSEEK"
default_model = "deepseek-v4-flash"
wire_api = "chat"

[[providers.deepseek.models]]
slug = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
description = "DeepSeek fast reasoning model with thinking mode."
context_window = 1000000
max_context_window = 1000000
max_output_tokens = 384000
reasoning_efforts = ["high", "max"]
capabilities = ["streaming", "function_calling", "parallel_tool_calls", "reasoning"]
input_modalities = ["text"]
base_instructions = ""
truncation_policy = { mode = "tokens", limit = 10000 }
```

## 10.5 Provider 和 Model

`providers` 可保存多个 provider。每个 provider 持久化：

- provider 运行配置，例如 `name`、`base_url`、`env_key`、`bearer_token`、`wire_api`。
- 完整 `models` 列表。

模型配置必须能完整表达运行时 `ModelInfo` 的可持久化字段：

- `slug`
- `display_name`
- `description`
- `context_window`
- `max_context_window`
- `auto_compact_token_limit`
- `default_temperature`
- `max_output_tokens`
- `reasoning_efforts`
- `capabilities`
- `input_modalities`
- `truncation_policy`
- `base_instructions`

`used_fallback` 是运行时状态，不写入 TOML。

配置里的模型会覆盖或补充 bundled model。角色引用的 model 必须存在于对应 provider 的 `models` 中。

## 10.6 配置草稿

配置构造和校验的纯逻辑属于 `pl-core`。`pure-studio` 设置页可以使用默认配置草稿，并支持：

- 默认选中 DeepSeek provider，也可切换为 OpenAI provider。
- 至少配置一个 provider。
- 可继续添加多个 provider 实例，允许同类 provider 重复，例如 `deepseek`、`deepseek-2`、`openai`、`openai-work`。
- 每个 provider key 必须唯一。
- DeepSeek 模板来自 `ProviderInfo::deepseek(None)`，OpenAI 模板来自 `ProviderInfo::openai(None)`。
- 每个 provider 的模型列表包含模板默认模型，并可追加用户自定义模型。
- 用户选择一个默认 provider；四个模型角色默认都指向该 provider 的默认模型和默认 effort。

设置页保存前必须完成本地校验：

- provider key 非空且唯一。
- API key 非空。
- provider 的 default model 必须存在于该 provider 模型列表中。
- 同一 provider 下模型 slug 不重复。
- 角色引用的默认模型必须声明至少一个 `reasoning_efforts`，用于生成角色 `effort`。

## 10.7 pure-studio 设置页

`pure-studio` 设置页复用 `pl-core` 的配置类型和校验逻辑，首版覆盖：

- DeepSeek / OpenAI provider。
- API key、base URL、provider key 和显示名。
- provider 默认模型和自定义模型。
- 四个模型角色到 provider/model/effort 的路由。

保存前必须执行 `PureConfig::validate()`；失败时只在 UI 中展示错误，不写入磁盘。

设置页 UI 按 React 页面模块拆分，顶层 App 负责页面路由和共享状态，具体页面放在 `src/pages`，可复用组件放在 `src/components`，Tauri 命令封装放在 `src/lib`。Provider 标签页优先从 `PureConfig.providers` 派生列表和详情，不引入新的配置存储。当前阶段的 `Add Provider` 是视觉和交互占位，只更新 UI 状态提示，不创建 provider，也不写入 `~/.pure/config.toml`。

桌面窗口必须支持自由缩放。`pure-studio` 只声明首选窗口尺寸，不把 UI 绑定到固定宽高；设置页内容跟随窗口尺寸自适应。Provider 标签页在常规桌面宽度使用 provider 列表和详情双栏布局，在窄窗口下切换为上下堆叠并允许滚动，避免表格和详情面板被裁剪。

为了支持设计验证，`pure-studio` 的 React 页面应支持 Vite dev server 中的 fixture 状态预览。Provider 设置页的本地预览入口固定为：

```powershell
npm --prefix code/pure-studio run dev
```

Vite 预览只用于布局和视觉对照，最终应用行为仍以 Tauri 运行结果为准。

## 10.8 凭据策略

配置允许持久化明文 `bearer_token`，但这会把 API token 直接写入 `~/.pure/config.toml`。文档和默认模板应优先展示 `env_key`，只有用户明确需要时才写 `bearer_token`。

`pure-studio` 设置页按用户确认会把输入的 API key 明文写入对应 provider 的 `bearer_token`。后续版本可以增加系统凭据库或环境变量模式，但首版不改变现有 TOML schema。
