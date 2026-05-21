# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。它把用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

当前架构先收束为四个核心 crate：

```text
purec
  │  命令行编译器前端：clap 参数解析、结果渲染
  ▼
pl-core
  │  核心逻辑层：turn、session、配置、核心编译流程编排
  ├────────► pl-model
  │           LLM provider、模型元数据、wire API、SSE
  ▼
pl-protocol
              跨 crate 公共协议类型、事件、错误、消息
```

## 1.2 核心概念

| 概念 | 说明 |
| --- | --- |
| `purec` | Pure-Lang 的命令行编译器前端，负责接收参数并调用核心层 |
| `pl-core` | 核心逻辑层，组合配置、会话、单轮请求、模型调用和结果整理 |
| `pl-model` | LLM provider 层，负责外部模型 API 适配 |
| `pl-protocol` | 公共协议层，定义消息、事件、错误和权限等共享类型 |
| `AgentEvent` | 系统统一事件流，供前端渲染、日志和测试消费 |

## 1.3 设计原则

- `pl-protocol` 不依赖内部 crate，是协议和类型边界。
- `pl-model` 只依赖 `pl-protocol`，不承担核心流程编排。
- `pl-core` 可以依赖 `pl-model` 和 `pl-protocol`，负责组合核心逻辑和持久化配置。
- `purec` 是最薄的入口层，只做参数解析、初始化和展示。
- 当前版本不包含独立执行层，不执行命令、不修改文件、不提供沙箱。

## 1.4 当前编译路径

```text
自然语言 prompt
  → purec 解析 CLI
  → pl-core 读取 ~/.pure/config.toml
  → pl-core 使用 planner 角色解析 provider/model/effort
  → purec 构造 TurnRequest
  → pl-core 写入 CoreSession
  → pl-core 构造 CompletionRequest
  → pl-model 调用 provider 并推送 AgentEvent
  → pl-core 汇总 TurnResult
  → purec 渲染结果
```

## 1.5 依赖规则

```text
pl-protocol
    ↑
pl-model
    ↑
pl-core
    ↑
purec
```

`pl-core` 也直接依赖 `pl-protocol`，用于核心事件、消息和错误类型。
