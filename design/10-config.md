# 10 - 模型配置值对象与 Studio 持久化配置

## 10.1 配置位置

Pure-Lang 使用用户目录下的 `.pure` 目录保存本地配置：

```text
~/.pure/config.toml
```

Windows 下对应：

```text
%USERPROFILE%\.pure\config.toml
```

`pure-studio` 的桌面端产品状态与 Thread 历史统一保存在：

```text
~/.pure/studio/studio.sqlite
```

单库保存项目、Thread、Turn、Item、input、interaction、attachment 与 Task 产品表。Flutter
临时 UI 状态不入库，实时流也不保存 replay journal。数据库由 `pl-studio-runtime` 通过
SeaORM 2.0 异步访问；schema v14 与不兼容库/一次性 v13 附件迁移合同见
`19-studio-storage-and-diagnostics.md`。
`pl-core` 的正常依赖树不包含 SeaORM。provider/model/role 配置仍只由
`~/.pure/config.toml` schema 15 表达。

`ConfigRuntime` 在 `startStudioRuntime` 时读取配置；此后普通对话和设置查询只读内存 canonical
snapshot。当配置文件不存在时设置页展示内存中的默认配置。外部文件变化只有显式
`reloadSettingsFromDisk` 才能应用。普通设置项在用户修改后即时写入配置；独立新增/编辑页面保留
本地草稿，必须点击页面内保存按钮才写入配置，取消则丢弃草稿。当前且唯一支持的格式为 schema
15。Studio 启动时，旧 schema、未知版本、无法解析、含内联 provider 凭据或无法校验的配置均视为
内容不兼容：不迁移、不导入旧字段或旧 provider 凭据，先把原始字节备份到同目录唯一的
`config.toml.rejected.<timestamp>.bak`，再原子替换为当前默认配置并继续启动；仅按默认 provider id
注入已有系统凭据。备份路径冲突时递增后缀且不得覆盖已有备份。启动成功后桌面宿主返回一次性恢复
报告，GUI 展示备份路径；其他宿主至少记录脱敏诊断日志。运行期显式
`reloadSettingsFromDisk` 仍严格校验，不自动备份或替换。

仅配置文件不存在时使用 `StudioConfig::default_config()` 构造内存初始配置且不产生恢复报告。文件
读取、默认 provider 系统凭据读取、备份写入或默认配置原子替换失败不属于配置内容不兼容，必须
fail closed 并保留原文件；默认配置替换失败时已经完整写入的备份可以保留。

`pure-studio` 的所有 Settings command 必须携带 `expectedSettingsRevision`，成功只返回完整
`SettingsStateSnapshot`，由 Flutter 原子替换 Settings 领域；不得返回 Studio 聚合状态，也不得
携带 `configJson`、`generalSettingsJson` 或 raw map。CAS 或校验失败时保留当前 canonical 状态，
不覆盖新配置。Instructions 文本作为普通设置组展示时，在输入停止后自动写入
`~/.pure/config.toml`；Provider 新增/编辑等独立页面不自动保存。

## 10.2 配置职责

`pl-core` 只负责产品无关的模型配置值对象：

- `AgentModelConfig`、`ProviderConfig`、`ModelRouteConfig`。
- 校验动态角色到 provider/model/effort 的路由。
- 将路由解析为运行时 `ProviderEndpoint` 和唯一选中的不可变 `ModelInfo`。

`pl-studio-runtime` 负责：

- `StudioConfig` 与唯一支持的 schema version 15。
- `~/.pure/config.toml` 路径、serde TOML 解析、原子保存和默认值。
- Studio instructions、skills、MCP、runtime 和 UI 配置。
- 生成 Thread 首轮固定的 instruction snapshot。

`pl-model` 只消费已经解析好的 provider 和模型信息，不负责文件 IO 或路径定位。

## 10.3 角色路由

配置不使用 `active_provider`。`pl-core` 的角色表是动态字符串映射；Studio 默认提供以下
四个产品角色，但其他宿主可定义不同角色：

| TOML key | 中文角色 | 用途 |
| --- | --- | --- |
| `explorer` | 探索者 | 代码、文档和上下文探索 |
| `planner` | 计划者 | Task 根聊天、计划生成、任务协调、merge 与审查闭环 |
| `executor` | 执行者 | Simple 根聊天，或 Task 中 planner 调用的 worktree 执行者 |
| `reviewer` | 审查者 | 代码审查和结果检查 |

桌面对话按 `compileMode = simple | task` 路由根角色：Simple 使用 executor，Task 使用 planner。Task 确认实施后仍由 planner 通过 coordinator 发起执行者、合并和 reviewer，不切换模式。

每个角色必须配置：

- `provider`
- `model`
- 可选的 `effort`

