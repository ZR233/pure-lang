# 06 - 模型层

本文是 provider 适配、模型目录、transport、流归一化、可调参数与计量的唯一权威源；配置侧的
provider/model 结构见 [20](./20-config.md)。

## 6.1 职责与边界

pl-model 是模型目录、provider endpoint 与模型协议适配层：把核心层的统一请求转为具体 API 请求，
并把流式结果归一化为 provider 无关的模型事件与完成响应。Studio 显式构造 model 适配器并交给核心
模型会话契约；core 不代理 provider 或模型配置。pl-model 不维护产品 agent/session 历史，不解析
CLI，也不决定产品阶段；它只维护与一个 Thread 同生命周期的模型会话：物理连接、脱敏 fingerprint
与 Responses continuation 状态。pl-model 可以消费已经解析好的自定义模型列表，但不读取配置文件。

职责按四个稳定域组织：模型元数据/能力/参数/transport/价格与内置目录；provider 配置、preset 与
解析后的 endpoint；provider 无关的请求/响应/工具/用量/压缩与 canonical 模型事件；绑定一个已
解析模型的单模型运行时与唯一 OpenAI-compatible codec。

OpenAI Responses WebSocket、Responses HTTP/SSE、OpenAI Chat、DeepSeek 与智谱/GLM 兼容接口在
进入唯一 accumulator 前必须统一为 canonical 事件：response started/id、文本、思考、reasoning
summary、工具参数、工具 ready/done、usage 与完成/失败。核心层、Studio timeline 和外部集成方不
解析 provider 原始 JSON。同一协议族的 usage alias、cache details、reasoning details 与工具
identity 只能由共享的 typed normalizer 解释一次；SSE、WebSocket 与非流式 fixture parser 只负责
提取各自 envelope，不能各自维护字段优先级或 fallback——同一 usage/tool fixture 经过不同
transport 入口必须得到完全相同的 canonical 用量与工具身份。底层 event stream、decoder 和
accumulator 只服务单模型运行时与只读诊断投影，不是宿主 API；外部宿主通过运行时的"从路由构造"
入口获得模型绑定或辅助请求客户端，执行器接收已构造的运行时和冻结的 reasoning 参数。

### 工具调用身份

completion 内部 stream 状态机负责稳定工具调用 identity。工具调用一经解码即具有必填身份
（item id 与 call id）：Responses 使用事件携带的 item id 与 call id，两者都缺失是协议错误；
Chat Completions 以 chunk index 构造 stream id / item id，并确定性赋 call id = item id。不存在
可选 call id 或 id 与 call_id 互为 fallback 的规范化路径。Responses 可能先发送只有 provider
item id 的 `output_item.added`，后续 delta 或 done 才补独立 call id；同一个工具调用一旦通过
任一非空身份进入 accumulator，后续 late metadata 必须升级原 accumulator 并合并到同一个 open
tool，不得拆成第二个 tool call 或第二个 trace part。trace 的 tool part id 以最早稳定的 provider
item / runtime tool id 为锚，call id 只作为 metadata 写入 tool snapshot，用于协议回放和
provider tool result 匹配。

## 6.2 依赖与公开 API

pl-model 实现并依赖 core 的模型会话/模型请求契约，core 不依赖 model。产品 DTO 和 adapter 内部
消息可依赖 pl-protocol；pl-trace 只读消费 core 观察接口，不作为模型执行门面。provider 适配可以
使用 async-openai、reqwest、tokio-tungstenite 与 serde 等依赖，它们只用于内部 transport、typed
protocol request 和 typed stream event 解析，不向 pl-core 暴露。

公开 API 按四个稳定域组织（completion / model / provider / runtime），消费方通过域模块前缀访问
类型；crate 根不重导出本 crate 自有类型（错误基础类型与 Result 在根重导出），同一公开接口只有
域级一条 canonical 路径。各域精确重导出其公共签名中出现的 pl-protocol 类型：公共类型字段与公共
方法签名直接使用的依赖类型必须能从本 crate 导入，消费方只依赖 pl-model 即可命名完整签名；禁止
与公共签名无关的成套镜像转发。

## 6.3 模型能力矩阵

