# 02 - Crate 设计（方案乙）

## 2.1 总体形态

本仓库继续保持模块化单体，不新增常驻进程。核心边界采用端口-适配器：

- `pl-protocol`：跨 crate 公共 wire 协议、状态与错误
- `pl-trace`：内部 agent/trace 事件协议
- `pl-model`：模型 provider 适配
- `pl-core`：应用编排、领域模型、端口定义、基础设施适配器
- `pl-studio-bridge`：Flutter Rust Bridge v2 桥接 crate
- `pure-studio-flutter`：Flutter Windows 桌面端

## 2.2 pl-protocol

职责保持不变：定义稳定 wire 协议、错误与公共状态类型。

- 放置 `PureError`、`Message`、Studio DTO、interaction、runtime usage、agent status 等跨层共享类型
- 不依赖任何内部 crate
- 不包含 raw `AgentEvent` / `TracePart`、运行时行为与存储实现

## 2.3 pl-trace

`pl-trace` 是内部运行事件 crate。

- 放置 `AgentEvent`、`AgentEventSender`、`TraceEvent`、`TracePart`、`EnabledToolsEvent` 等 core/provider 内部类型
- 依赖 `pl-protocol` 的公共状态与 interaction 类型
- 不作为 Studio wire DTO 暴露；进入 UI 前必须由 `pl-core` 映射为 `StudioEventEnvelope`

## 2.4 pl-model

职责保持不变：封装 provider 差异，不承担会话编排。

- `ModelProvider` / `CompletionRequest` / `CompletionResponse`
- OpenAI-compatible wire 适配
- 依赖 `pl-protocol` 与 `pl-trace`，不依赖 `pl-core`

## 2.5 pl-core（端口-适配器）

`pl-core` 调整为四层目录语义，并继续作为所有桌面端共享的 Studio runtime 所有者：

- `application`：use case 编排（`StudioRuntime`）
- `domain`：会话、项目、timeline、审批等领域记录类型
- `interfaces`：端口 trait（RPITIT + `Send`）
- `infrastructure`：SQLite、文件系统、事件发射、工具执行等适配器

核心端口（示例）：

- `SessionRepository`
- `ConfigRepository`
- `TraceRepository`
- `EventSink`
- `ToolExecutor`

约束：

- trait 异步方法统一使用原生 RPITIT，并显式 `+ Send`
- `lib.rs` 只做模块声明与 `pub use` 出口
- `StudioRuntime` 不直接嵌入具体数据库/文件系统细节
- UI-facing runtime 状态机固定为 `Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped/Failed`
- `active_turns`、`submit_prompt`、`stop_prompt`、`resolve_interaction` 和后台 turn 启动/取消语义属于 `pl-core::studio`，Flutter 只做调用者
- `StudioEventRuntime` 保留旧全量订阅，并新增会话订阅和全局订阅；高频 message/part delta 只进入会话流，MCP/LSP health、配置和项目列表等低频变化进入全局流

审批默认策略固定为 `ToolApprovalPolicy::AutoAllow`。手动审批链路保留接口，但不是默认执行路径。

## 2.6 pl-studio-bridge（FRB 桥接）

`pl-studio-bridge` 位于 `code/pure-studio-flutter/rust/`，crate 名称遵循 `pl-` 前缀。它是 Flutter Rust Bridge v2 的 native crate，只负责把 Dart 调用转换成 `pl-core` Studio runtime API。

公开 API 以 Flutter 端需求为边界：