`effort` 使用字符串，校验 against 对应模型 `parameters` 中 `name = "effort"` 参数的候选值（即 `supported_efforts()`）。模型声明非空候选时，角色必须选择一个合法候选；模型没有声明 effort 参数时，角色必须省略 `effort`。候选、默认值和 wire 规则只来自模型目录，角色配置不保存第二份候选或默认值。

Studio 默认配置必须显式提供所需角色路由。provider 不保存 `default_model`；模型选择只由
route 决定。旧版本、缺失必需路由或无效引用会触发 Studio 配置重建，不进行兼容补齐。

## 10.4 TOML 示例

本地 TOML 使用 `snake_case`，不同于 API wire 格式。

```toml
schema_version = 15

[runtime]
permission_mode = "request-approval"
active_skills = ["rust", "git", "doc"]
active_mcp_servers = ["github", "filesystem"]

[instructions]
base_override = ""
developer = ""
user = ""
project_doc_max_bytes = 65536
project_doc_fallback_filenames = []

[mcp.servers.filesystem]
enabled = true
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/workspace"]
env = {}

[mcp.servers.github]
enabled = true
transport = "streamableHttp"
url = "https://example.com/mcp"
bearer_token_env_var = "GITHUB_MCP_TOKEN"
headers = {}

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

[models.routes.explorer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.planner]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.executor]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.reviewer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
# API token 由系统凭据库保存，TOML 中不出现明文 secret。
tool_wire_policy = "function_fallback"

preset = "deepseek"

[models.providers.deepseek.catalog]
source = "bundled"
catalog = "deepseek"

[models.providers.openai-work]
name = "OpenAI Work"
base_url = "https://api.openai.com/v1"
bearer_token_env = "OPENAI_API_KEY"

preset = "openai"

[models.providers.openai-work.catalog]
source = "bundled"
catalog = "openai"

[models.providers.openai-work.catalog.connection_overrides]
"gpt-5.5" = "http"
```

## 10.5 Provider 和 Model

`models.providers` 可保存多个 provider，相同 preset 可重复使用；唯一性只约束 map key
`ProviderId`。当前 StudioConfig schema 为 15，provider catalog snapshot schema 为 8。

每个 provider 实例持久化：

- 可选 `preset` 身份；完全自定义 provider 不保存 preset。
- 实例字段：`name`、`base_url`、`bearer_token_env`、`http_headers`、
  `tool_wire_policy` 与 `apply_patch_tool_type`。
- `catalog`：`Bundled { catalog, additional_models, connection_overrides }` 或
  `Explicit { models, connection_overrides }`。
- `capabilities`：`PresetDefaults` 或显式 `ProviderServiceCapabilities`。preset 实例默认继承
  canonical preset 的服务能力；custom 实例默认显式无能力。

Provider 不保存协议或连接方式。`AgentModelConfig::resolve()` 从当前 route 选择的 `ModelInfo` 和
模型目录 override 得到协议与最终连接方式，再与 provider endpoint、凭证、wire policy 和服务
能力组合成 runtime route。`ModelInfo.transport` 是必填字段；合法矩阵为 Responses+WS/HTTP、
Chat+HTTP。

服务能力不是模型能力的别名：provider 服务能力表示 endpoint 可以执行 Responses hosted tools、
hosted 或 standalone Web Search，`ModelCapabilities` 仍表示当前模型能否使用 native search 或
function tools。核心编排必须同时检查 endpoint 服务能力、模型能力与模型 request profile。官方
OpenAI endpoint 默认开启 Responses hosted tools；覆盖自定义 `base_url` 后默认关闭，只有显式配置
才能重新开启。产品 UI 从 catalog schema 7 的无密钥 descriptor 动态渲染能力选项，
不得识别 OpenAI、muxai 或其他具体 id。

OpenAI、MiMo API、MiMo Token Plan、DeepSeek、Zhipu 与 Zhipu Coding Plan 都只是 catalog
preset；厂商身份不进入模型执行分支。两个 MiMo preset 共享 `mimo` catalog。

当前不保留 Anthropic 占位；只有实现第二种协议族的 typed codec、能力模型与测试后，才可写入
配置或 catalog。

`pl-model` canonical catalog 与自定义/附加模型使用同一个 `ModelInfo`，可表达：

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
- `parameters`
- `transport`（协议、支持模式、默认模式）
- `capabilities`
- `request_profile`
- `truncation_policy`
- `base_instructions`

`used_fallback` 是运行时状态，不写入 TOML。