模型能力声明使用结构化能力矩阵，不使用 bitflag 或旧的输入模态列表作为主协议。配置和运行时只
接受结构化的 capabilities 对象：

- 基础能力：streaming、temperature、reasoning、web_search。
- 输入/输出模态：input、output，取值为 text、image、audio、video、pdf。
- 工具能力：function_calling、parallel_tool_calls、custom_tools、freeform_tools、
  programmatic_tool_calling。
- 推理交错字段：interleaved.field，当前支持 reasoning、reasoning_content、reasoning_details。

Studio 读取这些能力做装配校验和 UI 展示：图片输入必须要求模型声明 image 输入，工具调用必须匹配
工具能力，推理请求必须匹配 reasoning 能力。provider 私有差异不扩散到 pl-core。

提示词缓存不是由模型 slug 隐式推断的基础能力：provider 服务能力声明 endpoint 的缓存方言，模型
目录声明该模型是否报告缓存写入 token，核心层把两者与 wire protocol 合成为穷尽的有效缓存策略。
未声明能力的自定义 Responses/Chat endpoint 默认不发送任何缓存专属字段。

模型协议配置由模型绑定持有：transport 声明连接方式，请求选项区分 Responses 与 Chat。Responses
只携带最大输出字段和 programmatic tool calling 配置；Chat 只携带最大输出字段、parallel tools、
include usage 与 tool stream 配置。厂商原生选项在具体客户端公开，通用调用和原生调用共同经过同一
执行器；参数映射与开放原生扩展仅在适配边界解释，core 不拼接供应商字段。MiMo 使用
`max_completion_tokens`；智谱按明确模型能力启用 `tool_stream`、保留 thinking；兼容客户端只发送
所选协议的通用字段。

## 6.4 Provider 与运行时

模型执行采用窄接口与具体供应商客户端组合：统一入口只负责推理；OpenAI、DeepSeek、智谱、MiMo 与
OpenAI-compatible 拥有各自的具体客户端和类型化原生选项，动态路由使用持有这些客户端的封闭枚举。
单模型运行时与辅助请求客户端显式开放类型化供应商访问入口，不使用 `Any`、向下转型或包含所有
可选操作的大 trait。

通用与原生调用共享 transport、重试、取消、stream 生命周期与计量路径。余额、配额和远程压缩是
独立能力，只由实际支持的对象提供；Responses 协议本身不意味着支持远程压缩。通用模型目录与协议
绑定配置分离，Responses、Chat 及厂商选项只保存本身适用的字段；已知私有协议由 typed DTO 表达，
开放扩展与 opaque context 保真留在供应商边界。

core 冻结工具计划（ToolPlan）并拥有工具权限、执行、并发与结果提交；供应商边界落实原生
function/custom 工具、programmatic caller、hosted search、thinking 与工具流优化。原生优化不得
丢失其他已注册工具；reasoning、caller、opaque context 与工具结果配对在多轮、重试和恢复中保持
完整。具体模型和 endpoint 的显式能力决定优化，不按 slug 或 URL 猜测。

每个 provider 实例保存 preset 身份、endpoint override、凭证、headers、tool wire policy、服务
能力和 catalog binding；具体模型由角色 route 选择，不保存第二份 provider 默认模型。模型目录由
provider 配置解析（bundled catalog 加追加/显式模型），不从全局列表兜底；目录可按 slug 保存当前
连接方式 override。reasoning/thinking/effort 的 wire 规则由模型 parameters 声明驱动（见 6.8）。

OpenAI-compatible 自定义模型必须显式提供完整 transport profile：Chat 模型只能声明
ChatCompletions + HTTP；Responses 模型可以声明 Responses + HTTP，或同时支持 HTTP/WS 并指定默认
模式。具体 base URL、headers、tool wire policy 与 endpoint 能力由 provider 提供。Zhipu Coding
Plan 是 catalog preset，默认指向官方 coding 计划 endpoint 并引用智谱模型目录，不新增 runtime
变体；MiMo API 与 Token Plan 同为两个 preset，共同引用一个 MiMo catalog。

## 6.5 协议与 Transport

