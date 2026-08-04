# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。它把用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

当前架构收束为核心层与 Flutter 桌面前端。Flutter + flutter_rust_bridge v2 是唯一桌面入口，调用 `pl-studio-runtime` 组合的 `pl-core` agent runtime、双库 SQLite adapter 和公共协议。

```text
pure-studio
  │  Flutter Windows 桌面应用：Material 3、Riverpod、按会话订阅事件流
  ▼
pl-studio-bridge
  │  flutter_rust_bridge v2：FRB API、Stream<BridgeEventEnvelope> typed payload
  ▼
pl-core
```

## 1.2 核心概念

| 概念 | 说明 |
| --- | --- |
| `pure-studio` | Pure-Lang 的 Flutter 桌面前端，首版只承诺 Windows，使用 Riverpod store 和会话级事件订阅 |
| `pl-studio-bridge` | Flutter Rust Bridge v2 桥接 crate，把 Flutter API 转为 `pl-core` Studio runtime 调用，并把 Rust event stream 映射为 Dart stream |
| `pl-core` | 产品无关核心逻辑层，组合会话、单轮请求、工具审批、模型调用、runtime 状态机和结果整理，不依赖 SeaORM 或 Studio 路径 |
| `pl-studio-runtime` | Pure Studio 产品宿主，拥有 config、双库 SQLite schema、Task/worktree、历史 writer 与 UI projection |
| `pl-model` | LLM provider 层，负责外部模型 API 适配 |
| `pl-lsp` | LSP 客户端层，负责语言服务器进程、JSON-RPC framing 和代码智能查询 |
| `pl-protocol` | 公共协议层，定义消息、Studio wire DTO、错误、权限和状态等共享类型 |
| `pl-trace` | 内部 trace 协议层，定义 `AgentEvent`、`TraceEvent` 和 `TracePart`，进入 Studio 前必须映射为 message/part 事件 |

## 1.3 设计原则

- `pl-protocol` 不依赖内部 crate，是协议和类型边界。
- `pl-model` 只依赖 `pl-protocol` 与 `pl-trace`，不承担核心流程编排。
- `pl-lsp` 只依赖 LSP 协议与异步运行时，不依赖 `pl-core`。
- `pl-core` 可以依赖 `pl-model`、`pl-lsp`、`pl-protocol` 和 `pl-trace`，负责组合产品无关核心逻辑；Studio 配置和 SQLite 持久化归 `pl-studio-runtime`。
- `pure-studio` 保持薄入口层，Flutter UI 不直接持久化业务状态，只通过 `pl-studio-bridge` 调用 `pl-core` 并消费稳定的 Studio event envelope。
- `pl-studio-bridge` 不拥有业务规则。实时 stream 和 stale backfill 的 `BridgeEventEnvelope` 使用 typed FRB payload union，Dart 边界层归一为 `StudioBridgeEventPayload` 后交给 Riverpod reducer；snapshot 和命令响应使用 typed DTO。完整 config/general settings 与工具参数这类开放 JSON 标量只能停留在 FRB adapter 边界，不能扩散到 UI store；interaction payload 与 agent timeline payload 必须作为 typed DTO/union 传递。桥接层避免直接暴露 `serde_json::Value`，未知协议不得静默降级。
- 当前版本没有独立沙箱层；Studio 运行路径默认使用 `PermissionMode::RequestApproval`。workspace 内访问按本地策略直接放行，workspace 外访问按权限模式请求用户审批、AI reviewer 审批或在 `full-access` 下放行。

## 1.4 桌面编译路径

```text
用户选择项目和会话
  → pure-studio 调用 pl-core Studio API
  → pl-studio-runtime 读取 ~/.pure/studio/studio_state.sqlite 与 studio_history.sqlite
  → pl-studio-runtime 读取 ~/.pure/config.toml
  → pl-core 确认 Studio runtime 状态为 Ready
  → pl-core 构造 TurnRequest 和 TurnOptions
  → pl-core 读取项目 Agents.md 并运行 turn
  → pl-model 推送 pl-trace AgentEvent
  → pl-core 将内部 trace 映射为 Studio message/part snapshot 与 live delta
  → pure-studio 通过 FRB Stream<BridgeEventEnvelope> 只监听当前会话高频事件
  → pl-studio-runtime 异步批量写完整历史，再提交状态 projection 和广播 durable event
```

双库的 schema、提交顺序、恢复、分页、GC 和日志合同见
`19-studio-storage-and-diagnostics.md`。

Studio runtime 对 UI 暴露明确状态机：

```text
Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped
                         │                         │
                         └──────────────► Failed ◄──┘
```

`initializeRuntime()` 只完成配置、store、projection 恢复和未完成 turn 收敛；`startRuntime()` 启动后台 MCP/LSP health、事件桥接和可取消 turn 运行；`shutdownRuntime()` 取消活动 turn、关闭后台任务并进入 `Stopped` 或 `Failed`。每个会话拥有独立 active turn 与 cancellation handle；打开哪个会话，Flutter 端才订阅哪个会话的高频事件。

Studio 事件分为两类订阅：

- 会话订阅：`subscribe_session(sessionId)` 只包含该会话 timeline、turn、interaction、session runtime、agent 和高频 `messagePartDelta`。
- 全局订阅：`subscribe_global()` 包含项目、配置、Provider usage、MCP/LSP health 等低频全局变化。

## 1.5 依赖规则

```text
pl-protocol  ←  pl-trace  ←  pl-model  ←  pl-core  ←  pl-studio-bridge  ←  pure-studio
                              pl-lsp    ←  pl-core
```

`pl-core` 也直接依赖 `pl-protocol`、`pl-trace`、`pl-model` 与 `pl-lsp`，分别用于公共 wire/status 类型、内部运行 trace、模型 provider 和 LSP 查询能力。
