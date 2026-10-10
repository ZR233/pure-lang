# 20 - Studio 持久化配置

本文是配置文件 schema、provider/model 结构、提示词分层、MCP/Skills/LSP 配置与凭据策略的
唯一权威源；权限语义见 [04](./04-security.md)，模型层机制见 [06](./06-model.md)，设置页
UI 见 [19](./19-studio-ui.md)，Agent Profile 体系见 [12](./12-collaboration.md)。

## 20.1 配置位置与产品身份

anywork 使用独立产品身份，默认仅访问 `~/.anywork`，凭据服务名为 `anywork`，产品环境变量
前缀为 `ANYWORK_`。产品与用户数据的版本演进以 anywork 为起点；显式指定的数据根目录
仍遵循既有参数优先级。

配置文件固定为 `~/.anywork/config.toml`（Windows 下 `%USERPROFILE%\.anywork\config.toml`）。
会话、产品库与调试日志的数据布局由 [存储契约](./17-studio-storage.md) 统一定义。用户 Agent Profile
单独保存到 `~/.anywork/agents/*.toml`。schema 版本以代码常量为准。

配置运行时在 Studio 启动时读取配置；此后普通对话和设置查询只读内存 canonical snapshot；
配置文件不存在时设置页展示内存中的默认配置；外部文件变化只有显式重载命令才能应用。
支持在线模型探测的 provider 在每次程序启动各独立探测一次，设置查询不触发额外联网。首屏先使用
成功缓存或默认模型定义，不等待所有网络结果；模型观察不写回 config.toml。完整语义见 20.5。
普通设置项在用户修改后即时写入配置；独立新增/编辑页面保留本地草稿，必须点击页面内保存
按钮才写入，取消则丢弃草稿。