pl-model 支持 Responses API 与 Chat Completions API 两种协议 API；差异保持在 pl-model 内部，
核心层只看到绑定模型的 runtime、精简后的完成请求/响应和 provider 无关的模型事件。所有当前
preset 复用同一 OpenAI-compatible codec 的两种 wire API。协议与连接模式正交：Responses 支持
WebSocket 或 HTTP，Chat Completions 只支持 HTTP。协议由模型 transport profile 声明，不由
provider 实例统一决定；同一 provider 实例下的不同模型可以使用不同协议。

transport profile 是模型信息的必填字段，包含 protocol、supported_connection_modes 与
default_connection_mode；provider 的模型目录可按模型 slug 保存 connection_overrides，解析后的
模型把 override 投影为本次请求的最终连接方式。Chat + WS、空支持列表、默认模式不在支持列表，
以及 override 指向未知或不支持模式的模型，都在配置加载/保存时拒绝，校验失败返回类型化的模型
profile 错误（携带 slug 与涉事模态/wire 上下文），消费方并入配置错误；媒体契约校验使用同一错误
类型。Web 与 Flutter 只渲染模型目录返回的 transport 和当前 override，不按 preset ID 推断。

内建矩阵固定为：全部 GPT 使用 Responses，支持 WS/HTTP 且默认 WS——选择 HTTP 时仍调用
`/responses` 并消费 SSE，绝不切换到 Chat Completions；DeepSeek V4.1 Flash 与 V4 Pro 使用
Responses/HTTP；全部 GLM 和全部 MiMo 使用 Chat Completions/HTTP。runtime 按当前模型选择对应
endpoint path，同一 provider 实例可以路由不同协议的模型。

Responses WebSocket 使用 `/responses` 握手和 `response.create` 帧，并固定 `store: false`。连接
及 continuation 属于 Thread 的模型会话，按模型、协议和连接方式隔离；断线、取消、未完整消费或
无效 continuation 都丢弃旧连接，新连接使用冻结的完整输入和附件。建连保持系统 DNS、IPv4/IPv6
交错竞争及 15 秒握手上限。

### 重试预算

每次逻辑模型请求在首次发送之外最多重试 5 次；建连、流中断、无效 continuation 和切换连接方式
共用这一预算，内部不嵌套重试，底层 HTTP 客户端与第三方库的默认重试关闭。瞬态失败使用有界指数
退避和稳定抖动，优先采用供应商等待提示；等待和建连均可取消。WS 完整重试一次仍失败后，同一模型
会话切换 HTTP，后续请求保持 HTTP；切换也计入剩余重试预算，不重置次数。认证、配置、协议等永久
错误立即返回。

普通文本、推理及尚未交付执行的本地工具参数流中断时，允许重试当前模型请求。失败尝试的可见片段
保留为独立项目并结束其流状态，新尝试采用独立项目身份；只把成功尝试交给核心层执行工具和追加
模型上下文，已执行的前序工具结果保留在冻结输入中，不重跑整轮。含供应商托管工具且已经产生流
事件的请求，以及已经报告使用量的未正常结束响应不自动重放，以免重复远端副作用或丢失计费事实。
耗尽预算返回原始类型化失败，保留已提交内容。

重试开始通过运行时进度项目显示"连接中断，正在重试（n/5）"，恢复及耗尽也显示明确结果；进度只
进入界面与冷历史，不注入模型上下文。重试期间轮次保持运行、允许停止，不修改数据库布局或界面
协议，也不依赖持久化进度。

### 请求组装

effort 等可调参数的 wire 写入由通用透传机制驱动，协议层不为每供应商硬编码 reasoning/thinking
映射。请求组装先把强类型核心字段（model、messages、stream、tools 等）序列化为 JSON 对象，再
依次注入 base body（模型请求 profile 的固定体，如 DeepSeek 固定的 `thinking.type = enabled`）
与 parameter wire（用户选中的候选值按模型 parameters 声明写入或移除字段，见 6.8）。覆盖优先级
为 parameter wire > base body > 协议默认字段。

OpenAI Responses 的 `reasoning.summary` 仍按 Codex wire 语义发送（Auto 和兼容层的 Enabled 都
发送 `auto`，Disabled 不发送 summary 字段），由 reasoning 配置的 summary 选项独立驱动，不进入
parameter wire。模型返回的 `reasoning_content` 进入 canonical reasoning event；历史回放时仍通过
assistant message 的 `reasoning_content` 字段写回 Chat Completions。

