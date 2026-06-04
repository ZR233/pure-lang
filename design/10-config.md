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

SQLite 只保存 Studio 状态，例如项目、会话、消息、工具审批、agent 状态事件和应用设置，并由 `pl-core` 通过 SeaORM 纯异步访问。provider/model/role 配置仍只由 `~/.pure/config.toml` 表达。

普通对话运行时读取配置；当配置文件不存在时，`pure-studio` 设置页展示默认配置草稿。写入配置必须由用户在具体设置操作中显式触发，例如 provider 编辑页保存、删除 provider、选择默认 provider 或调整角色路由。

`pure-studio` 设置页不提供全局保存按钮；各设置项确认后即时写入 `~/.pure/config.toml`，校验失败时只展示错误并保留当前页面状态。

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

为了兼容旧配置，读取 TOML 时允许缺失某个 `[roles.<key>]` 块。缺失角色按默认模型补齐：按配置 key 顺序取首个 provider，并使用该 provider 的 `default_model` 和该模型的第一个 `reasoning_efforts`。如果角色块存在但引用了不存在的 provider、model 或 effort，配置仍视为无效并返回错误。

## 10.4 TOML 示例

本地 TOML 使用 `snake_case`，不同于 API wire 格式。

```toml
schema_version = 2

[runtime]
permission_mode = "request-approval"
active_skills = ["rust", "git", "doc"]
active_mcp_servers = ["github", "filesystem"]

[skills]
enabled = true
auto_learn = true
project_dir = "skills"
user_dir = "~/.pure/skills"
external_dirs = []
disabled = []
auto_learn_min_tool_calls = 5

[skills.system]
enabled = true

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
bearer_token = "sk-..."
default_model = "deepseek-v4-flash"
wire_api = "chat"

[[providers.deepseek.models]]
slug = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
description = "DeepSeek fast reasoning model with thinking mode."
context_window = 1000000
max_context_window = 1000000
max_output_tokens = 384000
currency = "CNY"
input_price_per_mtok = 1.0
output_price_per_mtok = 2.0
cache_read_price_per_mtok = 0.02
reasoning_efforts = ["high", "max"]
capabilities = ["streaming", "function_calling", "parallel_tool_calls", "reasoning"]
input_modalities = ["text"]
base_instructions = ""
truncation_policy = { mode = "tokens", limit = 10000 }

[[providers.deepseek.models]]
slug = "deepseek-v4-pro"
display_name = "DeepSeek V4 Pro"
description = "DeepSeek flagship reasoning model with thinking mode."
context_window = 1000000
max_context_window = 1000000
max_output_tokens = 384000
currency = "CNY"
input_price_per_mtok = 3.0
output_price_per_mtok = 6.0
cache_read_price_per_mtok = 0.025
reasoning_efforts = ["high", "max"]
capabilities = ["streaming", "function_calling", "parallel_tool_calls", "reasoning"]
input_modalities = ["text"]
base_instructions = ""
truncation_policy = { mode = "tokens", limit = 10000 }
```

## 10.5 Provider 和 Model

`providers` 可保存多个 provider。每个 provider 持久化：

- provider 运行配置，例如 `name`、`base_url`、`bearer_token`、`wire_api`。
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
- `currency`
- `input_price_per_mtok`
- `output_price_per_mtok`
- `cache_read_price_per_mtok`
- `reasoning_efforts`
- `capabilities`
- `input_modalities`
- `truncation_policy`
- `base_instructions`

`used_fallback` 是运行时状态，不写入 TOML。

价格字段为可选字段，用于本地 UI 估算费用。`currency` 只作为展示单位，系统不做汇率转换；三个 `*_price_per_mtok` 字段均表示每百万 token 单价。缺失任一参与计算的价格或缺失 `currency` 时，本次 token 仍进入上下文和用量统计，但费用标记为未计价。

会话总费用按货币分组展示，例如 `CNY 0.04 + USD 0.01`。系统不会把不同货币相加，也不会根据当前模型重新估算历史调用；每次 inference 都按当次实际使用模型的价格配置生成费用 delta。

Bundled DeepSeek 模型按中国官网人民币 API 价格配置：`deepseek-v4-flash` 为缓存命中输入 0.02 元、缓存未命中输入 1 元、输出 2 元；`deepseek-v4-pro` 为缓存命中输入 0.025 元、缓存未命中输入 3 元、输出 6 元。`input_price_per_mtok` 表示缓存未命中输入价，`cache_read_price_per_mtok` 表示缓存命中输入价。

配置里的模型会覆盖或补充 bundled model。角色引用的 model 必须存在于对应 provider 的 `models` 中。

## 10.6 运行态声明

`[runtime]` 保存本地 Studio 运行态展示所需的可选声明。首版字段：

- `permission_mode`
- `active_skills`
- `active_mcp_servers`

`permission_mode` 是会话 turn 的默认权限模式，缺失时按 `request-approval` 处理。可选值：

