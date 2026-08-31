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

root TurnFactory 使用统一 `unified_root` 指令和 planner route。它解析 Thread 的动态 ModeId：无 active
run 时读取当前 Mode Skill winner，active/terminal run 的执行上下文使用 run 内冻结 snapshot。模型
上下文按固定 section 注入项目指令、Mode Skill 与（存在 active workflow 时）`pl.workflow`。

root 注册可选 `workflow_state`、统一 `complete` 和通用 collaboration/workspace tools。阶段只改变
constraint prompt，不能改变工具授权；所有 root turn 由 `complete` 形成统一完成边界。模式切换由
runtime 命令校验 idle、pending interaction 与 workflow lifecycle。

## 17.3 child 会话

`spawn_agent(profileId, task, ...)` 从可用 catalog 解析 Profile，生成时冻结 system instructions、
provider、model 与 effort，再创建普通 child Thread。系统和用户 Profile 使用同一 snapshot 类型。

child 可使用普通 workspace/command/collaboration tools，但不拥有根 workflow state tool。Profile 的
`unrestricted | directory | worktree` 模式决定有效 root/boundary 与项目内写策略。directory 策略只由
Pure 内置文件 mutation 工具强制执行，固定上下文必须声明 shell、Git、MCP 可绕过；worktree child 使用
产品层 durable lease，但不创建旧交付记录或自动整合。多个 Agent 的协调与成果采纳责任属于 root。

## 17.4 Interaction 与恢复

Thread 可以等待 `UserInput` 或 `ToolApproval`。响应进入同一 actor continuation，不存在 planner wake、
Task continuation 或专用计划确认状态。重启 activation 原子恢复 transcript、working state、pending
Interaction、Profile 与 workspace assignment；非法 session snapshot 产生通用 AgentState recovery issue。
worktree 的物理资源恢复另由 Studio durable lease 对账，身份不匹配时发布 Recovery issue 并保留现场。

## 17.5 生命周期

公开状态为 `Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling | Closing |
Closed | Faulted`。普通 close 释放热资源；worktree close 默认 preserve，只有显式 cleanup 才在精确
leaf 与 Pure branch 身份校验通过后清理。shutdown 先停止新输入，再中断/等待 Turn、flush checkpoint、
关闭协作实例和外部服务。所有 Agent 进程和 GUI 子进程都必须由宿主 ownership tree 回收。