- `initializeRuntime() -> RuntimeSnapshot`
- `startRuntime() -> RuntimeSnapshot`
- `shutdownRuntime() -> RuntimeSnapshot`
- `bootstrapStudio() -> JsonResponse`
- `openProject(path) -> JsonResponse`
- `selectProject(projectId) -> JsonResponse`
- `archiveProject(projectId, selectedProjectId) -> JsonResponse`
- `createSession(projectId, title) -> JsonResponse`
- `archiveSession(sessionId, selectedSessionId) -> JsonResponse`
- `setSessionMode(sessionId, mode) -> JsonResponse`
- `setModelRole(roleKey, providerId, model, effort, selectedSessionId) -> JsonResponse`
- `saveRuntimePermissionMode(mode) -> JsonResponse`
- `saveStudioSettingsDraft(section, draftJson) -> JsonResponse`
- `submitPrompt(sessionId, prompt, attachmentIds) -> JsonResponse`
- `stopPrompt(sessionId) -> JsonResponse`
- `resolveInteraction(interactionId, resolutionJson) -> JsonResponse`
- `loadSessionState(sessionId) -> JsonResponse`
- `loadStudioEvents(sessionId, afterSequence, limit) -> JsonResponse`
- `listDiscoveredSkills(projectId) -> JsonResponse`
- `subscribeSessionEvents(sessionId) -> Stream<BridgeEventEnvelope>`
- `subscribeGlobalEvents() -> Stream<BridgeEventEnvelope>`

`openProject` 调用 `pl-core::studio` 的项目打开、LSP reconcile 和 session bootstrap 流程后返回新的 Studio 快照。`selectProject`、`archiveProject`、`createSession`、`archiveSession`、`setSessionMode` 和 `setModelRole` 都返回 Studio 快照或当前 session snapshot，由 Flutter store 原子替换项目、会话、选中项、config view，并对返回的 `selectedSessionId` 再调用 `loadSessionState` 恢复当前会话 projection。`archiveProject` 是归档语义，不删除项目目录或历史会话；关闭当前项目时返回下一个可用项目/会话，没有剩余项目时返回空选中态。`setSessionMode` 只修改 session 的下一轮 `compileMode`；`setModelRole` 修改 `~/.pure/config.toml` 中对应模型角色，状态栏 planner 模型选择固定写 `planner` role，因为 Studio 根聊天 turn 始终使用 planner 角色。`listDiscoveredSkills` 只读取当前项目 workspace、user、system 与 external skill catalog，返回 camelCase JSON 中的 `skills[]`，不改变配置。`saveRuntimePermissionMode` 直接写回 `~/.pure/config.toml` 的 runtime 权限模式；`saveStudioSettingsDraft` 用于 Flutter 首版设置页把尚未映射为完整 typed config command 的编辑内容持久化到 Studio store draft，后续升级为 typed save API 时不得改变已有 draft section 名称。

`BridgeEventEnvelope` 使用稳定字段：`eventId`、`sessionId`、`turnId`、`sequence`、`createdAt`、`kindType`、`payloadJson`。复杂 payload 保持 canonical camelCase JSON 字符串；Dart 层根据 `kindType` 解码为 sealed model。桥接层不得把 `serde_json::Value` 直接暴露为 FRB 类型，也不得复制 UI 业务规则。

## 2.7 pure-studio-flutter（Flutter UI）

`pure-studio-flutter` 位于 `code/pure-studio-flutter/`，首版只承诺 Windows 桌面。UI 使用 Material 3 工具型设计、Riverpod 状态管理和 `go_router` 页面栈。功能覆盖 Studio 主路径：项目/会话侧栏、聊天 timeline、streaming markdown、reasoning/tool/plan part、composer、停止、权限模式、tool approval、user input、plan confirmation、状态栏，以及 Provider/Instructions/Skills/Roles/MCP/Security/General 设置页。

Flutter store 不直接读取 SQLite 或配置文件，只通过 `pl-studio-bridge` 调用 `pl-core`。打开会话时订阅该会话事件流，切换会话时取消旧订阅；全局事件流只承载低频配置、项目和 health 变化。

## 2.8 本地数据版本

方案乙采用破坏性升级，不保留运行期兼容层：

- SQLite 切换到新 schema（v2）
- `config.toml` 切换到新结构（v2）
- 启动时检测旧格式：先备份，再重建新结构

Flutter 桌面端不做额外 SQLite 或 `~/.pure/config.toml` 破坏性迁移。

## 2.9 Workspace

workspace crate 组成保持不变：

```toml
[workspace]
members = [
    "code/pl-protocol",
    "code/pl-trace",
    "code/pl-model",
    "code/pl-lsp",
    "code/pl-core",
    "code/pure-studio-flutter/rust",
]
resolver = "3"
```
