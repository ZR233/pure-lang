# 17 - Thread Runtime 与 Agent 宿主

## 17.1 统一宿主

```text
ThreadManager
  ├─ ThreadDirectory + ToolManager + AgentProfileCatalog
  └─ ThreadHandle → ThreadActor → RunningTurn → TurnEngine
```

ThreadActor 唯一拥有 revision、input queue、活动 Turn、live Item overlay、prompt generation 与 typed
working state。root 和 child 都走同一 actor/engine；差异来自预加载指令、Profile snapshot、冻结的
workspace assignment 和工具集合。assignment 与 Profile 一起写入 canonical session，TurnFactory 只消费
热状态，不根据最新设置重算，也不在每轮回读 SQLite。

## 17.2 root 会话

root TurnFactory 使用统一 `unified_root` 指令和 planner route。它以 Thread 的 `ThreadModeId` 从
`ThreadModeManager` 捕获不可变快照，并在 provider 前协调 run 与图 hash。模型上下文按固定 section
注入项目指令、Mode Prompt 与（存在 workflow 时）`pl.workflow`。

root 注册可选的拆分 workflow 工具、统一 `complete` 和通用 collaboration/workspace tools。阶段只改变
constraint prompt，不能改变工具授权；所有 root turn 由 `complete` 形成统一完成边界。模式切换由
runtime 命令校验 idle、pending interaction 与 workflow lifecycle。

## 17.3 child 会话

`spawn_agent(profileId, task, ...)` 从可用 catalog 解析 Profile，生成时冻结 system instructions、
provider、model 与 effort，再创建普通 child Thread。系统和用户 Profile 使用同一 snapshot 类型。

child 可使用普通 workspace/command/collaboration tools，但不拥有根 workflow 工具。Profile 的
`unrestricted | directory | worktree` 模式决定有效 root/boundary 与项目内写策略。directory 策略只由
Pure 内置文件 mutation 工具强制执行，固定上下文必须声明 shell、Git、MCP 可绕过；worktree child 使用
产品层 durable lease，但不创建旧交付记录或自动整合。多个 Agent 的协调与成果采纳责任属于 root。
产品 lifecycle 的 spawn request 同时携带 session 中已经冻结的 typed Profile snapshot，供容器、模型
路由等外部资源准备直接消费；产品不得从父 Agent 或当前设置重建另一份 Profile。

## 17.4 Interaction 与恢复

Thread 可以等待 `UserInput` 或 `ToolApproval`。响应进入同一 actor continuation，不存在 planner wake、
Task continuation 或专用 Plan Interaction kind。计划确认仍是通用 `UserInput`，但 typed purpose 将其
绑定到独立 `session::plan` 状态机；pending、Plan state 与 resolution continuation 在 actor owner commit
中保持原子。重启 activation 原子恢复 transcript、working state、pending
Interaction、Profile 与 workspace assignment；非法 session snapshot 产生通用 AgentState recovery issue。
worktree 的物理资源恢复另由 Studio durable lease 对账，身份不匹配时发布 Recovery issue 并保留现场。

Mailbox metadata 在热状态中保持 typed value tree。产品 host 必须能借用读取 object、array 与 scalar，
例如解析一组 Skill mentions 或其它产品输入提示，而无需把热状态重新序列化成 `serde_json::Value`。
这里约束的是“typed 结构可遍历”的公共能力，不锁定 `as_array` 等具体方法名；后续即使更换 value 类型
或 accessor，也必须保留等价的无 JSON round-trip 读取语义。否则产品会被迫复制 wire 解析、丢失类型
边界，并重新引入 runtime 内的非类型化兼容路径。

TurnFactory 必须能在返回冻结的 `PreparedAgentTurn` 时为本轮选择 wall-clock 预算。原因是 Review、
批处理等产品会话可能需要不同于 PL 默认值的运行窗口，而模型、工具、等待、用量和终态仍必须由同一
TurnEngine 统一编排。公共 Rust 接口的名称可以演进，但“产品宿主为单轮提供 typed 预算，PL 负责
执行并产生 `BudgetLimited` 与 rollover”的能力语义必须保留；不得删除该能力后迫使产品复制 Turn
循环、通过外层 watchdog 模拟预算，或把预算塞进非类型化 metadata。预算在 TurnFactory 返回时一次性
冻结，活动 Turn 不因配置热更新改变上限；mailbox 的 budget refresh 只开始新的预算 tranche，不改写
已经冻结的上限。

root 的 `BudgetLimited` 继续由 PL 尝试 rollover。child 的预算终态不执行 rollover：actor 在同一个
settlement commit 中保存 `TurnRolloverOutcome::NotAttempted`、进入带预算快照的 idle pause，并阻止
pending mailbox 自动启动。父 Agent 的下一条显式输入同时清除 pause、按 FIFO 选择 Turn 输入并取得
新预算；pause 经过恢复仍保持，且不能被 LRU 当作普通可淘汰 idle。

## 17.5 生命周期

每轮独立拥有事件通道与待处理事件。结束时停止并等待生产者，处理合法事件并在内存提交唯一终态，
再启动下一轮；旧轮次事件不得进入新轮次。协议错误终结当前轮次并明确报告，未保存事实由独立
冷存储缓冲保留，不参与实时收束。持久化健康状态不决定 Thread 是否接受输入或执行工具。

公开状态为 `Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling | Closing |
Closed | Faulted`。普通 close 释放热资源；worktree close 默认 preserve，只有显式 cleanup 才在精确
leaf 与 Pure branch 身份校验通过后清理。shutdown 先停止新输入，再中断/等待 Turn、flush checkpoint、
关闭协作实例和外部服务。所有 Agent 进程和 GUI 子进程都必须由宿主 ownership tree 回收。