`capabilities` 是结构化能力矩阵。每个 input capability 显式声明 modality、允许来源和限制；
`request_profile.media` 同时声明当前 provider codec 可使用的有序表示与混合规则。两者不完整或
无法把持久快照重新编码时，该 modality 校验失败。未知模型默认 text-only；非视觉模型即使
provider wire API 接受图片字段，也会在任何附件 IO 和凭据读取前被拒绝。PDF 使用 file modality，
不保留旧的 pdf modality 别名。

价格字段为可选字段，用于本地 UI 估算费用。`currency` 只作为展示单位，系统不做汇率转换；三个 `*_price_per_mtok` 字段均表示每百万 token 单价。缺失任一参与计算的价格或缺失 `currency` 时，本次 token 仍进入上下文和用量统计，但费用标记为未计价。

会话总费用按货币分组展示，例如 `CNY 0.04 + USD 0.01`。系统不会把不同货币相加，也不会根据当前模型重新估算历史调用；每次 inference 都按当次实际使用模型的价格配置生成费用 delta。

Bundled DeepSeek 模型按中国官网人民币 API 价格配置：`deepseek-v4-flash` 为缓存命中输入 0.02 元、缓存未命中输入 1 元、输出 2 元；`deepseek-v4-pro` 为缓存命中输入 0.025 元、缓存未命中输入 3 元、输出 6 元。`input_price_per_mtok` 表示缓存未命中输入价，`cache_read_price_per_mtok` 表示缓存命中输入价。

Bundled OpenAI/GPT 模型参数以本地 Codex 仓库 `codex-rs/models-manager/models.json` 的修改为准，不按公开 API 文档臆造价格、最大输出或上下文窗口。当前 OpenAI 模板顺序为 `gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`。GPT-5.6 三个模型的上下文窗口和最大上下文窗口均为 `272000`，最大输出 token 未声明，且继承 OpenAI 的 Responses、视觉输入、工具调用、并行工具和 web search 能力。Pure 当前 `ModelInfo` 不持久化 Codex 的 `default_reasoning_level` 字段，因此 OpenAI 模型 effort 参数候选值的首项表示 Codex 默认 reasoning level，后续项表示其他支持档位；选中值通过 wire 写入 Responses 的 `reasoning.effort`。GPT-5.6 Sol 的默认 effort 为 `low`，Terra/Luna 的默认 effort 为 `medium`。Codex 的 `ultra` 是带自动任务委派语义的内部档位，Pure 没有对应语义，因此不加入候选值。

`request_profile.responses_max_tokens_field` 控制 Responses endpoint 如何序列化 `CompletionRequest::max_tokens`，可选值为 `omit`、`max_output_tokens`、`max_tokens` 和 `max_completion_tokens`。默认值为 `omit`，与 Codex 常规 Responses 请求保持一致；只有兼容代理明确要求输出 token 限制字段时才应在模型 profile 中显式设置。

Bundled Zhipu 模型 effort 候选值默认为 `enabled` / `none`，直接映射到 Chat 的 `thinking.type`；`glm-5.2` 例外，候选值为 `high` / `max` / `none`，wire 层 `high` / `max` 同时写入 `reasoning_effort` 并设置 `thinking.type = enabled`，`none` 设置 `thinking.type = disabled` 并移除 `reasoning_effort`。`glm-5.3` 候选值为 `high` / `low` / `max`，三档均写入 `reasoning_effort` 并设置 `thinking.type = enabled` 与 `clear_thinking = false`；GLM-5.3 始终思考，不提供 `none` 候选。`glm-5.3-flash` 复用 `glm-5.3` 的候选值与 wire，输入能力固定为 text/image；remote URL 图片首发优选 URL，local 与 durable snapshot 使用 Data URL。video/file 在缺少该模型精确契约时不得声明。

Bundled 模型只读，`additional_models` 只能添加新的 slug，冲突直接校验失败，不支持字段级覆盖。
完全自定义 provider 用 `Explicit` 保存完整模型列表。角色引用的 model 必须存在于
`ProviderConfig::effective_models()` 结果中。

## 10.6 提示词配置

`[instructions]` 保存 Codex 风格提示词分层的用户可配置部分。缺失整个表或字段时使用默认值。

- `base_override`：完整替换当前模型的 `base_instructions`。仅用于需要完全接管系统提示词的高级场景；普通长期偏好不应写在这里。
- `developer`：追加到 developer 层，适合本地运行约束、协作偏好和稳定行为规则。
- `user`：追加到 user context 层，适合用户背景、项目上下文和非强制偏好。
- `project_doc_max_bytes`：AGENTS 项目文档总读取上限，默认 `65536`，设为 `0` 表示禁用项目文档注入。
- `project_doc_fallback_filenames`：除 `AGENTS.override.md`、`AGENTS.md`、`Agents.md` 外额外尝试的项目文档文件名。

