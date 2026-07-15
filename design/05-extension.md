# 05 - 扩展点

## 5.1 Provider 扩展

新增模型 provider 时优先扩展 `pl-model`：

- 新增明确的 provider 类型、`ProviderInfo` 构造或配置来源。
- 在 `~/.pure/config.toml` 中新增 provider 和完整 models 列表。
- 实现或复用 `ModelProvider`。
- 适配目标 API 的 request/response wire 格式。

OpenAI-compatible 不是一等公共 provider 抽象。新增供应商必须显式建模为 provider，并通过 `protocol::openai` 或未来其他 protocol 复用底层 API 协议。provider 私有 request/stream 结构不得泄漏到 `pl-core`；如果目标 API 兼容 Chat Completions 或 Responses 但增加了私有字段，应新增本地强类型扩展并继续通过 `async-openai` client/stream 发送。

公共消息、事件和错误类型继续来自 `pl-protocol`。

配置里的模型信息会覆盖或补充 bundled model，使用户可以接入自定义模型。

## 5.2 核心流程扩展

需要影响 turn、session、store 或编译阶段时扩展 `pl-core`。

上下文压缩的编排属于 `pl-core` 扩展点：turn pipeline 负责自动/手动触发、pre-turn/mid-turn/standalone phase、原子替换 `CoreSession` 有序上下文项，并在 Studio store 中同步持久化。`pl-model::ModelProvider::compact_context` 只暴露统一压缩请求/响应，provider runtime 内部封装私有 wire。远程压缩能力严格限于 `ProviderKind::OpenAi`；新增 provider 即使复用 OpenAI 协议，也默认且强制走本地摘要，除非未来通过新的设计变更明确提升为一等远程能力。

OpenAI 远程模式默认使用 v2 `compaction_trigger`，`/responses/compact` 只作为显式 legacy 兼容模式，不做运行期自动回退。扩展压缩 wire 时必须保持 `ModelContextItem::Compaction` 的 provider 无关边界，不得把加密 checkpoint 伪装成普通 system/user 消息，也不得让 Chat Completions 消费该项。

扩展时保持入口层薄：

- UI 输入只在 `pure-studio-flutter` 中收集。
- 进入核心层前转换为明确 enum 或 options struct。
- 避免把 bool 参数暴露到核心 API。

需要影响角色、provider/model 路由或配置持久化时扩展 `pl-core` 的配置模块。

## 5.3 前端扩展

`pure-studio-flutter` 是当前桌面前端。后续可以增加 CLI、Web 或 IDE 前端，但都应调用 `pl-core`，并复用 `pl-protocol` 的事件和消息类型。

`pure-studio-flutter` 使用 Flutter Windows 桌面应用，UI 使用 Material 3、Riverpod 和 flutter_rust_bridge。桌面端状态通过 `pl-core::StudioStore` 纯异步写入 SQLite，业务配置仍通过 `pl-core::ConfigStore` 读写 `~/.pure/config.toml`。

## 5.4 执行能力扩展

命令执行、文件编辑、工具系统和沙箱能力必须以独立策略接入，并通过权限模型和事件流暴露给核心流程。

桌面端允许注册 `bash`、完整 agent 协作工具和文件工具。当前 Studio 运行路径默认使用 `PermissionMode::RequestApproval`：workspace 内访问按工具策略直接放行，workspace 外访问请求用户审批；`auto-review` 会把 workspace 外访问交给 reviewer，`full-access` 在策略层放行已暴露工具。旧 `ToolApprovalPolicy::Manual` 和 `DenyAll` 只作为兼容构造保留；审批和交互结果通过统一 `Interaction` 与 Studio event/projection 记录，拒绝时将拒绝原因作为 tool result 写回会话。

文件工具作为 `pl-core` 工具系统的一部分注册，当前不新增独立 `pl-tool` crate。文件工具包括读取、写入、列目录、搜索、stat、建目录、删除、复制、移动和 `apply_patch`。工具 schema 不强制模型提供绝对路径；workspace-relative 路径按 `workspaceRoot` 解析，执行层统一转换为规范化绝对路径后再校验、审批和执行。只读工具仍受工作区路径边界限制；修改工具进入现有工具审批流程。

`stat_path` 同时承担安全的存在性探测：目标存在时返回 `exists: true` 和元数据；目标不存在但最近存在父目录仍可在当前路径权限下安全解析时，返回成功结果 `exists: false`，不得把常规的“不存在”记录成工具失败。绝对路径、父目录跳转、符号链接或其他 workspace 越界仍按统一路径策略拒绝，不能因为存在性探测而放宽边界。

`list_files` 对 workspace 内尚不存在的目录返回成功的空列表，供 planner 在首次创建 `design/**` 前安全探测；本地与容器 backend 必须保持相同语义。缺失路径之外的读取错误仍显式失败，workspace 越界规则不变。

文件工具输入 schema 使用明确 enum 表示危险语义。`delete_path` 的删除模式是 `mode: "file" | "emptyDirectory" | "recursiveDirectory"`；`copy_path` / `move_path` 的目标冲突策略是 `collision: "failIfExists" | "overwrite"`。旧 bool 字段 `recursive` 和 `overwrite` 的运行期兼容读取路径已删除，工具 schema 只暴露 `mode` / `collision`，历史会话或手写输入若仍使用旧字段会被校验拒绝。

