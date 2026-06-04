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

上下文压缩属于 `pl-core` 扩展点：模型层只暴露窗口、阈值和 provider 调用能力，turn pipeline 负责判断触发、生成摘要、替换 `CoreSession` 历史，并在 Studio store 中同步持久化改写后的消息。

扩展时保持入口层薄：

- UI 输入只在 `pure-studio` 中收集。
- 进入核心层前转换为明确 enum 或 options struct。
- 避免把 bool 参数暴露到核心 API。

需要影响角色、provider/model 路由或配置持久化时扩展 `pl-core` 的配置模块。

## 5.3 前端扩展

`pure-studio` 是当前前端。后续可以增加 CLI、Web 或 IDE 前端，但都应调用 `pl-core`，并复用 `pl-protocol` 的事件和消息类型。

`pure-studio` 使用 Tauri 2 实现跨平台桌面应用，UI 使用 React、Vite 和 TypeScript。桌面端状态通过 `pl-core::StudioStore` 纯异步写入 SQLite，业务配置仍通过 `pl-core::ConfigStore` 读写 `~/.pure/config.toml`。

## 5.4 执行能力扩展

命令执行、文件编辑、工具系统和沙箱能力必须以独立策略接入，并通过权限模型和事件流暴露给核心流程。

桌面端允许注册 `bash`、完整 agent 协作工具和文件工具。当前 Studio 运行路径默认使用 `AutoAllow` 直接执行已注册工具；切换到 `Manual` 时，审批结果通过 `AgentEvent` 和 Studio SeaORM 状态记录，拒绝时将拒绝原因作为 tool result 写回会话。

文件工具作为 `pl-core` 工具系统的一部分注册，当前不新增独立 `pl-tool` crate。文件工具包括读取、写入、列目录、搜索、stat、建目录、删除、复制、移动和 `apply_patch`。只读工具仍受工作区路径边界限制；修改工具进入现有工具审批流程。

`apply_patch` 的唯一正式输入协议是 Codex 风格 freeform patch：外层使用 `*** Begin Patch` / `*** End Patch`，每个文件操作必须以 `*** Add File:`、`*** Delete File:` 或 `*** Update File:` 开头。OpenAI Responses 和支持 custom tools 的 Chat Completions provider 使用 `type: "custom"` 工具，并通过 Lark grammar 描述 patch 格式。不支持 custom/freeform 的 Chat-compatible provider 使用普通 function fallback，将完整 patch 放入 `patch` 字段；工具描述和 function fallback schema 必须给出一个最小有效 `*** Update File:` 示例，帮助真实模型生成可执行格式。执行层复用同一个 parser 和 apply 流程，按 hunk 出现顺序提交文件变更；如果前面的 hunk 已成功写入而后续 hunk 失败，已提交前缀保留，并在错误输出中报告 committed delta。路径解析、workspace 边界、符号链接写保护和 workspace 写锁仍由 Studio 文件工具层强制执行，不因对齐 Codex patch 语义而放宽。解析层接受 Codex 的宽容输入：可选 `*** Environment ID:` preamble、单 patch block 外部的 markdown fence 或前后说明文字、`<<EOF` / `<<'EOF'` / `<<"EOF"` heredoc wrapper，以及不以 hunk/control marker 开头的裸上下文行；`---/+++` unified diff、`*** File:` 元数据头和自然语言编辑指令不属于 `apply_patch` 输入格式，应返回可纠正的错误提示。上下文匹配采用 Codex 风格的逐级宽容策略：先精确匹配，再忽略尾随空白，再忽略首尾空白，最后对常见 Unicode 标点和空白做 ASCII 归一化后匹配；当模型把同一个缩进上下文行同时写在新增块前后时，执行层可把它解释为围绕该上下文的纯插入，避免因为 patch 控制空格与文件缩进混淆而失败。没有 old lines 的纯新增 update chunk 按 Codex 语义追加到文件末尾；`*** Add File:` 可以覆盖已存在文件并可创建空文件，带 `*** Move to:` 的更新可以覆盖目标文件；`*** Delete File:` 仍要求目标存在且不是目录。为了避免同一进程内 agent、subagent 或同轮工具调用同时写 workspace，修改类文件工具共享进程内 workspace 写锁；该锁不承诺跨进程或外部编辑器互斥。

工具调用历史必须保留调用种类。`function_call` 的历史回放写回 `function_call_output`；custom/freeform 工具写回 `custom_tool_call_output`。不得只保存 JSON arguments 后在下一轮统一当作 function tool 回放。

Skills 工具同样挂在 `pl-core` 默认工具集中。`skills_list` 和 `skill_view` 是只读工具；`skill_manage` 是写入工具，但只能修改当前项目的 `<workspace_root>/skills/`，不能修改用户级、系统或外部只读 skills。subagent 通过同一默认工具注册入口继承 skills 能力。
