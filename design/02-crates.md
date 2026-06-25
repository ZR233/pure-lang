# 02 - Crate 设计（方案乙）

## 2.1 总体形态

本仓库继续保持模块化单体，不新增常驻进程。核心边界采用端口-适配器：

- `pl-protocol`：跨 crate 公共 wire 协议、状态与错误
- `pl-trace`：内部 agent/trace 事件协议
- `pl-model`：模型 provider 适配
- `pl-lsp`：LSP 客户端与语言服务器运行时
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

## 2.5 pl-lsp

`pl-lsp` 负责语言服务器协议和运行时边界。

- LSP JSON-RPC framing、URI/path 转换和 server 进程生命周期
- `LspRuntimeRegistry`、诊断和 query 结果类型
- 不依赖 `pl-core`；`pl-core` 在工具路径策略完成后，把规范化绝对路径交给 `pl-lsp`

## 2.6 pl-core（端口-适配器）

`pl-core` 继续作为所有桌面端共享的 Studio runtime 所有者，并按端口-适配器边界渐进式整理。当前代码中 `interfaces` 放置可替换端口 trait，`application`、`domain` 和 `infrastructure` 作为边界入口与 re-export；既有实现仍按 `studio`、`core`、`tool`、`config`、`mcp` 等业务命名空间组织，后续重构应逐步把新端口和适配器沉入对应边界。

- `application`：use case 编排入口（当前 re-export `StudioRuntime`）
- `domain`：项目、会话和 runtime 记录等领域类型入口
- `interfaces`：端口 trait（RPITIT + `Send`）
- `infrastructure`：配置、SQLite store 等适配器入口

当前核心端口：

- `SessionRepository`
- `ConfigRepository`
- `EventSink`
- `TurnSnapshotRepository`
- `RuntimeEventEmitter`

约束：

- trait 异步方法统一使用原生 RPITIT，并显式 `+ Send`
- `lib.rs` 只做模块声明与 `pub use` 出口
- `StudioRuntime` 不直接嵌入具体数据库/文件系统细节
- UI-facing runtime 状态机固定为 `Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped/Failed`
- `active_turns`、`submit_prompt`、`stop_prompt`、`resolve_interaction` 和后台 turn 启动/取消语义属于 `pl-core::studio`，Flutter 只做调用者
- `StudioEventRuntime` 保留旧全量订阅，并新增会话订阅和全局订阅；高频 message/part delta 只进入会话流，MCP/LSP health、配置和项目列表等低频变化进入全局流

默认权限模式固定为 `PermissionMode::RequestApproval`。旧 `ToolApprovalPolicy::AutoAllow | Manual | DenyAll` 只作为兼容构造保留，核心执行前统一以 `PermissionMode` 和工具路径访问分类做策略判断。

## 2.7 pl-studio-bridge（FRB 桥接）

`pl-studio-bridge` 位于 `code/pure-studio-flutter/rust/`，crate 名称遵循 `pl-` 前缀。它是 Flutter Rust Bridge v2 的 native crate，只负责把 Dart 调用转换成 `pl-core` Studio runtime API。

公开 API 以 Flutter 端需求为边界：

- `initializeRuntime() -> RuntimeSnapshot`
- `startRuntime() -> RuntimeSnapshot`
- `shutdownRuntime() -> RuntimeSnapshot`
- `bootstrapStudio() -> BridgeStudioSnapshotResponse`
- `openProject(path) -> BridgeStudioSnapshotResponse`
- `selectProject(projectId) -> BridgeStudioSnapshotResponse`
- `archiveProject(projectId, selectedProjectId) -> BridgeStudioSnapshotResponse`
- `createSession(projectId, title) -> BridgeStudioSnapshotResponse`
- `archiveSession(sessionId, selectedSessionId) -> BridgeStudioSnapshotResponse`
- `setSessionMode(sessionId, mode) -> BridgeSessionStateResponse`
- `setModelRole(roleKey, providerId, model, effort, selectedSessionId) -> BridgeStudioSnapshotResponse`
- `saveRuntimePermissionMode(mode) -> ConfigSavedResponse`
- `saveProviderSettings(settingsJson) -> BridgeStudioSnapshotResponse`
- `saveInstructionsSettings(settingsJson) -> BridgeStudioSnapshotResponse`
- `saveSkillsSettings(settingsJson) -> BridgeStudioSnapshotResponse`
- `saveMcpSettings(settingsJson) -> BridgeStudioSnapshotResponse`
- `saveGeneralSettings(settingsJson) -> BridgeStudioSnapshotResponse`
- `saveStudioSettingsDraft(section, draftJson) -> SettingsDraftResponse`
- `loadProviderUsages() -> ProviderUsagesResponse`
- `submitPrompt(sessionId, prompt, attachmentIds) -> SubmitPromptResponse`
- `stopPrompt(sessionId) -> StopPromptResponse`
- `resolveInteraction(interactionId, resolutionJson) -> ResolveInteractionResponse`
- `loadSessionState(sessionId) -> BridgeSessionStateResponse`
- `loadStudioEvents(sessionId, afterSequence, limit) -> BridgeStudioEventsResponse`
- `listDiscoveredSkills(projectId) -> SkillsResponse`
- `subscribeSessionEvents(sessionId) -> Stream<BridgeEventEnvelope>`
- `subscribeGlobalEvents() -> Stream<BridgeEventEnvelope>`