model 从核心冻结请求转换的完成请求始终带完整 canonical input，且不携带 model、stream、store、
previous response、trace 或 transport session。runtime 固定使用流式请求；Responses 固定
`store: false`。prompt cache 和 trace 属于单次 invocation context，continuation 只由模型会话
管理。model adapter 在 invocation 内创建事件 sink；轮次选项只承载宿主可控的取消状态，不暴露
pl-trace 类型。模型会话在相同连接和 fingerprint 下由上次完整请求前缀计算增量，仅 WebSocket 帧
设置 `previous_response_id`；Responses HTTP/SSE 和 Chat Completions 始终发送完整历史。

完成请求中的 system 角色表示本轮临时前置指令或开发者上下文：Responses endpoint 序列化为
input message role `developer`，避免发送不被部分 Responses 兼容服务接受的 `system` role；
Chat Completions 仍序列化为 `system`。

完成请求的工具列表使用 pl-protocol 的 ToolSpec——provider-neutral 的唯一 wire 事实。每个
model step 携带当前工具计划的完整可见工具列表；adapter 只把冻结规格转为 Responses/Chat typed
body，不自行发现、过滤或注入 agent 工具。core 保存不透明工具声明与稳定工具 ID，每步冻结声明和
执行租约；model 将声明编码为 provider wire 并恢复 assistant 调用名称/原参数，具体工具实现与
参数解析属于 pl-tool。hosted 工具仅在 model 内配置，不向 core 注册占位 executor；MCP 名称映射
属于工具适配，不由 core 反解析。deferred reveal 由 Thread 保存稳定声明身份：重连不改写模型可见
前缀，删除或声明变更使旧状态失效；完整原调用参数和当时使用的名称进入模型历史，下一次编码不按
当前工具目录重写过去。

## 6.6 多模态消息

消息内容只有一个有序 multipart 形态：parts 列表。持久协议仅允许文本部分与附件部分
（attachment_id + modality）；附件 modality 为 image、video 或 file，PDF 属于 file。持久消息
不得保存本地路径、外部 URL、Base64、provider file id 或请求期 data URL，也不保留
text/multipart 双形态。

模型输入能力同时声明 modality、允许的输入来源与格式/数量、单项字节、批次总字节和图片宽高
限制；模型请求 profile 为每种 modality 声明有序的发送表示（封闭枚举：远程 URL、provider file、
data URL）、provider wire 映射和混合规则。能力声明必须至少有一条当前输入可用的首发路线和一条
基于持久快照的重放路线；未知模型或缺少完整 profile 的 modality 按不支持处理，不按模型名、
provider 名或 wire 宽松程度推断。

pl-model 在准备阶段通过资源访问端口把稳定附件引用 materialize 为自身私有的已准备内容部分：
只携带已校验 bytes、当前首发允许使用的瞬时 URL 或 provider file id；pl-model 不读取 Studio
存储，也不解析本地路径。同一 modality 批次选择同一种表示；provider 文件上传失败只能在推理请求
发出前整批切换到下一条 profile 路线，流建立后不得自动重发。

代理主动读取图片使用独立的 `view_image` 工具和工具媒体上下文条目。MCP typed image result 只有
在调用它的精确模型同样声明 image 输入、完整快照 replay profile，且当前 Thread 安装 attachment
runtime 时才进入相同通道；否则图片块只产生有界诊断文本，不持久化为仅供 UI 使用的附件。工具
成功结果仍先以普通 typed tool result 闭合 provider tool call；同一批次的全部结果闭合后，core
再追加一个按 call 顺序排列的工具媒体上下文条目，每项只保存 call id、安全展示标签与
thread-owned 附件元数据。它不是用户消息，也不进入用户 Timeline；Responses 与 Chat adapter 都
把该上下文投影为一个内部 user multipart，并按"标签文本、图片"顺序发送——Chat 的并行 tool
message 保持连续，两种协议复用同一份 durable history。图片 bytes 仍由宿主附件 loader 在请求期
materialize，同 Turn 后续 inference、失败重试和恢复不得读取原始 workspace 路径。

