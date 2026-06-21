# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。它把用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

当前架构先收束为核心层与桌面前端。Tauri 2 + Solid/Vite 端继续作为稳定对照；Flutter + flutter_rust_bridge v2 端作为 Windows 优先的新桌面端并行实现，两者共享 `pl-core` 的 Studio runtime、SQLite projection 和公共协议。

```text
pure-studio
  │  Tauri 2 桌面应用：Solid UI、命令桥接、全量事件推送、输入回调
  ▼
pl-core
  │  核心逻辑层：turn、session、配置、Studio SQLite、工具审批、runtime 状态机、核心编译流程编排
  ├────────► pl-model
  │           LLM provider、模型元数据、wire API、SSE
  ├────────► pl-trace
  │           内部 agent/trace 事件通道
  ▼
pl-protocol
              跨 crate 公共 wire DTO、错误、消息、状态类型

pure-studio-flutter
  │  Flutter Windows 桌面应用：Material 3、Riverpod、按会话订阅事件流
  ▼
pl-studio-bridge
  │  flutter_rust_bridge v2：FRB API、Stream<BridgeEventEnvelope>、camelCase JSON payload
  ▼
pl-core
```

## 1.2 核心概念

| 概念 | 说明 |
| --- | --- |
| `pure-studio` | Pure-Lang 的 Tauri 2 桌面前端，Rust 侧负责命令桥接和事件推送，Solid 前端负责 UI 渲染和交互状态 |
| `pure-studio-flutter` | Pure-Lang 的 Flutter 桌面前端，首版只承诺 Windows，使用 Riverpod store 和会话级事件订阅 |
| `pl-studio-bridge` | Flutter Rust Bridge v2 桥接 crate，把 Flutter API 转为 `pl-core` Studio runtime 调用，并把 Rust event stream 映射为 Dart stream |
| `pl-core` | 核心逻辑层，组合配置、Studio 状态库、会话、单轮请求、工具审批、模型调用、runtime 状态机和结果整理 |
| `pl-model` | LLM provider 层，负责外部模型 API 适配 |
| `pl-protocol` | 公共协议层，定义消息、Studio wire DTO、错误、权限和状态等共享类型 |
| `pl-trace` | 内部 trace 协议层，定义 `AgentEvent`、`TraceEvent` 和 `TracePart`，进入 Studio 前必须映射为 message/part 事件 |

## 1.3 设计原则

- `pl-protocol` 不依赖内部 crate，是协议和类型边界。
- `pl-model` 只依赖 `pl-protocol` 与 `pl-trace`，不承担核心流程编排。
- `pl-core` 可以依赖 `pl-model`、`pl-protocol` 和 `pl-trace`，负责组合核心逻辑、持久化配置和 Studio SQLite 状态。
- `pure-studio` 是薄桌面入口层，Tauri Rust 侧只做命令桥接、事件推送，并保留工具审批回调能力；Solid 前端负责用户输入、页面状态和渲染。
- `pure-studio-flutter` 同样保持薄入口层，Flutter UI 不直接持久化业务状态，只通过 `pl-studio-bridge` 调用 `pl-core` 并消费 typed event stream。
- `pl-studio-bridge` 不拥有业务规则。复杂 payload 通过 canonical camelCase JSON 字符串传输，Dart 层用 `kindType` 和 typed routing fields 解码为 sealed model，避免 FRB API 直接暴露 `serde_json::Value`。
- 当前版本没有独立沙箱层；Studio 运行路径暂时使用 `ToolApprovalPolicy::AutoAllow`，已注册工具会按 `pl-core` 的工作区边界和工具实现直接执行。

## 1.4 桌面编译路径

```text
用户选择项目和会话
  → pure-studio 或 pure-studio-flutter 调用 pl-core Studio API
  → pl-core 读取 ~/.pure/studio/studio_1.sqlite
  → pl-core 读取 ~/.pure/config.toml
  → pl-core 确认 Studio runtime 状态为 Ready
  → pl-core 构造 TurnRequest 和 TurnOptions
  → pl-core 读取项目 Agents.md 并运行 turn
  → pl-model 推送 pl-trace AgentEvent
  → pl-core 将内部 trace 映射为 Studio message/part snapshot 与 live delta
  → pure-studio 通过 Tauri event 转发 StudioEventEnvelope 并在 Solid UI 中流式渲染内容、思考和工具状态
  → pure-studio-flutter 通过 FRB Stream<BridgeEventEnvelope> 只监听当前会话高频事件
  → pl-core 通过 SeaORM 保存会话消息和按策略产生的审批记录到 SQLite
```

Studio runtime 对 UI 暴露明确状态机：

```text
Uninitialized -> Initializing -> Ready -> ShuttingDown -> Stopped
                         │                         │
                         └──────────────► Failed ◄──┘
```

`initializeRuntime()` 只完成配置、store、projection 恢复和未完成 turn 收敛；`startRuntime()` 启动后台 MCP/LSP health、事件桥接和可取消 turn 运行；`shutdownRuntime()` 取消活动 turn、关闭后台任务并进入 `Stopped` 或 `Failed`。每个会话拥有独立 active turn 与 cancellation handle；打开哪个会话，Flutter 端才订阅哪个会话的高频事件。

Studio 事件分为三类订阅：

- 兼容旧 Tauri/Solid 的全量订阅：继续转发所有 `StudioEventEnvelope`。
- 会话订阅：`subscribe_session(sessionId)` 只包含该会话 timeline、turn、interaction、session runtime、agent 和高频 `messagePartDelta`。
- 全局订阅：`subscribe_global()` 包含项目、配置、Provider usage、MCP/LSP health 等低频全局变化。

## 1.5 依赖规则

```text
pl-protocol
    ↑
pl-trace
    ↑
pl-model
    ↑
pl-core
    ↑
pure-studio
    ↑
pl-studio-bridge
    ↑
pure-studio-flutter
```

`pl-core` 也直接依赖 `pl-protocol` 与 `pl-trace`，分别用于公共 wire/status 类型和内部运行 trace 类型。