`openProject` 调用 `pl-core::studio` 的项目打开、LSP reconcile 和 session bootstrap 流程后返回新的 Studio 快照。`selectProject`、`archiveProject`、`createSession`、`archiveSession`、`setSessionMode` 和 `setModelRole` 都返回 Studio 快照或当前 session snapshot，由 Flutter store 原子替换项目、会话、选中项、config view，并对返回的 `selectedSessionId` 再调用 `loadSessionState` 恢复当前会话 projection。`archiveProject` 是归档语义，不删除项目目录或历史会话；关闭当前项目时返回下一个可用项目/会话，没有剩余项目时返回空选中态。`setSessionMode` 只修改 session 的下一轮 `compileMode`；`setModelRole` 修改 `~/.pure/config.toml` 中对应模型角色，状态栏 planner 模型选择固定写 `planner` role，因为 Studio 根聊天 turn 始终使用 planner 角色。`listDiscoveredSkills` 只读取当前项目 workspace、user、system 与 external skill catalog，返回 typed `skills[]`，不改变配置。typed settings 保存接口直接写回 canonical config 或 Studio setting，并返回新的 Studio 快照；`saveStudioSettingsDraft` 只保留兼容旧草稿入口，不作为当前设置页生效配置的主路径。

`BridgeEventEnvelope` 当前 wire 字段为：`eventId`、`sessionId`、`turnId`、`sequence`、`createdAt`、`payload: BridgeEventPayload`。`BridgeEventPayload` 是 FRB/Freezed sealed union，承载 turn、message、part、delta、interaction、agent、agent timeline、runtime、health、session list 和 stale 等结构化事件；Dart FRB adapter 将其归一为 app 内部 typed `StudioBridgeEventPayload` 后交给 Riverpod reducer。`loadStudioEvents` backfill 返回 `BridgeStudioEventsResponse`，其中 `events[]` 与实时 stream 使用同一个 typed `BridgeEventEnvelope`。Studio snapshot、session snapshot 和小型命令响应均使用 typed DTO；完整 config 与 general settings 暂以 `configJson`/`generalSettingsJson` 字符串保留在 adapter 边界，agent timeline payload 使用 `BridgeAgentTimelinePayloadDto` union 表达。桥接层不得把 `serde_json::Value` 直接暴露为 FRB 类型，也不得复制 UI 业务规则。

## 2.8 pure-studio-flutter（Flutter UI）

`pure-studio-flutter` 位于 `code/pure-studio-flutter/`，首版只承诺 Windows 桌面。UI 使用 Material 3 工具型设计、Riverpod 状态管理和 `go_router` 页面栈。功能覆盖 Studio 主路径：项目/会话侧栏、聊天 timeline、streaming markdown、reasoning/tool/plan part、composer、停止、权限模式、tool approval、user input、plan confirmation、状态栏，以及 Provider/Instructions/Skills/Roles/MCP/Security/General 设置页。

Flutter store 不直接读取 SQLite 或配置文件，只通过 `pl-studio-bridge` 调用 `pl-core`。打开会话时订阅该会话事件流，切换会话时取消旧订阅；全局事件流只承载低频配置、项目和 health 变化。

## 2.9 本地数据版本

方案乙采用破坏性升级，不保留运行期兼容层：

- SQLite 切换到新 schema（v2）
- `config.toml` 切换到新结构（v2）
- 启动时检测旧格式：先备份，再重建新结构

Flutter 桌面端不做额外 SQLite 或 `~/.pure/config.toml` 破坏性迁移。

## 2.10 Workspace

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