工具媒体宿主边界由 pl-tool 拥有，Studio 提供持久化实现，model 拥有媒体上下文的版本化编码与
typed 解码；core 只保留通用上下文和资源引用。模型请求通过资源读取端口 materialize 已归档
媒体，不重读源路径；实际模型图片能力使用 prepared call 冻结的投影材料，不根据当前目录重建
历史能力（准入与资源投影约定见 [16](./16-core-contracts.md)）。工具图片、对应 tool results
与前导 assistant tool calls 在历史中保持关联；Studio 的工具 Timeline 附件由同一持久化媒体事实
生成，不伪造用户消息；GUI 读取同时校验 Thread 访问权与资源引用归属，显示与重放不因本地/SSH
环境而分叉。

MCP image content 在持久化前必须先检查编码长度、严格 Base64 解码、校验声明 MIME 与真实文件头，
再复用 `view_image` 的格式、解码、尺寸和模型限制。一个 MCP result 的图片批次任一项无效或写入
失败时整批不发布；`isError` result 在图片解码和写入前短路。tool result、trace、audit 与
SQLite 不得保存 typed image 的原始 Base64，只保留有界占位文本、摘要、尺寸、MIME 与附件 id。

OpenAI Responses 的已实现图片路线使用 `input_image`；OpenAI Chat 使用 `image_url`。智谱 Chat
codec 还定义 `video_url` 与 `file_url`，但模型只有在精确请求契约、限制与快照重放路线都经过
验证后才声明对应能力。GLM-5.3-Flash 当前只声明 text/image：远程图片首发优选 URL，本地图片
以及历史、重试和恢复统一使用 Data URL。未声明相应 modality 的模型必须在任何附件 IO 或凭据
读取前拒绝。DeepSeek V4.1 Flash 当前声明 text/image，并通过 Responses `input_image` 发送：
远程图片首发优选 URL，本地图片、历史、重试和恢复使用 Data URL；支持的快照格式固定为 JPEG、
PNG、GIF、WebP。官方 Files API 在 provider file 上传、瞬时 file id 与快照回放生命周期全部实现
前不声明该表示。官方对少于 15 张与至少 15 张图片使用不同边长上限：canonical profile 选择全
批次均可成立的 4096 像素保守上限，并以 32 MiB snapshot 批次总字节上限保证 Data URL 重放不会
越过接口的 48 MiB 请求体边界；该保守子集不按模型名在 adapter 中特判。

## 6.7 自定义模型与远程压缩

产品宿主使用自己的配置读取完整的模型配置，经校验与解析得到已解析模型路由后，才交给 pl-model
构造模型绑定或辅助请求客户端；pl-core 与 pl-model 都不读取 `~/.anywork/config.toml`。

Bundled catalog 只读，配置只能通过 `additional_models` 追加不冲突 slug；完全自定义 provider
使用显式模型列表。附加与显式模型都必须声明 transport；模型目录的 connection_overrides 只保存
当前模式选择，不修改模型声明的支持矩阵。

模型信息中的 `base_instructions` 是模型级基础提示词来源，进入 Studio 的 instruction
assembler；配置中的 `[instructions].base_override` 可以完整替换它。模型信息中的
`context_window`、`max_context_window` 和 `auto_compact_token_limit` 只描述模型能力与默认阈值；
压缩政策与摘要要求由 Studio 选择，model 执行辅助调用，core 校验并原子提交上下文替换与持久化，
pl-model 不维护压缩状态。

模型调用绑定在请求准备阶段冻结解析后的可选 context_window，随请求及结果 receipt 保存，供实时
和历史消费者读取。容量来自实际绑定模型的 context_window，缺失时使用其 max_context_window；
未知容量保持未知。旧 receipt 缺少该字段仍可解码为未知，不能读取当前模型目录补写历史容量，也
不能把输出 token 上限或累计用量当成上下文容量。

完成请求的输入使用 provider 无关的有序上下文条目，包括普通消息与专用压缩条目
（Compaction，携带 encryptedContent）；压缩条目可以映射为 Responses 原生输入，Chat
Completions 必须明确拒绝。运行时以"远程压缩能力对象"暴露实际支持的能力：接收有序上下文并
返回保真上下文与最终 accounting；能力由 endpoint 显式声明，不能从 Responses 协议推断，网络
执行复用共享执行器并固定使用 HTTP。

