# 02 - Crate 边界

## 2.1 总体形态

Studio 是模块化单体业务核心，同时提供桌面 FRB 与 HTTP 两个 transport：

```text
pure-studio → pl-studio-bridge ─┐
                               ├→ pl-studio-runtime → pl-core
pl-studio-server ───────────────┘          │             │
                                          └→ pl-protocol ←┘
                                                  ↑
                                  pl-model / pl-trace / pl-lsp
```

两个 transport 只做 typed DTO 映射，不拥有业务状态。会话主线统一为
`Thread → Turn → Item`；root 与 child Agent 使用同一框架。

## 2.2 稳定边界

- `pl-protocol`：Thread、Turn、Item、Interaction、workflow、Agent Profile snapshot、通知与错误。
- `pl-model`：provider 请求、stream 归一化、模型目录与连接协议。
- `pl-core`：会话编排、模型循环、工具运行时、Skill catalog、working state 与 workflow 编译器。
- `pl-studio-runtime`：项目/Thread owner、配置、内置 Mode Skill、Agent Profile catalog、SQLite repository。
- `pl-studio-bridge`：Rust 与 Dart 的机械映射。
- `pl-studio-server`：同一 runtime 的 HTTP/SSE 适配。
- `pure-studio`：Flutter projection、交互与设置 UI。

Mode 不是运行时类型分支。`mode.simple`、`mode.task` 与自定义 `mode.<id>` 都是预加载 Skill；
`StudioModeId` 只是稳定字符串。工作流完整状态存入 `AgentWorkingState`，不新增产品业务表。

## 2.3 事实归属

Thread owner 的 typed 内存 snapshot 是活动状态唯一事实源。SQLite 只用于冷恢复、历史分页和
checkpoint 持久化。GUI 只消费 bridge 返回的 canonical snapshot/notification，不能在 Dart 侧
推演工作流状态。

协作实例与配置 Profile 分离：`list_agent_profiles` 返回可用配置，`list_agents` 返回运行实例。
系统 Profile 由 Rust 注册且不可编辑、不可删除；用户 Profile 位于 `~/.pure/agents/*.toml`。

旧 TaskCoordinator、TaskRuntime、TaskRun、WorkUnit、ReviewRound、MergeRecord、worktree 与
专用确认/恢复协议不属于任何 crate 边界，也没有兼容 adapter。
