# 05 - 扩展点

## 5.1 Provider 扩展

Provider 扩展分为协议、preset 和实例三层：

- 新 wire 协议才扩展 `pl-model` 的 typed request/stream adapter。
- 使用现有 Responses 或 Chat Completions 的供应商只在 `pl-core::ProviderCatalogRegistry`
  增加 preset，并把模型元数据放入 `pl-model` canonical catalog。
- 产品配置只保存 preset/catalog 引用、实例凭证、endpoint override、连接模式和附加模型。
- 完全自定义 provider 使用 `ProviderTransportSelection::Custom` 与 `Explicit` 模型目录。

Provider 提供的外部服务能力与 wire 协议正交。`ProviderPreset` 通过
`ProviderServiceCapabilities` 声明 hosted Web Search、standalone Web Search 等默认能力；preset
实例默认保存 `ProviderCapabilitySelection::PresetDefaults`，因而重新编译后可获得新增能力。完全
自定义 provider 默认保存显式空能力，也可在高级配置中声明兼容能力。运行期只能消费解析后的能力，
不得根据 provider id、preset id、模型 slug 或 endpoint 猜测能力。

Responses-compatible 与 Chat-compatible 都是一等公共协议能力，不为 MiMo、DeepSeek、Zhipu
等厂商增加执行枚举或 runtime struct。provider 私有 request/stream 结构不得泄漏到 `pl-core`；
兼容 API 的字段差异通过 `ModelRequestProfile`、模型参数 wire 和 tool wire policy 数据化表达。

公共消息、事件和错误类型继续来自 `pl-protocol`。

Bundled catalog 只读，`additional_models` 只能追加不冲突 slug；完全自定义目录使用
`Explicit`。`ProviderConfig::effective_models()` 是唯一合并入口。

Web Search 的候选发现、优先级和模型门控统一由 `pl-core::plan_web_search()` 完成。产品只提供
`AgentModelConfig`、当前 `ResolvedModelRoute` 和用户配置，再把 `WebSearchPlan` 安装到
`TurnEngine`；Studio、Mai 和后续宿主不得各自实现 resolver。

MCP 执行环境通过 `McpRuntimeHost` 扩展。PL 拥有配置 fingerprint、增量 reconcile、工具发现、
命名冲突、健康状态和 generation 生命周期；产品 Host 只连接具体 session。Studio 使用
`LocalMcpRuntimeHost`，容器产品实现自己的 container Host。turn 只持有固定 generation 的
`McpTurnLease`，配置更新不得改变正在执行 turn 的 schema 或 backend。

## 5.2 核心流程扩展

需要影响 turn、session、store 或编译阶段时扩展 `pl-core`。

上下文压缩的编排属于 `pl-core` 扩展点：turn pipeline 负责自动/手动触发、pre-turn/mid-turn/standalone phase、原子替换 `AgentSession` 有序上下文项，并由宿主同步持久化。`pl-model::ModelProvider::compact_context` 只暴露统一压缩请求/响应，provider runtime 内部封装私有 wire。远程压缩能力由 `ProviderWireProtocol::Responses` 与显式 compaction 配置共同决定，不依赖 preset 或厂商 ID；Chat Completions 始终使用本地摘要。

OpenAI 远程模式默认使用 v2 `compaction_trigger`，`/responses/compact` 只作为显式 legacy 兼容模式，不做运行期自动回退。扩展压缩 wire 时必须保持 `ModelContextItem::Compaction` 的 provider 无关边界，不得把加密 checkpoint 伪装成普通 system/user 消息，也不得让 Chat Completions 消费该项。

扩展时保持入口层薄：

- UI 输入只在 `pure-studio` 中收集。
- 进入核心层前转换为明确 enum 或 options struct。
- 避免把 bool 参数暴露到核心 API。

需要影响 provider/model 路由的通用值对象与校验时扩展 `pl-core::model_config`；产品角色、
默认值、schema version、路径与文件持久化分别扩展 `pl-studio-runtime` 或其他宿主。

## 5.3 前端扩展

`pure-studio` 是当前桌面前端。后续可以增加 CLI、Web 或 IDE 前端；产品前端调用
自己的宿主 runtime，宿主再接入 `pl-core` agent 框架。

`pure-studio` 使用 Flutter Windows 桌面应用，UI 使用 Material 3、Riverpod 和
flutter_rust_bridge。桌面端状态和配置均由 `pl-studio-runtime` 持久化。