## 6.8 模型可调参数（effort 机制）

effort（推理强度）不是固定的全局枚举，而是"模型声明的可调参数"。该机制是通用的：effort 是
首个应用，类型设计可容纳未来 thinking、verbosity 等参数。各供应商自由定义候选值域，并由模型
自身声明选中值如何写入 API 请求体；协议层据此通用透传，不包含任何供应商特定代码。

模型目录独占参数的候选值、显示名、默认候选与 wire 规则；角色路由只保存当前选择，不得复制或
重新定义候选。模型声明非空 effort 候选时，产品角色必须保存其中一个候选；模型没有声明 effort
参数时，角色选择必须为空，完成请求不携带 effort，最终请求体也不得制造默认字符串或字段。当前
选择从角色路由进入统一 reasoning 配置，Responses 与 Chat Completions 均只由 parameter wire
写入供应商请求体。

每个模型参数声明包含：参数名（effort 的 name 为 `effort`）、可选显示名（缺失回退参数名）、
候选值域（固定按推理强度从弱到强排列）与 wire 规则表（每个候选值 → 写入规则）。写入规则由
set 列表（嵌套 dot 路径 + 透传字符串值，如 `reasoning.effort`、`thinking.type`）与 remove
列表（dot 路径）组成；应用时按 dot 路径逐层写入或移除嵌套 JSON 对象字段，移除不存在的字段
静默忽略。wire 规则表使用按候选值索引的静态结构而非动态 JSON 值，保持配置友好且无需运行时
反序列化。目录为 effort 提供便捷查询（参数声明、候选列表、默认候选），复用于配置校验、默认
角色补齐和 GUI 渲染。

各供应商的 effort 声明形态（本文为唯一权威源）：

| 供应商 | candidates | set（选中值 → 字段） | remove |
| --- | --- | --- | --- |
| OpenAI（GPT-5.5） | `low` / `medium` / `high` / `xhigh` | `reasoning.effort` = 值 | — |
| OpenAI（GPT-6 Astra / GPT-5.6 Sol / Terra / Luna） | `low` / `medium` / `high` / `xhigh` / `max` | `reasoning.effort` = 值 | — |
| DeepSeek | `low` / `high` / `max` | `reasoning_effort` = 值（`thinking.type = enabled` 作为 base body） | — |
| 智谱普通 | `none` / `enabled` | `thinking.type` = 值 | — |
| GLM-5.2 | `none` / `high` / `max` | `high`/`max`：`reasoning_effort` + `thinking.type = enabled` + `thinking.clear_thinking = false`；`none`：`thinking.type = disabled` | `none` 移除 `reasoning_effort` |
| GLM-5.3 / GLM-5.3-Flash | `low` / `high` / `max` | 三档均为 `reasoning_effort` + `thinking.type = enabled` + `thinking.clear_thinking = false` | — |
| MiMo | `disabled` / `enabled` | `thinking.type` = 值 | — |

GLM-5.2 的"一个选择联动多个字段"和"none 时移除字段"由 wire 的多条 set 与 remove 完整表达，
无需协议层特判。GLM-5.3 系列始终启用思考，不提供禁用思考的 `none` 候选；effort 选择只改变
`reasoning_effort` 值。

所有模型的候选必须按思考强度从弱到强声明，因此首项是该模型可用的最弱强度。内部摘要类请求
（例如 Thread 自动命名）沿用 Explorer 路由的模型，并选择该数组首项，不复制 provider/model 或
重新定义强度枚举。

## 6.9 模型家族预设

同供应商的模型共享大量元数据（capabilities、truncation policy、effort 参数声明、base body）。
内置目录不为每个模型独立构造完整模型信息，而是用模型家族（ModelFamily）预设封装共享部分，具体
模型仅以差异字段实例化。家族不承担请求生命周期或费用计算；模型计价（ModelPricing）独立表达
未知价格或包含长度分档、时段倍率及来源的费率定义，具体结构以公开 Rust 类型为准。

