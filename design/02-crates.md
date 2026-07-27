# 02 - Crate 设计（方案乙）

## 2.1 总体形态

本仓库继续保持模块化单体，不新增常驻进程。核心边界采用端口-适配器：

- `pl-protocol`：跨 crate 公共 wire 协议、状态与错误
- `pl-trace`：内部 agent/trace 事件协议
- `pl-model`：模型 provider 适配
- `pl-lsp`：LSP 客户端与语言服务器运行时
- `pl-core`：产品无关的 turn/session/agent 框架、模型配置值对象与通用工具
- `pl-studio-runtime`：Pure Studio 配置、持久化、项目与任务编排
- `pl-studio-bridge`：Flutter Rust Bridge v2 桥接 crate
- `pure-studio-flutter`：Flutter Windows 桌面端
- `pl-xtask`：本仓库开发任务入口，不参与运行时依赖链

## 2.2 pl-protocol

职责保持不变：定义稳定 wire 协议、错误与公共状态类型。

- 放置 `PureError`、`Message`、interaction、runtime usage、统一的
  `SessionEventEnvelope/SessionStreamFrame/SessionViewSnapshot` 与无 secret 的
  `ProviderCatalogSnapshot`、provider 服务能力、Web Search resolution、MCP/LSP health descriptor
  等跨产品 wire 类型
- 不依赖任何内部 crate
- 不包含 Studio 产品 DTO、raw trace、运行时行为与存储实现

## 2.3 pl-trace

`pl-trace` 是内部诊断 trace crate。

- 放置 `TraceEvent`、`TracePart`、`EnabledToolsEvent` 等 core/provider 内部类型
- 依赖 `pl-protocol` 的公共状态与 interaction 类型
- 不导出 UI broadcast；进入产品前必须由 `pl-core::SessionEventProjector` 映射为公共
  session event

## 2.4 pl-model

职责保持不变：封装 provider 差异，不承担产品 agent/session 编排。

- `ModelProvider` / `CompletionRequest` / `CompletionResponse`
- Responses WebSocket/HTTP、Chat Completions HTTP wire 适配
- session 级 `ModelTransportSession`，只管理物理连接、fingerprint 与 continuation
- 依赖 `pl-protocol` 与 `pl-trace`，不依赖 `pl-core`

## 2.5 pl-lsp

`pl-lsp` 负责语言服务器协议和运行时边界。

- LSP JSON-RPC framing、URI/path 转换和 server 进程生命周期
- `LspRuntimeRegistry`、诊断和 query 结果类型
- 不依赖 `pl-core`；`pl-core` 在工具路径策略完成后，把规范化绝对路径交给 `pl-lsp`

## 2.6 pl-core（Agent 框架）

`pl-core` 是产品无关的 agent 框架，不再拥有 Pure Studio。它提供 `TurnEngine`、
`AgentSession`、`AgentRuntime<H>`、非泛型 `AgentRuntimeHandle`、host 端口、动态执行策略和
通用工具。详细边界见 `17-agent-runtime-host.md`。

- `agent_runtime`：actor、命令句柄、host 端口、commit 与恢复
- `session_event`：公共 session projection、per-session channel、snapshot/replay 与 reducer
- `core`：turn pipeline、工具调度和结果归一化
- `tool`：通用工具、effect 与执行策略
- `mcp`：Host 驱动的公共 runtime、非泛型 handle、generation lease、健康状态和本地 Host
- `web_search`：provider capability planner 与统一工具安装入口
- `model_config`：只含 serde 值对象及校验/解析

核心 host 端口：

- `AgentStateRepository`
- `AgentTurnFactory`
- `AgentLifecycleAdapter`
- `AgentCommitObserver`

约束：

- trait 异步方法统一使用原生 RPITIT，并显式 `+ Send`
- `lib.rs` 只做模块声明与 `pub use` 出口
- `pl-core` 不依赖 SeaORM，不感知配置文件路径和产品 schema version
- agent/session/turn lifecycle 只能由 `AgentRuntime` 修改
- 产品工具只能通过 `AgentRuntimeHandle` 访问协作状态机

### 2.6.1 pl-studio-runtime

`pl-studio-runtime` 拥有 Studio config、SQLite/store、公共 session event repository 适配、
产品级事件、项目、会话、任务、worktree、Simple/Task 产品策略与 Studio-only DTO。
Flutter bridge 只调用该 crate。

默认权限模式固定为 `PermissionMode::RequestApproval`。旧 `ToolApprovalPolicy::AutoAllow | Manual | DenyAll` 只作为兼容构造保留，核心执行前统一以 `PermissionMode` 和工具路径访问分类做策略判断。

## 2.7 pl-studio-bridge（FRB 桥接）