## 5.4 执行能力扩展

命令执行、文件编辑、工具系统和沙箱能力必须以独立策略接入，并通过权限模型和事件流暴露给核心流程。

桌面端允许注册 `exec`、完整 agent 协作工具和文件工具。当前 Studio 运行路径默认使用 `PermissionMode::RequestApproval`：workspace 内访问按工具策略直接放行，workspace 外访问请求用户审批；`auto-review` 会把 workspace 外访问交给 reviewer，`full-access` 在策略层放行已暴露工具。旧 `ToolApprovalPolicy::Manual` 和 `DenyAll` 只作为兼容构造保留；审批和交互结果通过统一 `Interaction` 与 Studio event/projection 记录，拒绝时将拒绝原因作为 tool result 写回会话。

文件工具作为 `pl-core` 工具系统的一部分注册，当前不新增独立 `pl-tool` crate。文件工具包括读取、写入、列目录、搜索、stat、建目录、删除、复制、移动和 `apply_patch`。工具 schema 不强制模型提供绝对路径；workspace-relative 路径按 `workspaceRoot` 解析，执行层统一转换为规范化绝对路径后再校验、审批和执行。只读工具仍受工作区路径边界限制；修改工具进入现有工具审批流程。

容器文件 backend 以容器本身作为隔离边界，`cwd` 只负责解析相对路径，不充当第二个文件系统沙箱。绝对容器路径按其自身语义解析，即使同时传入了不同目录的 `cwd` 也不得被误判为“逃逸 cwd”；相对路径中的 `..` 仍不能越过声明的绝对 `cwd`。`stat`、读取、写入、删除、列目录和搜索必须采用一致的绝对路径语义，避免前置检查成功而复制阶段拒绝同一路径。宿主本地文件 backend 仍按 workspace 与权限模式执行原有边界校验，不因容器语义而放宽。

`stat_path` 同时承担安全的存在性探测：目标存在时返回 `exists: true` 和元数据；目标不存在但最近存在父目录仍可在当前路径权限下安全解析时，返回成功结果 `exists: false`，不得把常规的“不存在”记录成工具失败。绝对路径、父目录跳转、符号链接或其他 workspace 越界仍按统一路径策略拒绝，不能因为存在性探测而放宽边界。

`list_files` 对 workspace 内尚不存在的目录返回成功的空列表，供 planner 在首次创建 `design/**` 前安全探测；本地与容器 backend 必须保持相同语义。缺失路径之外的读取错误仍显式失败，workspace 越界规则不变。

文件工具输入 schema 使用明确 enum 表示危险语义。`delete_path` 的删除模式是 `mode: "file" | "emptyDirectory" | "recursiveDirectory"`；`copy_path` / `move_path` 的目标冲突策略是 `collision: "failIfExists" | "overwrite"`。旧 bool 字段 `recursive` 和 `overwrite` 的运行期兼容读取路径已删除，工具 schema 只暴露 `mode` / `collision`，历史会话或手写输入若仍使用旧字段会被校验拒绝。

`exec` 是模型可调用的统一命令入口，schema 包含 `command`、`cwd`、`timeoutSeconds`、`yieldTimeMs` 和 `maxOutputChars`。PL 的通用命令管理器负责短命令、后台 `processId`、stdin、超时、取消、输出截断与 turn 清理；`write_stdin` 只能操作已经由 `exec` 启动且通过审批的 live process，不重新触发命令审批。本地 backend 在 Windows 上依次使用 PowerShell Core、Windows PowerShell 和 `cmd.exe`，Unix 上使用 `sh -c`；宿主可以注入容器或远程 backend，而不改变模型 schema 和结果。`cwd` 缺省为 Agent workspace，相对路径按该 workspace 解析，backend 必须拒绝越过自己的隔离边界。完整 stdout/stderr 写入模型可通过文件工具读取的 `target/pure/<session>/<tool>/output.log`，上下文只回传截断输出、状态、退出码、超时标记、输出文件路径和恢复提示。容器 backend 还负责将完整流投影为宿主 artifact，并在超时、取消或 Drop 时同时清理 transport 与容器内进程组。