内建家族预设按供应商与模型线划分（OpenAI 各线、DeepSeek 主线与 Flash 线、MiMo、智谱文本与
各 GLM 线、智谱 vision），共享能力矩阵由各供应商能力构造复用；家族之间的差异集中在 effort
候选值域、request profile 与 typed input capabilities。DeepSeek V4.1 Flash 使用经过官方文档
确认的 Responses image profile，V4 Pro 只声明 text；两者共享 effort、thinking、上下文和
Responses HTTP 规则，但计费独立保存在各自模型实例。GLM-5.3 与 GLM-5.2 复用同一条"启用思考"
wire 组合，差异只在候选值域：GLM-5.3 为 `high` / `low` / `max`，且不提供禁用思考候选；
GLM-5.3-Flash 复用 GLM-5.3 的始终思考 wire 与候选值域，并声明 image 的 local/data-url 与
remote-url/snapshot 路线，不得从相邻视觉模型推断 video/file 能力。

## 6.10 Prompt 缓存

固定 instructions 与 prelude 包含模型基础指令、平台与全局配置、模式与角色、稳定 Skill 目录和
Workspace/项目文档。每轮 Skill 调用与推荐写入该轮 canonical transcript，不进入旧历史之前的固定
前缀。工具定义由每步冻结的工具计划提供；相同模型可见定义保持确定性编码。

模型可见 working context 在内容变化时以隐藏的 user-role 运行事实完整快照追加到 transcript；
相同内容不追加，清空时明确声明旧快照失效。快照使用稳定 section ID 排序，不提升为 system 权限；
同一 Turn 更新也只追加新快照，不覆盖此前模型已观察的内容。压缩移除最新快照后重新补入当前值；
请求从已记录的历史读取，冷恢复不重新渲染旧快照。Evidence Ledger 和 session note 的完整正文
继续不进入模型上下文；typed working state 与模型可见投影各自保留。working context 的变化只
更新 context hash，不因追加记录提升固定前缀 generation；provider、model、固定指令或工具
schema 的变化仍单独记录（完整依赖倒置与 prepared-call 契约见 [16](./16-core-contracts.md)）。

上下文压缩采用 Codex 风格的版本化 replacement：采样前估算完整物化请求，达到 90% 自动阈值时
replace transcript，再把当前 working context 注入新窗口一次；provider 报告 token 达到阈值时，
下一次采样前执行同样 replacement。压缩不得丢失 tool call/output 配对或当前用户任务。

每个指令层分别计算内容 hash；基础、模式角色、Skill、可见工具组说明、Workspace、wire 工具
前缀、provider、model 或 compaction 变化都给出类型化的前缀变化原因并提升 generation。工具与
缓存的关系只由工具计划的 wire fingerprint 表达：它是实际发送的完整工具规格列表的 canonical
哈希——工具按模型可见名称排序，JSON Schema 递归使用确定性字段顺序；registry revision、group
identity、注册顺序和 executor generation 不参与 wire。计划只在单次 model step/retry 内冻结，
不能为了复用缓存跨 Thread、worktree 或 agent 工具集共享。

DeepSeek 使用隐式共同前缀，不发送 `prompt_cache_key`、breakpoint 或 OpenAI options。工具层
不生成、轮换或参与 provider cache key；宿主显式提供的 session-stable key 仍可由支持的模型
调用透传，但不得由 tool revision 或 wire fingerprint 派生。cache key 只是路由提示，不能代替
请求前缀相等。

缓存请求控制、服务端用量和价格是三个独立契约：只有服务端报告的缓存读取、写入和 reasoning
才能作为计量事实；缺失不是零，不截断异常计数，不按缓存请求策略推导写入费用。OpenAI 的普通
输入、缓存读和缓存写按官方语义互斥计费，DeepSeek 按输入命中/未命中计费；具体单价来自模型
目录，不在 core 中保留供应商规则常量。

缓存诊断只记录 generation、固定前缀/wire 工具前缀/working context 的 hash、工具 wire
fingerprint、token 数和变化原因；不得记录 prompt、工具参数或结果、header、凭据和配置正文。
提示词诊断记录完整 tool schema 的估算 token、Programmatic program 数与嵌套调用数；Responses
transport 记录 continuation attempted/used/invalid、full replay retry 和 HTTP fallback 的稳定
原因；compaction 记录替换前后估算 token。这些计数附着于对应 inference 或 Turn，不能以无法
关联的独立日志代替，也不得记录 program 正文。