`bash` 是模型可调用的命令执行入口。它保留 `command`、`workingDirectory` 和 `timeoutSeconds` 参数，并支持 `yieldTimeMs` 与 `maxOutputChars` 控制首次等待时长和模型上下文中的输出预算。Windows 上执行层优先使用 PowerShell Core (`pwsh.exe`)，找不到时回退到 Windows PowerShell (`powershell.exe`)，最后才回退到 `cmd.exe`；PowerShell 调用会注入 UTF-8 输出前缀以稳定捕获中文和 Unicode 输出。Unix 上仍使用 `sh -c`。`workingDirectory` 与文件工具复用同一套路径策略：缺省为 `workspaceRoot`，相对路径按 `workspaceRoot` 解析，workspace-only 模式拒绝逃逸，`full-access` 模式允许解析到 workspace 外的已存在目录。短命令在当前工具调用内返回完成状态；超过 `yieldTimeMs` 仍未退出的命令进入后台运行，工具结果返回 `running` 状态和 `processId`。后台进程由 `pl-core` 内的进程管理器持有，并通过配套 `write_stdin` 工具继续发送 stdin、等待新输出或轮询最终状态；`write_stdin` 只能操作已经由 `bash` 启动且通过审批的 live process，不重新触发命令审批。完整 stdout/stderr 始终写入 `target/pure/<session>/<tool>/output.log`，模型上下文只回传截断后的 stdout/stderr、状态、退出码、超时标记、输出文件路径和恢复提示。命令超时、用户中断、turn 清理或运行时 drop 时，执行层应尽力终止仍存活的子进程；后台进程数量受固定上限保护，超过上限时返回可恢复错误，提示模型等待现有进程结束或轮询已有 `processId`。

`apply_patch` 的唯一正式输入协议是 Codex 风格 freeform patch：外层使用 `*** Begin Patch` / `*** End Patch`，每个文件操作必须以 `*** Add File:`、`*** Delete File:` 或 `*** Update File:` 开头。OpenAI Responses 和支持 custom tools 的 Chat Completions provider 使用 `type: "custom"` 工具，并通过 Lark grammar 描述 patch 格式。不支持 custom/freeform 的 Chat-compatible provider 使用普通 function fallback，将完整 patch 放入 `patch` 字段；工具描述和 function fallback schema 必须给出一个最小有效 `*** Update File:` 示例，帮助真实模型生成可执行格式。执行层复用同一个 parser、路径 resolver 和 apply 流程，按 hunk 出现顺序提交文件变更；如果前面的 hunk 已成功写入而后续 hunk 失败，已提交前缀保留，并在错误输出中报告 committed delta。路径解析、workspace 边界、符号链接写保护和 workspace 写锁仍由 Studio 文件工具层强制执行，不因对齐 Codex patch 语义而放宽。解析层接受 Codex 的宽容输入：可选 `*** Environment ID:` preamble、单 patch block 外部的 markdown fence 或前后说明文字、`<<EOF` / `<<'EOF'` / `<<"EOF"` heredoc wrapper，以及不以 hunk/control marker 开头的裸上下文行；`---/+++` unified diff、`*** File:` 元数据头和自然语言编辑指令不属于 `apply_patch` 输入格式，应返回可纠正的错误提示。上下文匹配采用 Codex 风格的逐级宽容策略：先精确匹配，再忽略尾随空白，再忽略首尾空白，最后对常见 Unicode 标点和空白做 ASCII 归一化后匹配；带 `*** End of File` 的 chunk 先尝试文件末尾匹配，找不到时再回退到普通顺序搜索，避免模型误用 EOF marker 导致可定位上下文失败。当模型把同一个缩进上下文行同时写在新增块前后时，执行层可把它解释为围绕该上下文的纯插入，避免因为 patch 控制空格与文件缩进混淆而失败。没有 old lines 的纯新增 update chunk 按 Codex 语义追加到文件末尾；`*** Add File:` 可以覆盖已存在文件并可创建空文件，带 `*** Move to:` 的更新可以覆盖目标文件；`*** Delete File:` 仍要求目标存在且不是目录。`apply_patch` 解析或上下文匹配失败时，工具结果必须保留原始失败原因，并提示后续模型先重新读取目标文件、用当前内容生成更小的 patch 后重试，不应重复提交同一个失败 patch。为了避免同一进程内 agent、subagent 或同轮工具调用同时写 workspace，修改类文件工具共享进程内 workspace 写锁；该锁不承诺跨进程或外部编辑器互斥。

工具调用历史必须保留调用种类。`function_call` 的历史回放写回 `function_call_output`；custom/freeform 工具写回 `custom_tool_call_output`。不得只保存 JSON arguments 后在下一轮统一当作 function tool 回放。

Skills 工具同样挂在 `pl-core` 默认工具集中。`skills_list` 和 `skill_view` 是只读工具；`skill_manage` 是写入工具，但只能修改当前项目的 `<workspace_root>/skills/`，不能修改用户级、系统或外部只读 skills。subagent 通过同一默认工具注册入口继承 skills 能力。

Subagent 工具通过 `spawn_agent`、`send_input`、`wait_agent`、`list_agents`、`close_agent` 和 `resume_agent` 暴露。`spawn_agent.forkTurns` 可取 `none`、`all` 或正整数字符串；默认 `none`，表示子代理只接收初始任务消息。显式请求 `all` 或 `N` 时，运行时从父会话构造过滤后的历史快照：保留 system/user 消息和 assistant 最终文本，丢弃工具调用、工具结果、reasoning 内容和运行时提示，避免把大输出复制给子代理。`send_input.triggerTurn` 表达继续执行，`send_input.interrupt` 表达立即打断并重定向。`wait_agent` 只返回 `{ message, timedOut }` 的活动等待结果；状态明细、摘要、错误和预算信息通过 `list_agents` 读取当前 compact snapshot。Studio 的 agent 展示继续以 `AgentChanged` latest snapshot 和 `SubAgentActivity` append-only timeline 为准，不依赖模型上下文中的详细 tool result。