`apply_patch` 的唯一正式输入协议是 Codex 风格 freeform patch：外层使用 `*** Begin Patch` / `*** End Patch`，每个文件操作必须以 `*** Add File:`、`*** Delete File:` 或 `*** Update File:` 开头。OpenAI Responses 和支持 custom tools 的 Chat Completions provider 使用 `type: "custom"` 工具，并通过 Lark grammar 描述 patch 格式。不支持 custom/freeform 的 Chat-compatible provider 使用普通 function fallback，将完整 patch 放入 `patch` 字段；工具描述和 function fallback schema 必须给出一个最小有效 `*** Update File:` 示例，帮助真实模型生成可执行格式。执行层复用同一个 parser、路径 resolver 和 apply 流程，按 hunk 出现顺序提交文件变更；如果前面的 hunk 已成功写入而后续 hunk 失败，已提交前缀保留，并在错误输出中报告 committed delta。路径解析、workspace 边界、符号链接写保护和 workspace 写锁仍由 Studio 文件工具层强制执行，不因对齐 Codex patch 语义而放宽。解析层接受 Codex 的宽容输入：可选 `*** Environment ID:` preamble、单 patch block 外部的 markdown fence 或前后说明文字、`<<EOF` / `<<'EOF'` / `<<"EOF"` heredoc wrapper，以及不以 hunk/control marker 开头的裸上下文行；`---/+++` unified diff、`*** File:` 元数据头和自然语言编辑指令不属于 `apply_patch` 输入格式，应返回可纠正的错误提示。上下文匹配采用 Codex 风格的逐级宽容策略：先精确匹配，再忽略尾随空白，再忽略首尾空白，最后对常见 Unicode 标点和空白做 ASCII 归一化后匹配；带 `*** End of File` 的 chunk 先尝试文件末尾匹配，找不到时再回退到普通顺序搜索，避免模型误用 EOF marker 导致可定位上下文失败。当模型把同一个缩进上下文行同时写在新增块前后时，执行层可把它解释为围绕该上下文的纯插入，避免因为 patch 控制空格与文件缩进混淆而失败。没有 old lines 的纯新增 update chunk 按 Codex 语义追加到文件末尾；`*** Add File:` 可以覆盖已存在文件并可创建空文件，带 `*** Move to:` 的更新可以覆盖目标文件；`*** Delete File:` 仍要求目标存在且不是目录。`apply_patch` 解析或上下文匹配失败时，工具结果必须保留原始失败原因，并提示后续模型先重新读取目标文件、用当前内容生成更小的 patch 后重试，不应重复提交同一个失败 patch。为了避免同一进程内 agent、subagent 或同轮工具调用同时写 workspace，修改类文件工具共享进程内 workspace 写锁；该锁不承诺跨进程或外部编辑器互斥。

JSON/ARB 属性行只有在 patch 的 old/new 两侧保持不变时才属于保留型上下文；此时当前文件中同一属性键的值即使已经变化，也可以按键匹配并保留当前整行。新增、删除或修改值的属性行不得使用这一宽容规则。这样可以容忍翻译或生成值漂移，同时不会覆盖并发修改。

工具调用历史必须保留调用种类。`function_call` 的历史回放写回 `function_call_output`；custom/freeform 工具写回 `custom_tool_call_output`。不得只保存 JSON arguments 后在下一轮统一当作 function tool 回放。

Skills 工具同样挂在 `pl-core` 默认工具集中。`skills_list` 和 `skill_view` 是只读工具；`skill_manage` 是写入工具，但只能修改当前项目的 `<workspace_root>/skills/`，不能修改用户级、系统或外部只读 skills。subagent 通过同一默认工具注册入口继承 skills 能力。

协作工具通过 `spawn_agent`、`send_input`、`list_agents` 和 `close_agent` 暴露，并只持有
非泛型 `AgentRuntimeHandle`。未关闭 agent 可连续接收输入，不存在 `resume_agent`。输入使用
`QueueOnly | Start | InterruptThenStart` 明确表达；角色、目标选择和工具 effect 均来自产品
编译出的 `AgentExecutionPolicy`。等待由 runtime 的 direct-child `AgentEventHub` 订阅、
`WaitingAgents` 状态机和独立 inactivity timer 管理，不向模型暴露轮询工具。Studio 的 agent
展示继续以 durable snapshot 和 append-only timeline 为准。

通用协作工具不承载 Studio 的任务分配协议。Task harness 另外注册
`task_spawn_executor { taskName, message, ownedPaths }` 与 `task_request_review`，把强类型
输入转换为内部 spawn intent 后调用同一个 `AgentRuntimeHandle`。Task 根的通用
`spawn_agent` 只公开 explorer，避免模型通过自由 metadata 绕过 worktree、路径所有权和审查
授权。
