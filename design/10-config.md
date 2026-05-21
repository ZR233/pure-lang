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

普通对话运行时只读取配置；配置文件不存在时使用内存默认值，不隐式写盘。写入配置必须由显式命令触发，例如 `purec config init`。

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

普通 `purec "prompt"` 默认使用 `planner` 角色。

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

## 10.6 凭据策略

配置允许持久化明文 `bearer_token`，但这会把 API token 直接写入 `~/.pure/config.toml`。文档和默认模板应优先展示 `env_key`，只有用户明确需要时才写 `bearer_token`。

## 10.7 purec 命令

首版提供最小配置命令：

```powershell
purec config path
purec config show
purec config init
```

- `path` 输出配置文件路径。
- `show` 输出当前解析后的 TOML 配置。
- `init` 创建默认配置文件；如果文件已存在则报错，避免覆盖用户配置。
