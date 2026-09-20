# 20 - Studio 持久化配置

本文是配置文件 schema、provider/model 结构、提示词分层、MCP/Skills/LSP 配置与凭据策略的
唯一权威源；权限语义见 [04](./04-security.md)，模型层机制见 [06](./06-model.md)，设置页
UI 见 [19](./19-studio-ui.md)，Agent Profile 体系见 [12](./12-collaboration.md)。

## 20.1 配置位置与产品身份

anywork 使用独立产品身份，默认仅访问 `~/.anywork`，凭据服务名为 `anywork`，产品环境变量
前缀为 `ANYWORK_`。产品与用户数据的版本演进以 anywork 为起点；显式指定的数据根目录
仍遵循既有参数优先级。

配置文件固定为 `~/.anywork/config.toml`（Windows 下 `%USERPROFILE%\.anywork\config.toml`）。
桌面产品目录及动态状态保存在 `~/.anywork/studio/studio.sqlite`，Thread 通用事实由 core
保存到 `studio/sessions/<thread-id>.sqlite` 的不可变 journal 中（分层合同见
[17](./17-studio-storage.md)）。工作空间声明在 `workspaces/<project-id>.toml`。用户
Agent Profile 单独保存到 `~/.anywork/agents/*.toml`。schema 版本以代码常量为准。

配置运行时在 Studio 启动时读取配置；此后普通对话和设置查询只读内存 canonical snapshot；
配置文件不存在时设置页展示内存中的默认配置；外部文件变化只有显式重载命令才能应用。
普通设置项在用户修改后即时写入配置；独立新增/编辑页面保留本地草稿，必须点击页面内保存
按钮才写入，取消则丢弃草稿。