运行时会在 Thread 首轮固定 base/system、developer blocks、user context blocks 与 AGENTS
source paths。已有 Thread 后续不会因为配置或项目文档变化自动改变提示词；新 Thread 才使用
新配置。

## 10.7 运行态声明

`[runtime]` 保存本地 Studio 运行态展示所需的可选声明。字段：

- `permission_mode`
- `active_skills`
- `active_mcp_servers`

`permission_mode` 是 Thread Turn 的默认权限模式，缺失时按 `request-approval` 处理。可选值：

- `request-approval`
- `auto-review`
- `full-access`

Pure v1 的权限模式是策略层，不是 OS 沙箱。`request-approval` 和 `auto-review` 都直接允许 workspace 内读写；工具请求 workspace 外路径或 workspace 外 cwd 时分别走用户审批或 reviewer 审批。`full-access` 会放宽本地文件 backend 和 `exec.cwd` 的 workspace 边界并直接放行；宿主注入的容器或远程 backend 仍可保持更严格边界。

`active_skills` 只声明启动时预选项，不作为真实 skills 发现来源。`active_mcp_servers` 只声明启动时预选项，MCP server 的用户启用意图来源为 `[mcp.servers.<id>].enabled`；真实可用性由进程内 MCP registry 探测，不写回配置。

真实 skills 能力由 `[skills]` 配置和项目目录驱动。`active_skills` 不作为启停来源，不影响模型可见的 skills 列表，也不作为当前 Thread 状态栏 Skills 的来源。Studio 当前 Thread 的 `activeSkills` 由后端持久化的 Thread 级 skill activation 记录派生；只有成功执行过 `skill_view`、且后端写入 `SkillActivated` 的 skill 才计入。

## 10.8 MCP 配置

MCP server 配置保存在 `[mcp.servers.<server_id>]` 表。`server_id` 必须非空，且只能包含 ASCII 字母、数字、`_` 和 `-`，因为它会参与模型可见工具名。Pure 还会在运行时合成一组内置 MCP server；内置 server 不写入 `mcp.servers`，但会出现在 Studio MCP 设置页和状态栏中。用户配置不得占用内置保留 id。

内置 MCP 的 UI toggle 状态保存在 `[mcp.builtin_servers.<server_id>]`；该表不描述 transport 或 endpoint，也不允许新增 server。检测到 Zhipu Coding Plan 凭证时，相关内置 server 的缺失状态按默认启用补齐，但不会覆盖用户显式禁用。

每个 MCP server 必须配置：

- `enabled`：是否启用该 server，默认 `true`。
- `transport`：`stdio` 或 `streamableHttp`。

`stdio` server 必须配置：

- `command`
- `args`
- `env`
- `cwd`（可选）

`streamableHttp` server 必须配置：

- `url`
- `bearer_token_env_var`（可选）
- `headers`

Pure 启动后由 `McpRuntime<McpConnector>` owner 显式 reconcile 启用且凭据完整的 MCP server。`McpConnector`
只负责通过 rmcp 建立连接并返回 `ConnectedMcp`；PL 统一维护配置 fingerprint、增量 reconcile、
工具命名、冲突检查、健康状态和 generation 原子替换。Studio 只负责组合配置。Simple executor
可以消费 available tools；Task planner、explorer、reviewer 只暴露 effect 策略明确允许的动态工具，
未知 effect 默认拒绝。

内置 Zhipu Coding Plan MCP server 固定为：

- `zhipu_search`：Streamable HTTP，`https://open.bigmodel.cn/api/mcp/web_search_prime/mcp`
- `zhipu_reader`：Streamable HTTP，`https://open.bigmodel.cn/api/mcp/web_reader/mcp`
- `zhipu_zread`：Streamable HTTP，`https://open.bigmodel.cn/api/mcp/zread/mcp`
- `zhipu_vision`：stdio，`npx -y @z_ai/mcp-server`；Windows 运行时从标准 npm 安装布局解析
  `node.exe + npx-cli.js`，并让 npm 创建包级 launcher 时启用 `windowsHide`；配置和状态中不写入
  `.cmd`

这些内置 server 优先复用 Zhipu Coding Plan provider 的 `bearer_token` 作为 Coding Plan key；若不存在 Coding Plan provider，则兼容回退到普通 Zhipu provider 的 `bearer_token`。缺少可用 token 时内置 server 的配置状态为缺少凭据且不会进入健康探测；检测到 token 时，未显式配置状态的内置 server 默认启用并进入后台探测，已被用户禁用的内置 server 保持禁用。HTTP 内置 server 运行时直接发送 bearer token；Vision server 运行时注入 `Z_AI_API_KEY=<token>` 和 `Z_AI_MODE=ZHIPU`。

模型可见的 MCP tool 名称以以下形式为基础：