- `request-approval`
- `auto-review`
- `full-access`

Pure v1 的权限模式是本地策略层，不是 OS 沙箱。`request-approval` 和 `auto-review` 都直接允许 workspace 内读写；工具请求 workspace 外路径或 workspace 外 cwd 时分别走用户审批或 reviewer 审批。`full-access` 会放宽 Pure 文件工具和 `bash.workingDirectory` 的 workspace 边界并直接放行。

`active_skills` 和 `active_mcp_servers` 仅声明旧版 GUI 状态栏展示所需的 Skill / MCP 名称，不负责安装、启动或连接真实 Skill/MCP 管理器。缺失 `[runtime]` 或字段时按空列表处理；缺失 `permission_mode` 时按 `request-approval` 处理。旧配置里的 `workspace-write` 兼容读取为 `request-approval`，新配置不再输出该值。

真实 skills 能力由 `[skills]` 配置和项目目录驱动。`active_skills` 不作为启停来源，不影响模型可见的 skills 列表，也不作为当前会话状态栏 Skills 的来源。Studio 当前会话的 `activeSkills` 由该会话中成功执行过的 `skill_view` 工具结果派生，表示 skill 内容已经进入上下文。

## 10.7 Skills 配置

`[skills]` 控制本地 skills 系统：

- `enabled`：是否启用 skills 发现、prompt 注入和工具注册，默认 `true`。
- `auto_learn`：是否在 Studio 主 turn 结束后启动后台 reviewer 自动沉淀项目 skill，默认 `true`。
- `project_dir`：项目级 skills 目录，相对 `workspace_root` 解析，默认 `skills`。
- `user_dir`：用户级只读 skills 目录，默认 `~/.pure/skills`。
- `system.enabled`：是否启用内置系统 skills，默认 `true`。
- `external_dirs`：额外只读 skills 目录列表，默认空。
- `disabled`：禁用的 skill 名称列表，默认空。
- `auto_learn_min_tool_calls`：触发自学习 review 的最少工具调用数，默认 `5`。

加载优先级固定为：项目 skills > 用户 skills > 系统 skills > external dirs。同名 skill 只暴露最高优先级来源。自学习和 `skill_manage` 写入只作用于项目 skills 目录，不会修改用户目录、系统目录或外部目录。

系统 skills 来自编译进 `pl-core` 的内置资源，并同步缓存到 `~/.pure/skills/.system/`。该目录由 Pure 管理，用户需要覆盖系统 skill 时应在项目 `skills/` 目录创建同名 skill。

## 10.8 配置草稿

配置构造和校验的纯逻辑属于 `pl-core`。`pure-studio` 设置页可以使用默认配置草稿，并支持：

- 默认选中 DeepSeek provider，也可切换为 OpenAI、Zhipu GLM API 或 Zhipu GLM Coding Plan provider。
- 至少配置一个 provider。
- 可继续添加多个 provider 实例，允许同类 provider 重复，例如 `deepseek`、`deepseek-2`、`openai`、`openai-work`。
- 每个 provider key 必须唯一。
- DeepSeek 模板来自 `ProviderInfo::deepseek(None)`，OpenAI 模板来自 `ProviderInfo::openai(None)`，智谱模板来自 `ProviderInfo::zhipu_api(None)` 和 `ProviderInfo::zhipu_coding_plan(None)`。
- 每个 provider 的模型列表包含模板默认模型，并可追加用户自定义模型。
- 用户选择一个默认 provider；四个模型角色默认都指向该 provider 的默认模型和默认 effort。

设置项写入前必须完成本地校验：

- provider key 非空且唯一。
- API key 非空。
- provider 的 default model 必须存在于该 provider 模型列表中。
- 同一 provider 下模型 slug 不重复。
- 角色引用的默认模型必须声明至少一个 `reasoning_efforts`，用于生成角色 `effort`。

## 10.9 pure-studio 设置页

`pure-studio` 设置页复用 `pl-core` 的配置类型和校验逻辑，首版覆盖：

- DeepSeek / OpenAI / Zhipu GLM API / Zhipu GLM Coding Plan provider。
- API key、base URL、provider key 和显示名。
- provider 默认模型和自定义模型。
- 四个模型角色到 provider/model/effort 的路由。
- Security 标签页选择权限模式：请求批准、替我审批、完全访问。选择后即时写入 `[runtime].permission_mode`。

每次设置项写入前必须执行 `PureConfig::validate()`；失败时只在 UI 中展示错误，不写入磁盘。

设置页 UI 按 React 页面模块拆分，顶层 App 负责页面路由和共享状态，具体页面放在 `src/pages`，可复用组件放在 `src/components`，Tauri 命令封装放在 `src/lib`。Provider 标签页优先从 `PureConfig.providers` 派生 provider 卡片列表，不引入新的配置存储。

Provider 标签页必须提供结构化编辑能力：