配置版本演进必须满足以下迁移契约；当前实现缺口集中见
[17.7](./17-studio-storage.md#177-迁移实现状态与剩余边界)，不能据此假定已实现自动迁移。

- 启动时在产品发布前识别配置版本，使用明确的版本转换路径将 anywork 历史配置与用户
  Agent Profile 转为当前结构，支持跨版本升级；保留用户选择、provider 身份与凭据关联。
- 转换前备份原始文件且不覆盖已有备份，转换后执行完整校验，通过原子替换或可恢复步骤
  提交。涉及多个配置
  文件、数据库关联或凭据时由 Studio 协调，不能发布新旧结构混合的 canonical snapshot。
- 新增字段只能使用该版本迁移明确规定的默认值；旧字段在迁移边界转换，不能用重建整份
  默认配置、猜测模型路由或运行时兼容补齐代替迁移。正常读写只使用当前 schema。
- 未来或未知版本、无法解析、无效引用或缺失迁移路径返回无法恢复的数据错误，由统一启动
  协调器按 [17.2](./17-studio-storage.md#172-启动与按需激活) 完整备份后重建初始状态。
  配置模块自身不静默覆盖默认值。凭据或 IO 环境失败保留现场并报告，不触发重置。
  配置缺失判定只依据路径条目；权限、元数据及符号链接异常不能冒充缺失。
  18→19→20 优先保全迁移。解析诊断只含路径、类别和行/列位置，不回显原文或凭据。
- 中断后可安全重试或恢复一致状态；运行期显式重载只接受当前有效结构，失败保留已有
  canonical snapshot，不在查询或重载中隐式迁移。迁移结果与失败通过脱敏诊断报告。

迁移验证应覆盖版本跳跃、字段重命名、路由与 Profile 关联、凭据标识变化、未知版本、
损坏配置、备份或提交失败及重启恢复，证明用户选择与凭据可用性得到保留。

启动支持明确的 18→19 迁移：仅从 `disabled_system_agents` 移除 `planner` 并提升版本，
保留五条模型路由、其他设置和 provider 凭据关联。19→20 迁移把旧 `planner` route 复制为
`mode.simple` 与 `mode.task` 的默认 route，从 `models.routes` 删除 `planner`，并保留四个系统
子代理 route、provider、其他设置和凭据关联。两步都先校验和备份，再原子替换；迁移失败
保留原文件并向协调器返回明确错误。没有转换路径的版本保留原字节进入统一备份恢复，
环境失败则停止启动。正常运行不接受禁用主智能体或以该标识保存用户 Profile。

所有 Settings command 必须携带 `expectedSettingsRevision`，成功返回新的
`SettingsConfigSnapshot`；目录发生变化时另返回 `ModelCatalogSnapshot`，并分别发布
`SettingsConfigStateChanged` 与 `ModelCatalogStateChanged`。前者表示用户 desired 设置，后者
使用独立目录水位，不推进 desired revision。直接响应为 `SettingsStateResponse`，只是为了让
调用方一次得到两个独立资源，消费方仍按各自 revision 幂等合并；不得返回聚合状态、raw JSON
或 raw map。CAS 或校验失败时保留当前 canonical 状态，不覆盖新配置。

### 20.1.1 设置 mutation 的资源边界

写入接口按业务资源划分，而不是把整个 `StudioSettings` 快照作为 patch。标量设置、列表设置、
每个 MCP 字段、每个 Mode 的模型/思考强度以及每个 Agent role 的模型/思考强度，分别使用一个
typed mutation；请求只携带目标标识和新值，其他字段由配置 owner 从最新 canonical state 解析。
因此连续选择模型、思考强度或快速返回设置页不会把旧 selector 快照写回，也不会因为目录刷新
重放其它设置。

Provider 编辑和已有 Thread 的 model route 是有意保留的资源级原子接口。Provider 的 endpoint、
凭据、模型声明和连接覆盖需要联合校验，Thread route 的模型、思考强度和 runtime route 也必须
同时满足能力与活动状态约束；把它们拆成多个可独立落盘的请求会允许半个无效资源被观察到。
它们仍各自拥有独立的 mutation key、CAS、pending、重试和失败状态，并且不会携带其它 provider、
Mode 或 role 的旧快照。读响应可以为了减少往返同时返回 config/catalog 两个快照，但消费端必须
按各自 revision 合并，读聚合不改变写入粒度。

## 20.2 配置职责

每个 provider 的模型目录独立保存按模型 slug 索引的压缩阈值覆盖值，bundled、附加与 explicit
模型使用同一解析规则。覆盖值为正整数 tokens，缺省表示使用模型默认值；恢复默认仅删除对应
模型覆盖项。默认值与安全上限由 [06](./06-model.md) 定义，不复制进用户配置作为第二事实源。
用户新保存的无效值和不存在的模型引用在发布配置前拒绝，失败不改变 canonical snapshot。在线
目录变化使既有引用不可用时保留选择并报告 unavailable，见 20.5。新增可选覆盖
集合缺省为空，已有配置和凭据保持原样；既有模型默认阈值字段继续保持默认元信息语义。
Settings 命令携带 revision 并返回完整 canonical snapshot，保留未修改的模型和 provider 数据。
模型刷新将覆盖值纳入冻结配置身份，新建、恢复和后续安全刷新使用最新解析值，不改写已发请求。

pl-model 拥有产品无关的模型配置值对象：角色路由配置（provider/model/effort 校验与解析）、
provider 配置与模型路由配置，负责把路由解析为运行时 endpoint 和唯一选中的不可变模型信息。
pl-studio-runtime 拥有：Studio 配置 schema 与启动期版本迁移、配置文件路径、
TOML 解析、原子保存和默认值、instructions/skills/MCP/runtime/disabled_system_agents 与
UI 配置、Agent Profile 文件的逐文件解析与原子保存，以及 Thread 首轮固定 instruction
snapshot 的生成。pl-model 只消费已经解析好的 provider 和模型信息，不负责文件 IO 或路径
定位。
配置 owner 在同一发布边界区分磁盘 desired config 与非持久模型目录 overlay；解析视图合成两者，
所有 route、Profile、标题与 Settings 展示消费同一有效目录。保存只序列化 desired，不能把发现模型
塞进 additional_models 后随其他设置写回，也不能将其投影为可编辑 custom model。
Provider 编辑提交只发送既有或用户显式修改的连接覆盖，不把在线模型的当前默认连接方式
转成持久覆盖。更换地址或凭据会失效旧目录观察；未改动的连接覆盖与路由仍按既有 desired
选择保留，不能因此次保存制造对旧在线名单的新引用。

`[ui]` 仅保留 `follow_active_turn`（默认 true）和 `compact_timeline`（默认 false）；主题不
属于持久化设置。未知 UI 字段按既有规则忽略，正常保存后不再输出；启动不因未知字段重写
配置、触发恢复或提升 schema。

## 20.3 根路由与系统 Profile

配置不使用 `active_provider`。root Agent 的 desired route 属于各自 Thread；主配置以
`mode_model_routes` 保存各 Mode 的新建/切换默认 selector。`mode.simple` 与 `mode.task`
必须存在并独立保存 provider/model/effort；合法自定义 Mode ID 可以持久化，尚无条目时继承
`mode.simple`，首次显式选择后形成自己的条目。Mode 默认值不反向修改其他现有 Thread。

`models.routes` 只保存系统及用户子代理 Profile 的路由；四条内置系统 route 为：

| 配置 key | 中文角色 | 用途 |
| --- | --- | --- |
| `explorer` | 探索者 | 代码现状、文档、网络资源和上下文的只读探索 |
| `executor` | 执行者 | 实施修改和验证 |
| `worktree_executor` | Worktree 执行者 | 在独立 Git worktree 实施修改和验证 |
| `reviewer` | 审查者 | 代码审查和结果检查 |

探索协作由共用系统提示词统一约定：主代理在只读调查和 Task 的 planning 阶段也建议
按需委派 explorer，不以实施计划批准为探索前提；多个独立问题可按可用容量并行探索，
真实依赖保持串行。派发须提供自包含的背景、问题、范围、检索线索、来源要求及完成标准，
探索者返回结论、可追溯证据和不确定项，由主代理综合判断，减少原始检索材料占用主上下文。
该建议不扩大阶段或工具权限，不授权修改文件、Git 或外部状态，也不要求所有任务强制并行。

系统子代理 Profile 由内置结构体启动注册，不生成 TOML；身份、用途、指令和工作区模式
不可编辑、不可删除，可通过主配置 `disabled_system_agents` 禁用。主智能体在会话起始页与
会话状态栏选择模型，不在 Agents 页配置；系统 child route 在子代理区域配置。用户 Profile
的文件名 stem 是 Agent ID。
`list_agent_profiles` 只返回启用且路由可解析的 Profile；`spawn_agent` 创建 child 时冻结
系统指令、provider、model 与 effort，此后文件变化不改变既有 child；设置页另读完整
catalog，被禁用的用户 Profile 仍可编辑并重新启用。

每个角色必须配置 `provider`、`model` 和可选 `effort`。`effort` 使用字符串，校验对象是
对应模型 parameters 中 `name = "effort"` 参数的候选值：模型声明非空候选时，角色必须选择
一个合法候选；模型没有声明该参数时，角色必须省略 `effort`。候选、默认值和 wire 规则只
来自模型目录（见 [06](./06-model.md)），角色配置不保存第二份候选或默认值。provider 不
保存 `default_model`，模型选择只由路由决定；历史结构按 20.1 转换，缺失必需路由或无效
引用明确报错，不能静默重置用户选择。
保存 provider 字段时，未改动的 Mode 与 child route 原样提交，包括暂不可用的模型和 effort。
只有显式选择默认 provider 或删除路由所属 provider 才重新选择相应模型与默认 effort；实例
更名只迁移 provider 引用。编辑命令的空 effort 表示省略该选择，不从当前目录补入默认候选。
完整 child route 提交不构造备用路由；只有缺失角色需要补齐时才解析明确的默认 provider。
未承担新路由的在线实例允许其展示默认模型为空，不能让其合法空目录阻塞其他实例保存。

## 20.4 TOML 示例

本地 TOML 使用 `snake_case`，不同于 API wire 格式。精简示例：

```toml
schema_version = 20

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

[mode_model_routes."mode.simple"]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[mode_model_routes."mode.task"]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[models.routes.explorer]
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
Settings 的自定义模型快照必须携带同源的 typed input capabilities（modality、来源和限制），
经 FRB 投影到 Flutter，与内置 catalog 共用转换逻辑。附件入口不能因自定义模型元数据
在展示链路中丢失而退化为 text-only；本地和 SSH 项目使用同一能力判断。

ModelPricing 明确区分未知价格与包含费率的定义：货币不做汇率转换；输入、缓存读、缓存写
与输出按互斥类别计费，reasoning 已包含在输出内；用量或费率缺失时标记未计价；关闭计价
与实际零费用分别表示；每次请求冻结价格定义、计价开关和发送时间，最终账单按最终用量
选择长度档位，跨时段请求不拆分 token（本地估算口径，见 [06](./06-model.md)）。内置价格
来源、核对日期及完整档位保存在后端目录，设置页直接展示后端提供的行；文档不保存价格
快照。

Bundled 默认定义只读，`additional_models` 只能添加不与默认定义冲突的 slug，不支持用户对默认
条目的字段级覆盖；完全自定义 provider 用 `Explicit` 保存完整模型列表。支持在线探测的 bundled
目录可叠加非持久成功观察，按 [06](./06-model.md) 选择完整 API 名单并保留手工声明优先权；不将
explicit 目录自动改为远程目录，不借此改变 config.toml schema 或既有模型字段命名。

### 在线模型观察与配置校验

设置页可按实例手动刷新模型目录，与该实例正在执行的启动查询合并；刷新只推进目录水位，
通过同一 canonical snapshot 返回完整有效模型与来源、检查时间及类型化错误。GUI 不从预设
补回在线名单中缺失的模型，合法空名单保持为空；旧选择以不可用状态保留。HTTP 与原生桥接
提供相同刷新语义。

每次启动对每个支持探测的实例独立进行一次查询；同 preset 的多个实例也独立。查询跟随当前
base_url，网络边界归 [06](./06-model.md)，成功缓存与默认回退归 [17](./17-studio-storage.md)。
缺少所需凭据形成该实例失败，不阻塞其他实例；没有定时轮询，用户可另发手动刷新命令。

配置解析先恢复该实例同身份成功声明或默认定义，再验证模型声明、结构与用户 Profile。支持在线
目录的既有 route/effort/connection override 因缓存缺失或服务撤下模型/候选而不可用时，保持用户
选择并返回外部绑定 unavailable，不自动猜模型、删覆盖值或触发全局数据重置。显式手工目录、
缺失 provider、坏 schema 与声明本身继续严格校验；用户新保存/切换的选择须在当前有效目录中合法。
已知配置版本迁移仍使用既有数据保全流程，模型观察自身不引入新的 TOML 版本。

各实例探测按自身 generation/查询身份裁决结果，并合并到当前 desired 状态，不以开始时的旧全局
Settings revision 提交。别家成功不得使本家 stale；用户更换地址、headers、凭据或目录来源、删除
实例后，旧结果丢弃。同实例手动与自动刷新合并或串行，不并行覆盖。等待网络不持全局配置锁。
目录变化只通知受影响 provider 的绑定消费者：在途请求保持冻结，后续安全边界 deferred 重绑或
明确 unavailable；不因目录观察重装无关 provider 的工具或重算历史费用。

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

内置目录装配失败必须沿目录查询、模板选择和草稿构造接口返回原始错误，不得静默使用空目录
或触发 panic；此类错误属于程序定义故障，不进入用户配置损坏的重置流程。

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

搜索不是单选：`StudioWebSearchSettings` 与 `StudioDeepSeekWebSearchSettings` 只暴露
configured/effective/availability、provider_id/model 与原配置开关（`mode` / `enabled`），
不再有 `selected` 字段，也不存在互斥的 exclusive 路线。canonical DeepSeek preset 声明
DeepSeek 原生 standalone 能力；preset 实例覆盖非 canonical base_url 时由 PresetDefaults 仅撤销
该 DeepSeek 原生方言（连同其它 hosted/Responses 能力），非 canonical DeepSeek endpoint 必须显式
声明该方言。此撤销不改变既有 OpenAI `OpenAiSearchApi` 继承语义：覆盖非 canonical base_url 的
OpenAI endpoint 仍保留 `/alpha/search` standalone。显式 capability 仍可重新声明。

设置页运行独立的 service planner，不依赖会话、route 或当前模型：每条后端只按 provider
capability 与凭据解析 configured/effective/availability、provider_id 与 model。OpenAI
standalone 服务优先使用该 provider catalog 的 `gpt-6-sol`，否则用该 provider 首个有效模型；
provider 之间按 ID 稳定排序。DeepSeek standalone 服务默认模型 `deepseek-flash`；线程内优先
当前支持原生搜索的 DeepSeek provider，否则同样按 ID 排序。服务模型与会话模型独立，不再按
explorer / planner / executor / worktree_executor / reviewer role route 选模型。

实际 thread 消费时才用当前模型是否支持 function calling 决定本轮是否注册对应 standalone
搜索工具，它不改变设置里已解析的服务可用性。OpenAI standalone、DeepSeek 原生 standalone 与
可用的 hosted 搜索互不抢占，也不隐藏 MCP、LSP、文件和命令工具；只有 DeepSeek 搜索工具的
描述写明收费兜底，其他搜索不分优先级、不按 provider 排序、不自动跨供应商回退，也不因某一路
可用而停用另一路。

配置值与生效值必须分离：缺少有凭据的 provider 时保留 configured mode，但 effective mode
为 `disabled`——此状态下不得注册对应 standalone 或 hosted 搜索工具，也不得为它创建客户端。
服务 planner 的每后端可用性只看 provider capability 与凭据（`disabled` / `missingCredential` /
`providerUnsupported`），不因当前会话角色或模型不可用而降级；当前模型能力只在真实 thread
消费时生效。`cached` 映射为禁止外部实时访问；`indexed` 映射为显式 indexed 访问；`live` 允许
实时外网；`disabled` 完全关闭该路径。两张卡片的保存命令都携带 Settings CAS revision，成功后
以完整 canonical snapshot 回写（多后端仲裁见 [06](./06-model.md)）。

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

## 20.14 独立设置命令

配置 mutation 按最小业务资源拆分：provider 实例、默认 provider、provider 删除、单个 Mode
route 的 model、单个 Mode route 的 reasoning effort、单个 Agent role 的 model、单个 Agent
role 的 reasoning effort、permission、instructions 字段、skills 字段、单个 MCP server、
general 字段、web search 字段和 DeepSeek 开关各自有 typed command。所有 command
携带当前 `expectedSettingsRevision`，由同一 runtime command lock 做 CAS；成功返回新的 canonical
Settings snapshot 并发布设置事件。Bridge 与 loopback HTTP 均使用 `PUT /api/v1/settings/field`
承载 `SettingsFieldUpdate`，FRB 只暴露同一 typed handler。一个 command 不得把页面当前完整 provider、roles、mode
routes 或其它字段的旧副本作为无条件替换值；需要迁移引用时必须在删除命令中显式给出替换目标。
这里的“同一 handler”只表示传输入口稳定；请求体的 `kind` 是单字段/单资源类型，服务端
不得根据一个字段请求顺带修改其它设置。Provider 与 existing Thread route 仍是资源级原子
接口，因为 endpoint/凭据/模型声明或 runtime route 需要联合校验；它们不会把其它 provider、
Mode、role 的旧快照带入提交。

模型目录刷新属于另一个 command lane，只推进 `modelCatalogRevision`、provider catalog
状态和有效模型描述。它不会修改 desired route，也不会阻塞配置 command。route 指向被撤下的
模型时保留 desired 值并返回 unavailable；用户显式选择的新 route 仍需按当前有效目录校验。