## 6.11 Web 搜索 Provider 边界

Web 搜索同时维护 OpenAI 与 DeepSeek 两份独立 resolution，再按当前 route 仲裁。OpenAI 路径保留
standalone `/alpha/search` 与 Responses hosted search；DeepSeek 原生搜索只允许当前 route 自身
满足：endpoint 有凭据、模型使用 Responses transport、模型声明 web_search 能力，且 provider
服务能力声明 DeepSeek Responses 方言。DeepSeek 不跨 provider 借用；不满足或配置关闭时才允许
现有 OpenAI resolution 成为回退。

Responses 原生搜索统一通过携带 hosted dialect 的 WebSearch 工具规格表达。OpenAI dialect 可
发送 external/indexed access、context size、允许域名与近似位置；DeepSeek dialect 严格只序列化
`{"type":"web_search"}`，不得伪装支持官方未承诺的过滤、位置、上下文或 cached/indexed 语义；
tool choice 保持 `auto`。DeepSeek hosted search 是 additive 工具，必须与普通函数、MCP、LSP、
文件和命令工具共存；旧 OpenAI hosted-only 路径仍可按其约束进入 exclusive 模式。原生搜索参数
使用封闭变体：DeepSeek 无附加参数，OpenAI 拥有其实际支持的访问模式、过滤、位置与上下文设置；
构造 DeepSeek 搜索不需要填写 OpenAI 空字段，adapter 也不通过逐字段丢弃这些设置来模拟协议兼容。

Provider 服务能力同时包含 hosted_responses 与 hosted_dialect。内置 DeepSeek preset 使用
DeepSeekResponses 方言，OpenAI preset 与旧显式配置默认使用 OpenAiResponses 方言。preset 实例
覆盖非 canonical base_url 时不得继承 hosted search 或其他 Responses hosted 能力；显式
capability 仍可由用户重新声明。provider catalog schema 暴露 dialect，产品层不得从 provider id
或 URL 猜测。

DeepSeek `/responses` 返回的 `web_search_call` 与 OpenAI Responses 共用 canonical SSE
decoder、timeline 和历史回放：searching/completed 生命周期、search/open/find action 都投影为
统一事件；完整 native item（包括未知字段和 opaque results）作为 Responses context 持久化，
并在下一轮按原始 JSON 顺序回放。provider adapter 不自行注入未进入本轮冻结工具计划的 hosted
tool。

## 6.12 计量与价格

单次模型调用的 canonical 计量结果（InferenceAccounting）包含用量报告、完整性、计价状态和
价格快照。成功、失败、截断和取消均保留已收到的服务端报告，未报告保持未知；Chat 必须消费末尾
独立 usage 包后才终结，Responses、Chat、WS 和压缩采用一致语义；重复终态不重复记账。

模型计价（ModelPricing）声明币种、输入/输出长度分档与可选的每周时段倍率。调用开始冻结价表
与计价开关，按产生最终结果的请求发送时间选择时段，按最终用量选择长度档位；跨时段不拆分
token。这是本地模型 token 费用估算，不是供应商账单，不包含独立工具费、订阅费或汇率换算。
关闭计价、价格/用量不足、已估算与零费用分别表示；历史只读取冻结账单，不按现价重算。Core、
Studio、Flutter 仅归属、累计和展示，不再次解释供应商 usage 或价格。具体单价、时段定义与
退役模型以代码中的模型目录为唯一事实源，设计文档不保存价格快照。

普通 API 预设默认计价；Coding Plan、Token Plan 和自定义兼容预设默认不计价，用户选择只影响
之后的调用。账户余额和套餐配额保持独立查询。

独立 core 工具任务通过 Turn 结果的 billing 字段返回本轮每次推理与压缩的冻结账单；actor 使用
同一账单提交持久状态。取消仅提交已取得的计量事实，不推进已取消上下文；预算到期前已收到的
响应必须先记账；缺失或无效用量不能覆盖此前已知的上下文用量。自动标题等没有会话上下文的内部
推理，将完整冻结账单保存在所属 root Thread 的内部账单中，并与费用累计一次性提交；内部调用
不覆盖主会话上下文用量，重载只读取原账单和原累计。