```text
mcp__{server_id}__{tool_name}
```

PL 对 server id 和远端 tool name 统一规范化，合成名称最长 64 个字符；发生冲突时追加稳定 hash
后缀。命名规则属于公共 MCP runtime，产品 Host 和 UI 不重复实现。每个 turn 从 runtime 获取固定
generation 的 `McpTurnLease` 后再安装工具；新 generation 完全 ready 前不对新 turn 可见，旧
generation 在最后一个 lease 释放后异步关闭。相同 effective fingerprint 的 reconcile 完全 no-op；
手动重连走独立的单 server/All reset，不使用 force reconcile 模拟。reset 候选失败时保留当前
live generation，shutdown 是不可恢复终止态。完整合同见 `20-studio-state-runtime.md`。

## 10.9 Skills 配置

`[skills]` 控制本地 skills 系统：

- `enabled`：是否启用 skills 目录、prompt 注入、工具注册和用户 `/name` 手势，默认 `true`；关闭时
  Studio 仍可保留已发布 catalog 供设置页只读展示。
- `auto_learn`：是否在 Studio 主 turn 结束后启动后台 reviewer 自动沉淀项目 skill，默认 `true`。
- `project_dir`：项目级 skills 目录，相对 `workspace_root` 解析，默认 `skills`。
- `user_dir`：用户级只读 skills 目录，默认 `~/.pure/skills`。
- `system.enabled`：是否启用内置系统 skills，默认 `true`。
- `external_dirs`：额外只读 skills 目录列表，默认空。
- `disabled`：禁用的 skill 名称列表，默认空。
- `auto_learn_min_tool_calls`：触发自学习 review 的最少工具调用数，默认 `5`。

除可配置的 `user_dir` 外，运行时还固定发现 Agents 兼容用户目录：Linux
`$HOME/.agents/skills`、Windows `%USERPROFILE%\.agents\skills`。该兼容目录不新增配置字段，
与 `user_dir` 一样是只读 `User` 来源。加载优先级固定为：项目 skills > `user_dir` >
`.agents/skills` > 系统 skills > external dirs；同名 skill 只暴露最高优先级来源。自学习和
`skill_manage` 写入只作用于项目 skills 目录，不会修改用户目录、系统目录或外部目录。

Skill frontmatter 的 `disable-model-invocation` 默认 `false`，`user-invocable` 默认 `true`。无效类型
使该 skill 失败关闭并产生发现 warning；这两个字段不进入 `StudioConfig` schema。

Studio 系统 skills 来自编译进 `pl-studio-runtime` 的预置资源，并在每次 Studio Runtime 启动时
全量重建到 `<studio_home>/studio/skills/.system/`。系统目录固定属于 Studio 数据，不从
`skills.user_dir` 推导；`system.enabled` 只控制该来源是否参与发现，不控制启动刷新。该目录由
Pure 管理，用户需要覆盖系统 skill 时应在项目 `skills/` 目录创建同名 skill。

系统内置 `studio-config` skill 是面向 agent 的 Pure Studio 配置指南，覆盖配置文件位置、当前
schema、常用配置段、凭据处理和安全编辑行为。其 canonical 源文件固定为
`code/pl-studio-runtime/assets/skills/studio-config/SKILL.md`。任何改变配置路径、schema 版本、配置
段或字段及其默认值、凭据解析优先级、加载/保存/重载语义或最小有效配置的变更，都必须在同一
变更中同步更新该 skill，并复核 skill、本文档与运行时行为一致。

## 10.10 配置草稿

通用 provider/model 值对象、preset/catalog 和 endpoint 解析属于 `pl-model`，`pl-core` 只维护动态角色路由并作为宿主 runtime facade 重新导出必要类型；`StudioConfig`、schema、默认角色和配置文件 IO
属于 `pl-studio-runtime`。`pure-studio` 设置页先加载 canonical provider catalog，再构造产品草稿：

- 默认选中 Studio 产品默认 preset，也可选择 catalog 返回的任意 preset 或 Custom provider。
- 至少配置一个 provider。
- 可继续添加多个 provider 实例，允许同类 provider 重复，例如 `deepseek`、`deepseek-2`、`openai`、`openai-work`。
- 每个 provider key 必须唯一。
- preset、endpoint、凭证提示、协议、允许连接模式、suggested model 和 bundled catalog 全部来自
  `ProviderCatalogSnapshot`；Flutter 不保存生产目录副本。
- preset provider 引用只读 bundled catalog，并可追加不冲突的模型；Custom provider 使用 explicit models。
- Studio 草稿选择一个默认 provider；四个模型角色初始化为创建时选择的 suggested/default model
  和该模型声明的默认 effort。该选择只投影为四条 route，不写入 provider runtime。

