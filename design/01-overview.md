# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。它把用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

当前架构先收束为核心层与 Tauri 2 桌面前端：

```text
pure-studio
  │  Tauri 2 桌面应用：React UI、命令桥接、事件推送、输入回调
  ▼
pl-core
  │  核心逻辑层：turn、session、配置、Studio SQLite、工具审批、核心编译流程编排
  ├────────► pl-model
  │           LLM provider、模型元数据、wire API、SSE
  ▼
pl-protocol
              跨 crate 公共协议类型、事件、错误、消息
```

## 1.2 核心概念

| 概念 | 说明 |
| --- | --- |
| `pure-studio` | Pure-Lang 的 Tauri 2 桌面前端，Rust 侧负责命令桥接和事件推送，React 前端负责 UI 渲染和交互状态 |
| `pl-core` | 核心逻辑层，组合配置、Studio 状态库、会话、单轮请求、工具审批、模型调用和结果整理 |
| `pl-model` | LLM provider 层，负责外部模型 API 适配 |
| `pl-protocol` | 公共协议层，定义消息、事件、错误和权限等共享类型 |
| `AgentEvent` | 系统统一事件流，供前端渲染、日志和测试消费 |

## 1.3 设计原则

- `pl-protocol` 不依赖内部 crate，是协议和类型边界。
- `pl-model` 只依赖 `pl-protocol`，不承担核心流程编排。
- `pl-core` 可以依赖 `pl-model` 和 `pl-protocol`，负责组合核心逻辑、持久化配置和 Studio SQLite 状态。
- `pure-studio` 是薄桌面入口层，Tauri Rust 侧只做命令桥接、事件推送和工具审批等待；React 前端负责用户输入、页面状态和渲染。
- 当前版本没有独立沙箱层；桌面端仅在用户显式批准工具调用后执行已注册工具。

## 1.4 桌面编译路径

```text
用户选择项目和会话
  → pure-studio 调用 pl-core Studio API
  → pl-core 读取 ~/.pure/studio/studio_1.sqlite
  → pl-core 读取 ~/.pure/config.toml
  → pl-core 构造 TurnRequest 和 TurnOptions
  → pl-core 读取项目 Agents.md 并运行 turn
  → pl-model 推送 AgentEvent
  → pure-studio 通过 Tauri event 转发并在 React UI 中流式渲染内容、思考和工具审批
  → pl-core 通过 SeaORM 保存会话消息和审批记录到 SQLite
```

## 1.5 依赖规则

```text
pl-protocol
    ↑
pl-model
    ↑
pl-core
    ↑
pure-studio
```

`pl-core` 也直接依赖 `pl-protocol`，用于核心事件、消息和错误类型。
