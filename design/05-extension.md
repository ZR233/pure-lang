# 05 - 扩展点

## 5.1 Provider 扩展

新增模型 provider 时优先扩展 `pl-model`：

- 新增 `ProviderInfo` 构造或配置来源。
- 在 `~/.pure/config.toml` 中新增 provider 和完整 models 列表。
- 实现或复用 `ModelProvider`。
- 适配目标 API 的 request/response wire 格式。

公共消息、事件和错误类型继续来自 `pl-protocol`。

配置里的模型信息会覆盖或补充 bundled model，使用户可以接入自定义模型。

## 5.2 核心流程扩展

需要影响 turn、session、store 或编译阶段时扩展 `pl-core`。

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

`apply_patch` 的唯一正式输入协议是 Codex 风格 freeform patch：外层使用 `*** Begin Patch` / `*** End Patch`，每个文件操作必须以 `*** Add File:`、`*** Delete File:` 或 `*** Update File:` 开头。OpenAI Responses 和支持 custom tools 的 Chat Completions provider 使用 `type: "custom"` 工具，并通过 Lark grammar 描述 patch 格式。不支持 custom/freeform 的 Chat-compatible provider 使用普通 function fallback，将完整 patch 放入 `patch` 字段；工具描述和 function fallback schema 必须给出一个最小有效 `*** Update File:` 示例，帮助真实模型生成可执行格式。执行层仍复用同一个 parser、preflight 和 apply 流程。同一个 patch 内多个普通 `*** Update File:` 指向同一现有文件时，执行层按出现顺序合并为一次最终写入；`*** Add File:`、`*** Delete File:` 和带 `*** Move to:` 的更新仍要求相关路径在 patch 内唯一，避免文件存在状态含糊。执行层可以剥离单个 patch block 外部的 markdown fence、heredoc-like wrapper 或前后说明文字，但 `---/+++` unified diff、`*** File:` 元数据头和自然语言编辑指令不属于 `apply_patch` 输入格式，应返回可纠正的错误提示。

工具调用历史必须保留调用种类。`function_call` 的历史回放写回 `function_call_output`；custom/freeform 工具写回 `custom_tool_call_output`。不得只保存 JSON arguments 后在下一轮统一当作 function tool 回放。

Skills 工具同样挂在 `pl-core` 默认工具集中。`skills_list` 和 `skill_view` 是只读工具；`skill_manage` 是写入工具，但只能修改当前项目的 `<workspace_root>/skills/`，不能修改用户级、系统或外部只读 skills。subagent 通过同一默认工具注册入口继承 skills 能力。