设置项写入前必须完成本地校验：

- provider key 非空且唯一。
- preset 引用必须存在；Custom provider 必须显式选择 wire protocol。
- `Responses + WebSocket/Http` 合法，`ChatCompletions + Http` 合法，`ChatCompletions + WebSocket` 在发起网络请求前拒绝。
- API key 非空。
- 每个角色 route 的 model 必须存在于对应 provider 的 effective models。
- 同一 provider 下模型 slug 不重复。
- 角色引用的默认模型必须声明 `name = "effort"` 参数且至少一个候选值，用于生成角色 `effort`。

## 10.11 pure-studio 设置页

`pure-studio` 设置页消费 Bridge 返回的 catalog/config projection，保存时由
`pl-studio-runtime` 组合 `StudioConfig` 并统一校验，覆盖：

- catalog 中的全部 preset 与 Custom Responses/Chat provider。
- API key、base URL、provider key 和显示名。
- Studio 路由编辑投影中的 provider 默认模型和自定义模型；runtime provider 不保存默认模型。
- 四个模型角色到 provider/model/effort 的路由。
- Security 标签页选择权限模式：请求批准、替我审批、完全访问。选择后即时写入 `[runtime].permission_mode`。
- Instructions 标签页编辑 `[instructions]` 的 base override、developer、user 和项目文档预算；保存前由 `pl-core` 校验并即时写入配置。
- MCP 标签页管理用户 `[mcp.servers]`，包括 server id、启用状态、stdio/Streamable HTTP 传输方式、命令参数、环境变量、HTTP URL 和 token 环境变量；同时展示不可删除的内置 Zhipu Coding Plan MCP server。
- Provider 列表卡片展示供应商身份、默认模型、模型数量和只读额度状态，不把 base URL 作为主卡片信息。base URL 仍保留在编辑页和 TOML 配置中。
- Provider 额度查询由后端执行，前端只消费脱敏 DTO。DeepSeek provider 查询账户余额；Zhipu Coding Plan provider 查询 5 小时、7 天和 MCP 工具额度；普通 Zhipu provider 不查询 Coding Plan 额度。
- 供应商设置页打开时只展示 last-known state；只有手动“检查额度”命令访问网络，不做后台定时
  轮询。缺少 API key、网络失败和 provider 业务失败必须作为 failed/stale 卡片状态展示，不能阻塞
  配置编辑。

每次设置项写入前必须执行 `StudioConfig::validate()`（内部调用
`AgentModelConfig::validate()`）；失败时只在 UI 中展示错误，不写入磁盘。

MCP 标签页使用结构化表单，不展示 raw TOML。新增和编辑用户 server 使用本地草稿；保存成功后
即时写入 `~/.pure/config.toml`，effective fingerprint 变化时向 MCP owner 提交 incremental
reconcile。删除 server 和启用切换同样即时写入。页面“刷新”只读取 owner snapshot；单 server
“重新连接”调用 reset，“全部重置”经确认调用 All reset。内置 Zhipu Coding Plan MCP server
不可删除，不允许编辑 server id、transport、endpoint 或运行时注入字段；界面同时显示 desired
配置和 applied runtime。设置页状态栏展示的 MCP 数量和列表来自当前 owner snapshot。

设置页 UI 按 Flutter feature/page 模块拆分，顶层 `MaterialApp.router` 负责页面路由，Riverpod controller 负责共享状态，具体页面放在 `lib/src/features/settings`，桥接调用封装在 repository 层。Provider 标签页并行消费 canonical catalog 与服务端解析的 provider projection，不引入前端配置存储或目录 fallback。

Provider 标签页必须提供结构化编辑能力：