- Provider 列表页只提供一个“添加供应商”入口；点击后进入 Provider 编辑页。
- 新增 Provider 默认使用模板列表第一项，自动生成唯一 provider key；编辑页通过供应商类型下拉切换 DeepSeek / OpenAI / Zhipu GLM API / Zhipu GLM Coding Plan 等模板。
- 编辑 provider key、显示名、base URL、API key 和默认模型。
- 以 provider 卡片作为主要信息载体，展示 provider key、base URL、状态、wire API、默认模型、模型数量和更新时间等摘要信息。
- Provider 列表页不直接编辑字段；点击卡片编辑按钮进入 provider 编辑页。
- Provider 编辑页使用本地草稿，提供保存和取消按钮；保存成功后即时写入配置并返回列表，取消或返回列表不修改当前配置。
- Provider 卡片必须提供删除按钮；删除和默认 provider 选择都即时写入配置，列表页不使用独立右侧详情面板。
- Provider 标签页不展示 raw TOML 配置编辑器，确保列表和编辑页占据主要工作区。
- 展示 provider 模板自带的默认模型列表。
- 允许追加用户自定义模型，保存时由 `pl-core` 将模板默认模型排在前面，再追加用户自定义模型。
- 模型列表应展示关键参数，例如上下文窗口、最大输出 token、自动压缩阈值、temperature、reasoning efforts、capabilities、输入模态和截断策略。
- `wire_api` 由 provider 模板固定，不在 UI 中提供选择；DeepSeek 固定为 `chat`，OpenAI 固定为 `responses`，两个智谱模板固定为 OpenAI 兼容 `chat`。运行时由 `pl-model` 内部 typed wire 层转换为 async-openai 请求，用户配置中的 base URL 不自动追加或改写版本路径，只去除末尾多余 `/` 后与对应 API path 拼接。
- 智谱 OpenAI 兼容 `chat` 请求固定使用流式 `chat/completions`；模型 `reasoning_efforts` 使用 `enabled` / `none` 表达 thinking 开关，不发送 wire-level `reasoning_effort`。thinking 按官方请求体 `thinking.type = enabled/disabled` 控制，开启时设置 `clear_thinking = false` 并保留/回传历史 `reasoning_content`；Coding Plan 模板使用专属 base URL `https://open.bigmodel.cn/api/coding/paas/v4`。
- 写入前由 `pl-core` 构造 `PureConfig` 并执行 `PureConfig::validate()`；校验失败时只在 UI 中展示错误，不写入磁盘。

Roles 标签页必须展示固定四个角色：探索者、计划者、执行者、审查者。每个角色提供 provider、model 和 effort 下拉选择。provider 改变时，model 默认切换为该 provider 的 `default_model`；model 改变时，effort 默认切换为该模型的第一个可用 effort。角色路由下拉变更后即时提交完整 roles 快照，`pl-core` 统一校验后写入 `~/.pure/config.toml`。

桌面窗口必须支持自由缩放。`pure-studio` 只声明首选窗口尺寸，不把 UI 绑定到固定宽高；设置页内容跟随窗口尺寸自适应。Provider 标签页在常规桌面宽度使用单栏 provider 卡片列表，卡片内部承载摘要、操作和展开编辑内容；在窄窗口下保持单栏滚动并压缩卡片元信息，避免表格和编辑区域被裁剪。

为了支持设计验证，`pure-studio` 的 React 页面应支持 Vite dev server 中的 fixture 状态预览。Provider 设置页的本地预览入口固定为：

```powershell
npm --prefix code/pure-studio run dev
```

Vite 预览只用于布局和视觉对照，最终应用行为仍以 Tauri 运行结果为准。

聊天界面应展示 agent 活动面板，信息来自 Studio SQLite 中的 `agent_events` 和当前实时事件流。面板只展示路径、角色、状态、任务摘要、最终摘要或错误，不展示子代理完整推理流。

聊天界面使用双栏布局：左侧项目/会话栏和主聊天区，不再展示右侧工具历史面板。主聊天区底部状态栏展示当前模型、会话、上下文使用量、按货币分组的费用估算、Skill 数量和 MCP 数量；Skill/MCP 默认只显示数量，悬浮或键盘聚焦时展示完整列表。子代理列表展示每个 agent 的运行费用摘要。

## 10.9 凭据策略

配置允许持久化明文 `bearer_token`，但这会把 API token 直接写入 `~/.pure/config.toml`。当前运行时只使用配置中保存的 `bearer_token` 作为 provider API key。

`env_key` 字段保留为旧 TOML schema 的兼容字段和测试辅助信息，不作为运行时鉴权来源，也不在 `pure-studio` provider 设置界面展示或写入。`pure-studio` 设置页按用户确认会把输入的 API key 明文写入对应 provider 的 `bearer_token`。后续版本可以增加系统凭据库模式，但首版不从环境变量读取 provider key。
