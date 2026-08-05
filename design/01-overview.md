# 01 - 系统总览

## 1.1 系统定位

Pure-Lang 是一个自然语言编译器。Pure Studio 是当前唯一桌面产品入口，由 Flutter UI、
flutter_rust_bridge、Studio 产品运行时和产品无关的 Thread runtime 组成。

系统只使用四个会话概念：

- `Thread`：一个 agent 独占的对话、模型上下文和输入队列；root Thread 是用户可见会话，
  child Thread 是子代理自己的会话。
- `Turn`：Thread 中一次由明确输入启动的执行，状态为 queued、inProgress、completed、
  failed 或 interrupted。
- `Item`：Turn 内按固定顺序出现的用户消息、agent 消息、reasoning、plan、tool call、file，
  以及内部 context patch / context compaction。
- `Interaction`：等待用户回答的 user input、tool approval 或 plan confirmation；它不是普通
 聊天 Item。

## 1.2 运行路径

```text
Flutter ThreadWorkspace
        ↕ typed FRB
StudioRuntime ── TaskService
        ↓
ThreadManager → ThreadActor → TurnEngine
        ↓              ↓ typed live notifications
       studio.sqlite
```

- `ThreadManager` 维护 Thread registry、父子关系和 spawn/close。
- 每个 `ThreadActor` 串行拥有一个 Thread 的输入队列、活动 Turn、取消句柄和 live Item overlay。
- `TurnEngine` 只负责模型采样、工具调用、interaction 等待和上下文压缩。
- `TaskService` 管理 TaskRun、worktree、delivery、review、merge、branch lease 与恢复，不修改
  Thread 状态机。
- `studio.sqlite` 是所有 durable Thread/Turn/Item/Interaction 与 Studio 产品事实的唯一数据库。

## 1.3 唯一事实源

| 事实 | 唯一拥有者 |
| --- | --- |
| Thread、Turn、Item、输入、Interaction | `studio.sqlite` |
| 活动 Turn、流式增量、steer、取消 identity、prompt generation | `ThreadActor` |
| Task/worktree/review/merge/lease | `TaskService` 对应表 |
| Composer、滚动、展开、订阅 generation | Flutter `WorkspaceUiState` |

不存在第二套 session/message/part projection、durable event journal、snapshot JSON 或双库
watermark。UI snapshot 由 canonical 表与活动 actor overlay 组成；历史只按 Turn keyset 分页。

## 1.4 Crate 边界

- `pl-protocol`：Thread/Turn/Item、Interaction、runtime 与 product wire 类型。
- `pl-trace`：模型和工具内部诊断事件，不作为 UI 协议或持久化事实源。
- `pl-model`：provider 与 transport 适配。
- `pl-core`：ThreadManager、ThreadActor、TurnEngine、通用工具与 agent control plane。
- `pl-studio-runtime`：单库 StudioStore、项目、Task、worktree、配置与产品事件。
- `pl-studio-bridge`：protocol 到 FRB DTO 的机械映射。
- `pure-studio`：ThreadWorkspace reducer、timeline、interaction、状态栏和设置 UI。

模块默认私有；产品层不能反写 Thread turn 状态，Flutter 不能从 Item、Interaction 或 Task
本地推断 canonical Turn 状态。

## 1.5 恢复原则

进程重启不能恢复物理模型连接。启动事务把遗留 inProgress Turn/Item 收束为
`interrupted(runtimeRestarted)`，重新排队未确认消费的明确输入，取消 tool approval，保留
user input 与 plan confirmation。活动 Task 没有 pending input 时保持 paused，由用户显式继续。

恢复、清理和归档永不猜测资源所有权，也不自动删除旧 Task worktree 或用户工作区。