- Provider 列表页只提供一个“添加供应商”入口；点击后进入 Provider 编辑页。
- 新增 Provider 按 preset suggested model 的模型默认连接方式初始化并自动生成唯一 provider key；编辑页的 preset 下拉完全由 catalog 生成，也提供 Custom provider。创建草稿与切换 preset 必须共用同一个草稿工厂，以 catalog preset 一次性替换完整的 immutable provider 草稿；不得在事件处理器中逐字段拼接供应商默认值。preset 身份变化时必须重建整组表单状态，使 endpoint、凭据提示、默认模型、模型目录、能力及后续新增的 preset 字段从同一份草稿快照同步刷新。
- 编辑 provider key、显示名、base URL 和 API key。
- 以 provider 卡片作为主要信息载体，展示 provider key、preset、状态、当前路由模型、模型数量和额度状态等摘要信息；base URL 只在编辑页展示。
- Provider 列表页不直接编辑字段；点击卡片编辑按钮进入 provider 编辑页。
- Provider 编辑页使用本地草稿，提供保存和取消按钮；保存成功后即时写入配置并返回列表，取消或返回列表不修改当前配置。
- Provider 卡片必须提供删除按钮；删除和默认 provider 选择都即时写入配置，列表页不使用独立右侧详情面板。
- Provider 标签页不展示 raw TOML 配置编辑器，确保列表和编辑页占据主要工作区。
- 展示 provider effective models；bundled 模型与附加模型的顺序由服务端统一解析。
- 允许追加用户自定义模型，冲突 slug 直接拒绝，Flutter 不自行实现合并规则。
- 模型列表应展示关键参数，例如上下文窗口、最大输出 token、自动压缩阈值、temperature、effort 候选值（`supported_efforts()`）、capabilities、输入模态和截断策略。
- Provider 编辑页不提供 provider 级协议或连接方式控件。每个模型行展示协议、支持模式和当前模式；只有支持多个模式的模型提供 HTTP/WS 选择，并保存为该模型的 connection override。自定义模型编辑器必须显式选择协议、支持模式与默认模式，Chat + WS 在保存前拒绝。Responses 的 HTTP 仍调用 `/responses` 并消费 SSE，Chat Completions HTTP 调用 `/chat/completions`。Web 和 Flutter 的协议/模式/默认值必须来自 model descriptor，不得按 preset ID 分支。
- Zhipu 请求固定使用流式 `chat/completions`；effort 由模型 `parameters` 声明驱动（见 07-model.md 7.8）。默认模型 effort 候选值为 `enabled` / `none`，直接映射到 `thinking.type`，不发送 wire-level `reasoning_effort`。`glm-5.2` 候选值为 `high` / `max` / `none`，其中 `high` / `max` 会作为 `reasoning_effort` 透传给 API 并设置 `thinking.type = enabled` 与 `clear_thinking = false`，`none` 设置 `thinking.type = disabled` 并移除 `reasoning_effort`。`glm-5.3` 候选值为 `high` / `low` / `max`，三档均作为 `reasoning_effort` 透传并设置 `thinking.type = enabled` 与 `clear_thinking = false`；GLM-5.3 始终思考，不提供禁用思考候选。历史回放仍通过 assistant message 的 `reasoning_content` 字段保留。
- 写入前由 `pl-studio-runtime` 构造 `StudioConfig` 并执行完整校验；校验失败时只在 UI 中展示错误，不写入磁盘。更新 API key 时，空输入表示保留现有 secret；provider key 重命名必须携带 `originalId`，以便服务端保留 secret、headers、catalog metadata 和模型能力。

Roles 标签页必须展示固定四个角色：探索者、计划者、执行者、审查者。每个角色将模型与“思考强度”作为两个独立下拉控件展示；模型选项使用 `Provider / Model · Protocol · Connection`，思考强度候选值来自当前模型声明的 `supported_efforts()`。模型改变时，有候选的模型切换为其声明的默认 effort，没有显式默认时使用首个候选；无候选模型保存空选择并禁用强度控件。仅改变思考强度时必须保持当前 provider 和 model 不变。模型与思考强度变更都即时保存，但 Flutter 不进行持久 optimistic 更新，也不保存第二份 selection；成功后以 bridge 返回的 typed canonical settings snapshot 更新 store，失败时保持原 canonical 状态。`pl-studio-runtime` 统一校验后写入 `~/.pure/config.toml`。

桌面窗口必须支持自由缩放。`pure-studio` 只声明首选窗口尺寸，不把 UI 绑定到固定宽高；设置页内容跟随窗口尺寸自适应。Provider 标签页在常规桌面宽度使用单栏 provider 卡片列表，卡片内部承载摘要、操作和展开编辑内容；在窄窗口下保持单栏滚动并压缩卡片元信息，避免表格和编辑区域被裁剪。聊天状态栏在窄窗口下保留左侧高频控制，并把右侧只读状态按断点收入更多菜单。

为了支持设计验证，`pure-studio` 应通过 widget test 和 Windows 运行态截图验证设置页 fixture 状态。Provider 设置页的本地验证入口固定为：

```powershell
cargo xtask verify-gui
cargo xtask run-gui
```

widget test 只用于布局和状态回归，最终应用行为仍以 Flutter Windows 运行结果为准。

聊天界面的 agent 目录属于 root Thread 的轻量产品状态，信息来自 Thread directory 的 product
stream。标题区唯一的 `n agents` 菜单只展示 owner、父子关系、角色和状态，不携带 timeline、
Todo、interaction 或 context。选择条目后再订阅对应 Thread；底部状态栏不维护第二套 agent
活动面板。