`pl-studio-bridge` 位于 `code/pure-studio-flutter/rust/`，crate 名称遵循 `pl-` 前缀。它是 Flutter Rust Bridge v2 的 native crate，只负责把 Dart 调用转换成 `pl-studio-runtime` API。

公开 API 以 Flutter 端需求为边界：

- `initializeRuntime() -> RuntimeSnapshot`
- `startRuntime() -> RuntimeSnapshot`
- `shutdownRuntime() -> RuntimeSnapshot`
- `loadProviderCatalog() -> ProviderCatalogSnapshot`
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
- `listDiscoveredSkills(projectId) -> SkillsResponse`
- `subscribeSessionEvents(sessionId, afterSequence) -> Stream<BridgeSessionStreamFrame>`
- `subscribeGlobalEvents() -> Stream<BridgeEventEnvelope>`

`openProject` 调用 `pl-studio-runtime` 的项目打开、LSP reconcile 和 session bootstrap 流程后返回新的 Studio 快照。`selectProject`、`archiveProject`、`createSession`、`archiveSession`、`setSessionMode` 和 `setModelRole` 都返回 Studio 快照或当前 session snapshot，由 Flutter store 原子替换项目、会话、选中项和 config view。`loadProviderCatalog` 返回 PL canonical catalog，Flutter 只按 `revision` 做进程内缓存，不把目录写入 Studio 设置。`archiveProject` 是归档语义，不删除项目目录或历史会话；`setModelRole` 只保存 provider/model/effort 路由，模型元数据始终来自 catalog 或 provider 的 effective models。

`BridgeSessionStreamFrame` 机械映射 `pl-protocol::SessionStreamFrame`，session stream 的首帧
为 snapshot 或 replay，后续为 live event；lag 通过 `ResyncRequired` 要求重新订阅。Studio-only
global event 继续使用独立 envelope。桥接层不得复制 session projection 规则，也不得把
`serde_json::Value` 直接暴露为 FRB 类型。

## 2.8 pure-studio-flutter（Flutter UI）

`pure-studio-flutter` 位于 `code/pure-studio-flutter/`，首版只承诺 Windows 桌面。UI 使用 Material 3 工具型设计、Riverpod 状态管理和 `go_router` 页面栈。功能覆盖 Studio 主路径：项目/会话侧栏、聊天 timeline、streaming markdown、reasoning/tool/plan part、composer、停止、权限模式、tool approval、user input、plan confirmation、状态栏，以及 Provider/Instructions/Skills/Roles/MCP/Security/General 设置页。

Flutter store 不直接读取 SQLite 或配置文件，只通过 `pl-studio-bridge` 调用
`pl-studio-runtime`。打开会话时订阅该会话事件流，切换会话时取消旧订阅；全局事件流只承载低频配置、项目和 health 变化。

## 2.9 pl-xtask（开发任务入口）

`pl-xtask` 位于 `xtask/`，通过 `.cargo/config.toml` 暴露 `cargo xtask ...`。它只封装本仓库开发、运行和发布任务，不承载运行时业务逻辑，也不被任何 runtime crate 依赖。

公开命令：

- `cargo xtask run-gui [--demo] [--demo-fallback]`
- `cargo xtask build-gui [--demo] [--no-clean]`
- `cargo xtask build-rust-bridge --workspace-root <path> --configuration <Debug|Profile|Release> --output-dir <path> [--target-dir <path>]`

GUI 命令从仓库根目录调用，但所有 Flutter 子命令都以 `code/pure-studio-flutter/` 为工作目录执行。`build-rust-bridge` 是 Flutter Windows CMake 内部入口，负责构建并复制 `pl_studio_bridge.dll`/`.pdb`。

## 2.10 本地数据版本

Studio SQLite 的新库使用单一基础 schema（当前 `user_version = 5`）。受支持的旧版本先
备份，再通过事务 migration chain 升级；`user_version = 0` 且已经包含用户表的数据库属于
不兼容 legacy schema，不进入 migration chain，而是完整归档为唯一备份后重建当前数据库。
空的未版本化数据库可直接初始化。未来版本明确拒绝打开，迁移失败不得删除或降级原数据库。
`config.toml` 当前 schema 为 10，继续由 Studio runtime 单点校验与升级；Flutter 不实现
第二套迁移逻辑。

## 2.11 Workspace

workspace crate 组成：

```toml
[workspace]
members = [
    "code/pl-protocol",
    "code/pl-trace",
    "code/pl-model",
    "code/pl-lsp",
    "code/pl-output",
    "code/pl-patch",
    "code/pl-skill-core",
    "code/pl-core",
    "code/pl-studio-runtime",
    "code/pure-studio-flutter/rust",
    "xtask",
]
resolver = "3"
```
