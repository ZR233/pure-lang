# 12 - 方案乙实施规范

## 12.1 目标与范围

方案乙以一次原子切换统一 Studio 会话架构：

```text
Thread → Turn → Item
```

不保留旧命令兼容层、旧 DTO 解析门面、运行期双库、durable event journal 或长期 feature
flag。Simple、Task、多代理、worktree、审查、冲突处理与重启恢复必须在新模型上继续工作。

## 12.2 边界

- `pl-protocol` 定义穷尽的 Thread、Turn、Item、Interaction 和 typed notification。
- `pl-core` 提供 ThreadManager、ThreadActor、TurnFactory/TurnEngine 与协作工具控制面。
- `pl-studio-runtime` 实现单库 repository、TaskService、产品流、破坏性 schema 重建和恢复。
- `pl-studio-bridge` 机械映射 typed Rust/FRB DTO，不加入兼容 parser 或第二次状态分桶。
- Flutter data/repository 把 DTO 一次转换为 domain workspace；ViewModel/reducer 管理 canonical
  workspace 与 UI ephemeral state；Widget 只负责展示和交互。

公开会话命令固定为 `listThreads`、`readThread`、`listThreadTurns`、`startTurn`、
`steerTurn`、`interruptTurn`、`respondInteraction` 和 `subscribeThread`。

## 12.3 身份与事实归属

一个 agent 固定对应一个 Thread，`ThreadId` 同时是运行时和会话身份；`agentPath` 只用于工具
寻址。root/child 关系由 `rootThreadId`、`parentThreadId`、`role` 和 `agentPath` 表达。

事实只能有一个拥有者：

- SQLite：Thread、Turn、Item、input、interaction、attachment 与 Task 产品表。
- ThreadActor：durable input queue 内存镜像、活动 Turn、取消 identity、steer mailbox 和 live
  Item delta。
- TaskService：Task phase、WorkUnit、Delivery、ReviewRound、MergeRecord、BranchLease 与清理合同。
- Flutter UI：Composer 草稿、滚动、展开状态、submission revision 和订阅 generation。

TaskRun 只绑定 root Thread；WorkUnit 和 ReviewRound 直接引用 executor/reviewer Thread。Task 状态
从产品表与 Thread/Turn 事实组成，不创建 AgentOutcome 或 runtime progress 镜像。

## 12.4 存储与切换

Studio 只使用 `studio.sqlite` schema v2。每次 durable transition 在一个 SQLite 事务中校验
revision 并更新相关 canonical 行；失败不更新 actor，也不广播。

启动发现 canonical 库版本、结构或完整性不兼容时，关闭检查连接，精确删除该数据库与
`-wal/-shm` 后创建空 schema；不迁移、不备份、不导入旧会话或 Task。该流程不触碰其他旧库、
attachments、Project、worktree 或 branch。`config.toml` 只接受 schema 14，旧 schema 不迁移。

## 12.5 运行时、恢复与订阅

ThreadHandle 把普通命令直接发给目标 ThreadActor；ThreadManager 只管理 registry、spawn/close
和 directory watch。TurnFactory 直接返回可执行 TurnEngine、request 与 policy。

保留稳定 mail ID、CAS revision、迟到 completion identity、取消 grace period、显式
`wait_agents` 与事件驱动 directory watch。模型上下文从有序 Item 重建，最新
`contextCompaction` 是基线；provider 私有 Item 不向 Flutter 暴露。

重启时遗留活动 Turn/Item 收束为 `interrupted(runtimeRestarted)`，未确认消费的显式 input
重新排队。`toolApproval` 取消，`userInput` 与 `planConfirmation` 保持 pending。attach 不自动
启动模型；没有 pending input 的活动 Task 显示 paused，由用户显式继续。

`subscribeThread` 先注册监听，再返回 authoritative snapshot。之后只发送 typed Turn、Item、
Interaction 与 runtime notification。流式 delta 和 terminal 通知必须 lossless；普通 progress
可以 best-effort，但丢弃前发送 `Lagged`。断流、lag 或未知 revision 一律重新订阅并用 snapshot
替换；实时流不携带 durable cursor 或 replay journal，历史只用 opaque keyset cursor 分页。

## 12.6 Flutter 状态合同

Flutter canonical state 只保存 Thread directory 与 `workspacesByThread`。snapshot 直接替换目标
workspace；旧 generation 的通知直接丢弃。Timeline 只从 ThreadItem ordinal 投影，相邻工具合并
属于视觉层，不创建第二套 Message/Part 事实源。

`selectedThreadId` 是唯一选择状态，root 由 `rootThreadId` 派生。Timeline、Todo、状态栏、
interaction 和 Composer 作为一个 workspace 原子切换，同时保留该 Thread 的 UI ephemeral state。

## 12.7 验收

- 协议：serde/FRB/Dart union 穷尽映射，未知变体失败。
- 存储：不兼容库精确删除重建、单库建库、CAS、输入幂等、ordinal、keyset 分页和事务原子性。
- Runtime：start/steer/FIFO、取消、迟到 completion、重启、interaction、child Thread 与无轮询等待。
- Task：计划确认、并行 executor、delivery/review/rework、Planner 自主 Git、merge 记账、
  stop/restart/cleanup 与 lease。
- Flutter：Item timeline、reasoning、tool grouping、Composer revision、interaction dock、原子切换与
  UI ephemeral state。
- 真实应用：使用隔离数据目录运行 `cargo xtask run-gui --driver`。Driver 模式直接连接应用 VM
  Service，不启动额外 DDS 中间层；验收覆盖 Driver health、输入回读、`studio.sqlite`、截图和零
  runtime errors。最终 completed snapshot 必须逐 WorkUnit 校验 completion/merge revision 对应关系、
  `cleanupStatus = discarded | alreadyAbsent`、worktree leaf 已缺失、integrated pass HEAD 一致，且 root
  没有 active turn 或 pending interaction；任何 cleanup failed 不能只凭 phase=completed 通过 Driver。
  GUI 首次 Rust/Flutter 构建使用独立的 startup timeout，不消耗 plan/task/stall timeout；启动失败也要
  保存当时仍存活的完整构建/GUI 进程树。

最终质量门为 Rust fmt/严格 Clippy/默认并行 workspace tests、FRB 无漂移、Flutter analyze/tests
和 `git diff --check`。