Studio 交互状态统一保存在 SQLite `interactions` 表。工具审批、`request_user_input` 和 Plan 实施确认都通过该表与 `InteractionChanged` 事件恢复；旧 `tool_approvals` 不再作为读写路径或 UI pending 状态来源。破坏性 schema 版本不迁移旧 pending 审批、询问或计划确认。

聊天界面使用双栏布局：左侧项目/大会话栏和主聊天区，不再展示右侧工具历史面板。主聊天区底部状态栏左侧展示当前 agent 身份、`Auto / Plan` 模式、当前模型和推理强度，右侧展示上下文使用量、按货币分组的费用估算与 Skill/MCP/LSP 数量；权限模式保留在 Composer。Skill/MCP/LSP 默认只显示数量，悬浮、点击或键盘聚焦时展示当前 agent 的完整列表。状态栏不得显示 agent 数量或子代理列表；大会话下的 agent 数量、状态和切换只通过标题区唯一的 `n agents` 菜单表达。由于左侧栏会占用窗口宽度，状态栏响应式按聊天 footer 自身宽度折叠低频读数，并保证详情弹层不被状态栏滚动容器或窗口边界裁剪。

## 10.12 凭据策略

Provider 的 API token 保存到操作系统凭据库，service 固定为 `pure-studio`，account 为 `provider:{provider_id}`；`~/.pure/config.toml` 不保存 token、凭据引用或可逆密文。Provider 仍可保存 `bearer_token_env` 环境变量名。配置加载后，Studio 在 Rust 内存中注入系统凭据；运行时通过 `resolved_bearer_token()` 解析，系统凭据优先，其次读取非空环境变量值，空白值和缺失环境变量都视为无凭据。

设置页的 Preserve/Replace/Clear 语义保持不变：Preserve 不改系统凭据，Replace 在配置提交前写入并回读，Clear 删除凭据。凭据操作和 TOML 原子替换作为一个 fail-closed 提交流程；凭据阶段失败时不得覆盖配置文件。启动恢复不得按旧 provider id 读取、迁移或删除凭据；原配置只以逐字备份保留，默认配置仅按当前默认 provider id 注入已有系统凭据。运行期显式重载仍直接返回配置错误并保留原文件。

MCP stdio server 的 `env` 会按配置原样传给子进程，可能包含明文凭据。Streamable HTTP 的 `bearer_token_env_var` 只保存环境变量名，运行时从 Pure 进程环境读取对应 token 并构造 Authorization header。

## 10.13 Web 搜索配置与凭据门控

Studio 配置的顶层 `web_search` 包含：`mode` 为 `disabled | cached | indexed | live`，默认 `cached`；`context_size`、`allowed_domains` 和近似位置 `country/region/city/timezone` 均可省略。不兼容 schema 按 10.1 直接报错，不迁移旧 web search 状态，也不虚构位置、域名或 context size。

配置值与生效值必须分离：没有有凭据的 OpenAI preset 时保留 configured mode，但 effective mode 为 `disabled`。此状态下工具规划不得注册独立搜索或 hosted 搜索，且运行时不得创建 `/alpha/search` 客户端。可用账户优先当前 turn 的 OpenAI provider；否则按 provider id 稳定排序，并按 `explorer -> planner -> executor -> reviewer` 选择首个指向该 provider 的有效模型，最后才回退到目录首个模型。

`cached` 映射为禁止外部实时访问；`indexed` 映射为显式 indexed 访问；`live` 允许实时外网；`disabled` 完全移除搜索能力。设置保存返回 canonical config 和 availability，前端不得仅修改本地 draft。

## 10.14 LSP 自定义 server 配置

自定义语言服务器声明在 `[lsp.servers.<server_id>]` 表，schema 15 下为可选段：没有该段的旧
config 按默认（空表）加载，不 bump schema 版本。每个条目必须配置 `command` 与非空
`language_ids`，可选 `args`、`detection`（workspace 检测文件名/glob，缺省总是匹配）、
`extensions`（文件扩展名，缺省为空）、`display_name`（缺省使用 server id）和 `operations`
（`lsp_query` 操作子集，缺省支持全部）。示例：

```toml
[lsp.servers.purelang]
command = "purelang-lsp"
args = ["--stdio"]
language_ids = ["purelang"]
detection = ["pure.toml"]
extensions = [".purelang"]
```

该段与 `pl-lsp` 内置 catalog 合并；重复 server id 或 language id 与内置/其他自定义 server
冲突时，`StudioConfig::validate` 以 typed 错误 fail-loud，并按 10.1 的不兼容配置合同用初始
配置替换。自定义 server 使用通用命令 driver（`<command> --version` 探测，无 repair 语义），
运行行为与路由合同见 `14-lsp-runtime.md`。Studio 项目激活时把该段应用进 LSP registry
catalog，并纳入激活 fingerprint。
