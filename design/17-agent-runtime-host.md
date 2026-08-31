# 17 - Thread Runtime 与 Agent 宿主

## 17.1 统一宿主

```text
ThreadManager
  ├─ ThreadDirectory + ToolManager + AgentProfileCatalog
  └─ ThreadHandle → ThreadActor → RunningTurn → TurnEngine
```

ThreadActor 唯一拥有 revision、input queue、活动 Turn、live Item overlay、prompt generation 与 typed
working state。root 和 child 都走同一 actor/engine；差异只来自预加载指令、Profile snapshot 和工具
集合。SQLite write-behind 跟随 owner checkpoint，不能反向覆盖活动 snapshot。

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

child 可使用普通 workspace/command/collaboration tools，但不拥有根 workflow state tool。宿主不为 child
创建 Git branch、worktree 或交付记录；多个 Agent 修改同一 workspace 的协调责任属于 root。

## 17.4 Interaction 与恢复

Thread 可以等待 `UserInput` 或 `ToolApproval`。响应进入同一 actor continuation，不存在 planner wake、
Task continuation 或专用计划确认状态。重启 activation 原子恢复 transcript、working state、pending
Interaction 和活动目录；非法 session snapshot 产生通用 AgentState recovery issue。

## 17.5 生命周期

公开状态为 `Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling | Closing |
Closed | Faulted`。shutdown 先停止新输入，再中断/等待 Turn、flush checkpoint、关闭协作实例和外部
服务。所有 Agent 进程和 GUI 子进程都必须由宿主 ownership tree 回收。
