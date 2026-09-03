# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。Pure Studio 的业务核心只有
`pl-studio-runtime::StudioRuntime`；桌面 Flutter/FRB 与独立 HTTP server 是两种 API
适配器，不拥有业务状态或产品规则。

系统只使用四个会话概念：

- `Thread`：一个 Agent 独占的对话、模型上下文和输入队列；root Thread 是用户可见会话，
  child Thread 是子 Agent 自己的会话。
- `Turn`：Thread 中一次由明确输入启动的执行，状态为 queued、inProgress、completed、
  failed 或 interrupted。
- `Item`：Turn 内按固定顺序出现的用户消息、Agent 消息、reasoning、plan、tool call、file，
  以及内部 context patch / context compaction。
- `Interaction`：等待用户回答的 user input 或 tool approval；它不是普通聊天 Item。

Simple、Task 与自定义模式不是独立会话类型。它们都是同一 root Agent 的 `Thread Mode`；Mode Prompt
与可选预设图由内存注册表提供，模型不负责定义或编译工作流。

## 1.2 运行路径

```text
Flutter ThreadWorkspace ── typed FRB ─┐
                                     ├─ StudioRuntime
pl-studio-server ─ REST / typed SSE ─┘        ↓
                                      ThreadManager → ThreadActor → TurnEngine
                                             ↓              ↓ typed notifications
                                            studio.sqlite
```

- `ThreadManager` 维护 Thread registry、父子关系和 spawn/close。
- 每个 `ThreadActor` 串行拥有一个 Thread 的输入队列、活动 Turn、取消句柄、live Item overlay
  与 `AgentWorkingState`。
- `TurnEngine` 只负责模型采样、工具调用、Interaction 等待和上下文压缩。
- root Thread 预加载当前 Mode Prompt，拥有可选的拆分 workflow 工具和统一 `complete` 工具；child
  Thread 使用冻结的 Agent Profile 且不拥有 root workflow 工具。
- `studio.sqlite` 是 durable Thread/Turn/Item/Interaction、working state 与 Studio 产品事实的
  唯一数据库。

## 1.3 唯一事实源

| 事实 | 唯一拥有者 |
| --- | --- |
| Thread、Turn、Item、输入、Interaction、working state | `studio.sqlite` |
| 活动 Turn、流式增量、steer、取消 identity、prompt generation | `ThreadActor` |
| Workflow run、revision、history | `AgentWorkingState.workflow` |
| Thread Mode Prompt 与预设图 | `pl-core::thread::mode::ThreadModeManager` 不可变快照 |
| Agent Profile 文件 | `~/.pure/agents/*.toml`；系统 Profile 由 runtime 注册 |
| Composer、滚动、展开、订阅 generation | Flutter `WorkspaceUiState` |

不存在第二套 session/message/part projection、durable event journal、Task 产品表或双库 watermark。
UI snapshot 由 canonical 表与活动 actor overlay 组成；历史只按 Turn keyset 分页。

## 1.4 Crate 边界

- `pl-protocol`：Thread/Turn/Item、Interaction、workflow/Profile runtime 与 product wire 类型。
- `pl-trace`：模型和工具内部诊断事件，不作为 UI 协议或持久化事实源。
- `pl-model`：provider 与 transport 适配。
- `pl-core`：ThreadManager、ThreadActor、TurnEngine、工作流编译/状态工具、通用工具与 Agent
  control plane。
- `pl-studio-runtime`：唯一 Studio 业务实现，拥有单库 StudioStore、项目、配置、Mode/Profile
  catalog、生命周期与产品事件。
- `pl-studio-bridge`：Studio protocol 到 FRB wire 的机械映射与桌面宿主能力。
- `pl-studio-server`：可单独运行的 loopback HTTP/OpenAPI/SSE 适配器。
- `pure-studio`：ThreadWorkspace reducer、timeline、Interaction、workflow panel、状态栏和设置 UI。

模块默认私有；Flutter 不能从 Item 或 Interaction 本地推断 canonical Turn、workflow 或 Profile
状态。

## 1.5 恢复原则

进程重启不能恢复物理模型连接。启动事务把遗留 inProgress Turn/Item 收束为
`interrupted(runtimeRestarted)`，重新排队未确认消费的明确输入，取消 tool approval，并保留
user input。Thread 的 typed working state 与 workflow run 原样恢复；恢复后下一 Turn 从最新
`pl.workflow` projection 继续。

数据库 schema 不兼容时只执行 `19-studio-storage-and-diagnostics.md` 定义的破坏性重建；普通恢复、
清理和归档不猜测外部资源所有权，也不自动创建或操作 Git branch/worktree。