配置版本演进必须满足以下迁移契约；支持路径与剩余边界见
[17.6](./17-studio-storage.md#176-已实现迁移与剩余边界)，不能据此假定支持任意旧版本。

- 启动时在产品发布前识别配置版本，使用明确的版本转换路径将 anywork 历史配置与用户
  Agent Profile 转为当前结构，支持跨版本升级；保留用户选择、provider 身份与凭据关联。
- 转换前备份原始文件且不覆盖已有备份，转换后执行完整校验，通过原子替换或可恢复步骤
  提交。涉及多个配置
  文件、数据库关联或凭据时由 Studio 协调，不能发布新旧结构混合的 canonical snapshot。
- 新增字段只能使用该版本迁移明确规定的默认值；旧字段在迁移边界转换，不能用重建整份
  默认配置、猜测模型路由或运行时兼容补齐代替迁移。正常读写只使用当前 schema。
- 未来或未知版本、无法解析、无效引用、缺失迁移路径、凭据或 IO 失败时，保留原文件与
  关联凭据并报告原因，不自动恢复默认值。只有配置文件不存在时才采用内存默认配置。
- 中断后可安全重试或恢复一致状态；运行期显式重载只接受当前有效结构，失败保留已有
  canonical snapshot，不在查询或重载中隐式迁移。迁移结果与失败通过脱敏诊断报告。

迁移验证应覆盖版本跳跃、字段重命名、路由与 Profile 关联、凭据标识变化、未知版本、
损坏配置、备份或提交失败及重启恢复，证明用户选择与凭据可用性得到保留。

所有 Settings command 必须携带 `expectedSettingsRevision`，成功只返回完整设置状态快照，
由 Flutter 原子替换 Settings 领域；不得返回聚合状态、raw JSON 或 raw map。CAS 或校验失败
时保留当前 canonical 状态，不覆盖新配置。

## 20.2 配置职责

工作空间声明以一个 Project 一个 TOML 对象保存稳定 ID、名称、路径、本地/SSH 关联与
声明版本；文件名与 ID 必须一致且通过路径安全校验。最近打开时间、关闭状态、会话目录
和 worktree ownership 不属于声明，不写入这些文件。产品库中的声明检索列可重建，声明
文件是唯一持久配置源，运行期由内存 canonical snapshot 发布。保存采用校验和原子替换，
外部编辑通过显式重载应用，失败保持已发布状态并保留原文件。已创建会话的 workspace_path
是冻结事实，不因声明编辑而变化；lease 继续由独立资源 owner 管理。数据库到 TOML 的
首次转换纳入 [17](./17-studio-storage.md) 的布局迁移，不清空项目或重建 ID。

跨文件的声明提交使用持久 intent：先记录目标 ID、旧/新内容指纹与阶段，再原子替换声明
文件，最后更新布局标记；标记绑定每个声明文件的内容指纹，而不只是 ID 集合。中断后启动
只按 intent 重试完成或明确报告失败现场，未登记 intent 的额外声明文件一律拒绝而不是
自动采纳；同一 ID 的声明内容变化必须被标记校验发现。只有显式重载接受外部编辑，成功后
刷新标记指纹与 intent 基线。声明目录、控制文件与每个已存在祖先都拒绝链接或 reparse，
未知字段与未知材料明确失败，不静默跳过或被下次保存丢弃。

pl-model 拥有产品无关的模型配置值对象：角色路由配置（provider/model/effort 校验与解析）、
provider 配置与模型路由配置，负责把路由解析为运行时 endpoint 和唯一选中的不可变模型信息。
pl-studio-runtime 拥有：Studio 配置 schema 与启动期版本迁移、配置文件路径、
TOML 解析、原子保存和默认值、instructions/skills/MCP/runtime/disabled_system_agents 与
UI 配置、Agent Profile 文件的逐文件解析与原子保存，以及 Thread 首轮固定 instruction
snapshot 的生成。pl-model 只消费已经解析好的 provider 和模型信息，不负责文件 IO 或路径
定位。

`[ui]` 仅保留 `follow_active_turn`（默认 true）和 `compact_timeline`（默认 false）；主题不
属于持久化设置。未知 UI 字段按既有规则忽略，正常保存后不再输出；启动不因未知字段重写
配置、触发恢复或提升 schema。

## 20.3 根路由与系统 Profile

配置不使用 `active_provider`。所有模式的 root Agent 统一使用 `planner` 路由，不再根据
Simple/Task 切换根角色。Studio 注册五个系统 Agent Profile：

| 配置 key | 中文角色 | 用途 |
| --- | --- | --- |
| `explorer` | 探索者 | 代码、文档和上下文探索 |
| `planner` | 计划者 | 形成计划和约束；普通对话默认路由 |
| `executor` | 执行者 | 实施修改和验证 |
| `worktree_executor` | Worktree 执行者 | 在独立 Git worktree 实施修改和验证 |
| `reviewer` | 审查者 | 代码审查和结果检查 |

系统 Profile 由内置结构体启动注册，不生成 TOML；全部字段不可编辑、不可删除，只能通过主
配置 `disabled_system_agents` 禁用。系统 route 的 provider/model/effort 在 Agents 页统一
配置；禁用 planner 不影响 root 使用 planner 路由。用户 Profile 的文件名 stem 是 Agent ID。
`list_agent_profiles` 只返回启用且路由可解析的 Profile；`spawn_agent` 创建 child 时冻结
系统指令、provider、model 与 effort，此后文件变化不改变既有 child；设置页另读完整
catalog，被禁用的用户 Profile 仍可编辑并重新启用。

每个角色必须配置 `provider`、`model` 和可选 `effort`。`effort` 使用字符串，校验对象是
对应模型 parameters 中 `name = "effort"` 参数的候选值：模型声明非空候选时，角色必须选择
一个合法候选；模型没有声明该参数时，角色必须省略 `effort`。候选、默认值和 wire 规则只
来自模型目录（见 [06](./06-model.md)），角色配置不保存第二份候选或默认值。provider 不
保存 `default_model`，模型选择只由路由决定；历史结构按 20.1 转换，缺失必需路由或无效
引用明确报错，不能静默重置用户选择。

## 20.4 TOML 示例

本地 TOML 使用 `snake_case`，不同于 API wire 格式。精简示例：

```toml
schema_version = 18

disabled_system_agents = []

[runtime]
permission_mode = "request-approval"
active_skills = ["rust", "git", "doc"]
active_mcp_servers = ["github", "filesystem"]

[instructions]
base_override = ""
developer = ""
user = ""
project_doc_max_bytes = 65536

[mcp.servers.filesystem]
enabled = true
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/workspace"]
env = {}

[skills]
enabled = true
auto_learn = true
project_dir = ".agents/skills"
user_dir = "~/.anywork/skills"

[models.routes.planner]
provider = "deepseek"
model = "deepseek-flash"
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
```

## 20.5 Provider 与模型目录

`models.providers` 可保存多个 provider，相同 preset 可重复使用；唯一性只约束 map key。
每个 provider 实例持久化：可选 `preset` 身份（完全自定义 provider 不保存 preset）、实例
字段（`name`、`base_url`、`bearer_token_env`、`http_headers`、`tool_wire_policy` 与
`apply_patch_tool_type`）、catalog（`Bundled { catalog, additional_models,
connection_overrides }` 或 `Explicit { models, connection_overrides }`）与 capabilities
（`PresetDefaults` 或显式服务能力：preset 实例默认继承 canonical preset 的服务能力，
custom 实例默认显式无能力）。

Provider 不保存协议或连接方式。路由解析从当前 route 选择的模型信息和模型目录 override
得到协议与最终连接方式，再与 provider endpoint、凭证、wire policy 和服务能力组合成
runtime 路由；模型 transport 是必填字段，合法矩阵为 Responses+WS/HTTP、Chat+HTTP。

服务能力不是模型能力的别名：provider 服务能力表示 endpoint 可以执行 Responses hosted
tools、hosted 或 standalone Web Search（hosted search 还携带 OpenAIResponses 或
DeepSeekResponses 方言），模型能力仍表示当前模型能否使用 native search 或 function
tools；核心编排必须同时检查 endpoint 服务能力、模型能力与模型 request profile。官方
OpenAI endpoint 默认开启 Responses hosted tools；覆盖自定义 `base_url` 后默认关闭，只有
显式配置才能重新开启。产品 UI 从 catalog 的无密钥 descriptor 渲染预设和模型选项，不识别
具体厂商 id。OpenAI、MiMo API、MiMo Token Plan、DeepSeek、Zhipu 与 Zhipu Coding Plan 都
只是 catalog preset；显式 adapter 身份装配具体供应商客户端，两个 MiMo preset 共享同一
catalog，而 Zhipu（通用 Chat API）与 Zhipu Coding Plan（官方 OpenAI Response 协议端点）
各引用与端点形态匹配的目录。当前不保留 Anthropic 占位：只有实现第二种协议族的
typed codec、能力模型与测试后，才可写入配置或 catalog。

canonical catalog 与自定义/附加模型使用同一个模型信息结构：slug、display_name、
description、context_window、max_context_window、auto_compact_token_limit、
default_temperature、max_output_tokens、pricing、parameters、binding（transport 与协议
适用 request 配置）、capabilities、truncation_policy 与 base_instructions。capabilities
是结构化能力矩阵（见 [06](./06-model.md)）：每个 input capability 显式声明 modality、
允许来源和限制；两者不完整或无法把持久快照重新编码时该 modality 校验失败；未知模型
默认 text-only；非视觉模型即使 provider wire API 接受图片字段，也会在任何附件 IO 和
凭据读取前被拒绝；PDF 使用 file modality。

ModelPricing 明确区分未知价格与包含费率的定义：货币不做汇率转换；输入、缓存读、缓存写
与输出按互斥类别计费，reasoning 已包含在输出内；用量或费率缺失时标记未计价；关闭计价
与实际零费用分别表示；每次请求冻结价格定义、计价开关和发送时间，最终账单按最终用量
选择长度档位，跨时段请求不拆分 token（本地估算口径，见 [06](./06-model.md)）。内置价格
来源、核对日期及完整档位保存在后端目录，设置页直接展示后端提供的行；文档不保存价格
快照。

Bundled 模型只读，`additional_models` 只能添加新的 slug，冲突直接校验失败，不支持字段级
覆盖；完全自定义 provider 用 `Explicit` 保存完整模型列表；角色引用的 model 必须存在于
该 provider 解析后的有效模型集合中（bundled catalog + 追加/显式模型）。

## 20.6 提示词配置

`[instructions]` 保存 Codex 风格提示词分层的用户可配置部分，缺失整个表或字段时使用默认
值：

- `base_override`：完整替换当前模型的 `base_instructions`。仅用于需要完全接管系统提示词的
  高级场景；普通长期偏好不应写在这里。
- `developer`：追加到 developer 层，适合本地运行约束、协作偏好和稳定行为规则。
- `user`：追加到 user context 层，适合用户背景、项目上下文和非强制偏好。
- `project_doc_max_bytes`：AGENTS 项目文档总读取上限，默认 `65536`，设为 `0` 表示禁用项目
  文档注入。
- `project_doc_fallback_filenames`：除 `AGENTS.override.md`、`AGENTS.md`、`Agents.md` 外
  额外尝试的项目文档文件名。

运行时会在 Thread 首轮固定 base/system、developer blocks、user context blocks 与 AGENTS
source paths；已有 Thread 后续不会因配置或项目文档变化自动改变提示词，新 Thread 才使用
新配置。

## 20.7 运行态声明

`[runtime]` 保存本地 Studio 运行态展示所需的可选声明：`permission_mode`、
`active_skills` 与 `active_mcp_servers`。`permission_mode` 是 Thread Turn 的默认权限模式，
缺失时按 `request-approval` 处理；三值语义的唯一权威源见 [04](./04-security.md)。
`active_skills` 与 `active_mcp_servers` 只声明启动时预选项，不作为真实发现来源：MCP
server 的用户启用意图来源为 `[mcp.servers.<id>].enabled`，真实可用性由进程内 registry
探测，不写回配置；真实 skills 能力由 `[skills]` 配置和项目目录驱动，当前 Thread 的
activeSkills 由后端持久化的 Thread 级激活记录派生（见 [10](./10-skills.md)）。

## 20.8 MCP 配置

MCP server 配置保存在 `[mcp.servers.<server_id>]` 表。`server_id` 必须非空，且只能包含
ASCII 字母、数字、`_` 和 `-`，因为它参与模型可见工具名；用户配置不得占用内置保留 id。
Pure 运行时还会合成一组内置 MCP server：内置 server 不写入 `mcp.servers`，但出现在设置页
和状态栏中，其 UI toggle 状态保存在 `[mcp.builtin_servers.<server_id>]`（该表不描述
transport 或 endpoint，也不允许新增 server）。

每个 MCP server 必须配置 `enabled`（默认 true）与 `transport`（`stdio` 或
`streamableHttp`）。stdio server 必须配置 `command`、`args`、`env`，可选 `cwd`；
streamableHttp server 必须配置 `url` 与 `headers`，可选 `bearer_token_env_var`。

启动后由 MCP runtime owner 显式 reconcile 启用且凭据完整的 server：连接建立由 connector
负责，PL 统一维护配置 fingerprint、增量 reconcile、工具命名、冲突检查、健康状态和
generation 原子替换；Studio 只负责组合配置。相同 effective fingerprint 的 reconcile 完全
no-op；手动重连走独立的单 server/All reset；reset 候选失败时保留当前 live generation；
shutdown 是不可恢复终止态。

内置 Zhipu Coding Plan MCP server 固定为：`zhipu_search`、`zhipu_reader`、`zhipu_zread`
（Streamable HTTP，官方对应 endpoint）与 `zhipu_vision`（stdio，经 npm 启动，Windows 运行
时从标准 npm 安装布局解析 node + CLI，配置和状态中不写平台后缀）。这些内置 server 优先
复用 Zhipu Coding Plan provider 的 bearer token 作为 Coding Plan key；若不存在 Coding
Plan provider，则兼容回退到普通 Zhipu provider 的 token。缺少可用 token 时内置 server
的配置状态为缺少凭据且不进入健康探测；检测到 token 时，未显式配置状态的内置 server
默认启用并进入后台探测，已被用户禁用的保持禁用。

模型可见的 MCP tool 名称形如 `mcp__{server_id}__{tool_name}`。PL 对 server id 和远端
tool name 统一规范化，合成名称最长 64 个字符，冲突时追加稳定 hash 后缀；命名规则属于
公共 MCP runtime，产品 Host 和 UI 不重复实现。每个 turn 从 runtime 获取固定 generation
的轮次租约后再安装工具；新 generation 完全 ready 前不对新 turn 可见，旧 generation 在
最后一个租约释放后异步关闭（租约合同见 [09](./09-tool-runtime.md)）。

## 20.9 Skills 配置

`[skills]` 控制本地 skills 系统：

- `enabled`：是否启用 skills 目录、prompt 注入、工具注册和用户 `/name` 手势，默认 `true`；
  关闭时 Studio 仍可保留已发布 catalog 供设置页只读展示。
- `auto_learn`：是否在 Studio 主 turn 结束后启动后台 reviewer 自动沉淀项目 skill，默认
  `true`。
- `project_dir`：项目级 skills 目录，相对 workspace root 解析，默认 `.agents/skills`。
- `user_dir`：用户级只读 skills 目录，默认 `~/.anywork/skills`。
- `system.enabled`：是否启用内置系统 skills，默认 `true`。
- `external_dirs`：额外只读 skills 目录列表，默认空。
- `disabled`：禁用的 skill 名称列表，默认空。
- `auto_learn_min_tool_calls`：触发自学习 review 的最少工具调用数，默认 `5`。

目录优先级、frontmatter 格式、工具合同与自学习语义的唯一权威源见
[10](./10-skills.md)。系统内置 `studio-config` skill 是面向 agent 的 anywork 配置指南，
覆盖配置文件位置、当前 schema、常用配置段、凭据处理和安全编辑行为；任何改变配置路径、
schema 版本、配置段或字段及其默认值、凭据解析优先级、加载/保存/重载语义或最小有效配置
的变更，都必须在同一变更中同步更新该 skill，并复核 skill、本文档与运行时行为一致。

## 20.10 配置草稿与校验

anywork 设置页先加载 canonical provider catalog，再构造产品草稿：默认选中 Studio 产品
默认 preset，也可选择 catalog 返回的任意 preset 或 Custom provider；至少配置一个
provider；可继续添加多个 provider 实例，允许同类 provider 重复，每个 provider key 唯一。
preset、endpoint、凭证提示、协议、允许连接模式、suggested model 和 bundled catalog 全部
来自 catalog 快照，Flutter 不保存生产目录副本。Studio 草稿选择一个默认 provider；五个
模型角色初始化为创建时选择的 suggested/default model 和该模型声明的默认 effort，该选择
只投影为五条 route，不写入 provider runtime。

设置项写入前必须完成本地校验并由 Studio 统一执行完整校验，失败时只在 UI 展示错误，不
写入磁盘：

- provider key 非空且唯一；preset 引用必须存在；Custom provider 必须显式选择 wire
  protocol。
- Responses + WebSocket/Http 合法，ChatCompletions + Http 合法，ChatCompletions +
  WebSocket 在发起网络请求前拒绝。
- API key 非空（本地无鉴权服务允许省略）。
- 每个角色 route 的 model 必须存在于对应 provider 的有效模型集合；同一 provider 下模型
  slug 不重复。
- 角色引用的默认模型必须声明 effort 参数且至少一个候选值，用于生成角色 effort。

## 20.11 凭据策略

Provider 的 API token 保存到操作系统凭据库，service 固定为 `anywork`，account 为
`provider:{provider_id}`；`config.toml` 不保存 token、凭据引用或可逆密文。Provider 仍可
保存 `bearer_token_env` 环境变量名。配置加载后，Studio 在 Rust 内存中注入系统凭据；运行
时按"系统凭据优先，其次读取非空环境变量值，空白值和缺失环境变量都视为无凭据"解析。

设置页的 Preserve/Replace/Clear 语义保持不变：Preserve 不改系统凭据，Replace 在配置提交
前写入并回读，Clear 删除凭据。凭据操作和 TOML 原子替换作为一个 fail-closed 提交流程；
凭据阶段失败时不得覆盖配置文件。版本迁移需要改变 provider 标识时，须按明确映射保留
凭据关联：目标凭据写入并回读验证后才提交配置引用，旧关联在整个迁移成功前保持可恢复，
不能仅按默认 provider id 加载凭据。历史内联凭据仅能在迁移边界转入系统凭据库，不进入
当前配置、日志或 UI；包含敏感内容的原始备份必须受保护。安全边界见 [04](./04-security.md)。

MCP stdio server 的 `env` 按配置原样传给子进程，可能包含明文凭据。Streamable HTTP 的
`bearer_token_env_var` 只保存环境变量名，运行时从 Pure 进程环境读取对应 token 并构造
Authorization header。

## 20.12 Web 搜索配置与凭据门控

Studio 保留两段互不混用的顶层配置。`[web_search]` 只配置 OpenAI 搜索：`mode` 为
`disabled | cached | indexed | live`，默认 `cached`；`context_size`、`allowed_domains` 和
近似位置均可省略。`[deepseek_web_search]` 只包含 `enabled`，默认 `true`；缺失整段时同样
按启用处理。DeepSeek 不接受 cached/indexed、域名、位置或 context size 等 OpenAI 专属字段。

规划先独立解析两边状态，再统一仲裁：当前 route 的 provider 有凭据、使用 Responses
transport、模型支持 web search、endpoint 声明 DeepSeek hosted dialect，且 DeepSeek 开关
启用时，选择 DeepSeek 原生搜索并保持其他普通工具可见；否则沿用 OpenAI
standalone/hosted 规划。DeepSeek 不跨 provider 借用，跨 provider 回退只由 OpenAI 搜索
承担。两边均不可用时分别保留 `disabled`、`missingCredential`、`providerUnsupported` 或
`modelUnsupported`，不能合并为模糊状态。

配置值与生效值必须分离：没有有凭据的 OpenAI preset 时保留 configured mode，但 effective
mode 为 `disabled`——此状态下工具规划不得注册独立搜索或 hosted 搜索，运行时不得创建
独立搜索客户端。可用账户优先当前 turn 的 OpenAI provider；否则按 provider id 稳定排序，
并按 explorer → planner → executor → worktree_executor → reviewer 选择首个指向该 provider
的有效模型，最后才回退到目录首个模型。DeepSeek 被选中时，OpenAI 仍可显示 availability
为 `available`，但 `selected = false` 且 effective mode 为 `disabled`。`cached` 映射为
禁止外部实时访问；`indexed` 映射为显式 indexed 访问；`live` 允许实时外网；`disabled`
完全关闭该路径。两张卡片的保存命令都携带 Settings CAS revision，成功后以完整 canonical
snapshot 回写（仲裁机制见 [06](./06-model.md)）。

## 20.13 LSP 自定义 server 配置

自定义语言服务器声明在 `[lsp.servers.<server_id>]` 表。每个条目必须配置 `command` 与
非空 `language_ids`，可选 `args`、`detection`（workspace 检测文件名/glob，缺省总是匹配）、
`extensions`（文件扩展名，缺省为空）、`display_name`（缺省使用 server id）和
`operations`（`lsp_query` 操作子集，缺省支持全部）。示例：

```toml
[lsp.servers.purelang]
command = "purelang-lsp"
args = ["--stdio"]
language_ids = ["purelang"]
detection = ["pure.toml"]
extensions = [".purelang"]
```

该段与 pl-lsp 内置 catalog 合并；重复 server id 或 language id 与内置/其他自定义 server
冲突时，配置校验以类型化错误 fail-loud，保留原配置，版本处理遵循 20.1。自定义 server
使用通用命令 driver（`<command> --version` 探测，无 repair 语义），运行行为与路由合同
见 [21](./21-lsp.md)。Studio 项目激活时把该段应用进 LSP registry catalog，并纳入激活
fingerprint。
