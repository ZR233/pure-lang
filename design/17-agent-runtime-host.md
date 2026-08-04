# 17 - Thread Runtime 与产品宿主

## 17.1 Runtime 结构

```text
ThreadManager
  ├─ ThreadDirectory watch
  └─ ThreadHandle → ThreadActor → RunningTurn → TurnEngine
```

ThreadManager 管理 registry、容量和 spawn/close。ThreadHandle 查表后把 start、steer、interrupt、
snapshot 和 progress 命令直接发给目标 ThreadActor。只有 spawn/close 修改全局目录。

ThreadActor 唯一拥有 Thread revision、durable input queue 的内存镜像、活动 RunningTurn、取消
identity 和 live Item overlay。它不缓存完整历史，也不拥有 Task/worktree。

## 17.2 Host 端口

pl-core 只保留三个窄端口：

- `ThreadRepository`：以 expected revision 在单库事务中提交 Thread/Turn/Item/Input/Interaction
  mutation，并读取启动恢复所需状态。
- `TurnFactory`：准备 TurnEngine、request、instructions、tools 与 execution policy。
- `ChildLifecycle`：为 child Thread 准备/释放产品外部资源；Task 实现可以拒绝不安全的 close。

通知由 pl-core 在 repository 事务成功后直接发布，不经过额外 durable projection 或 replay
journal。Task tool 自己事务性写 TaskService；core 不携带 product mutation。

## 17.3 取消与恢复

RunningTurn 包含 turnId、进程内 identity、CancellationToken、abort handle、done 和 steer sender。
completion 必须同时匹配 turnId 与 Arc identity。interrupt 先触发 token，等待一秒清理，超时才
abort；终态数据库事务成功后才能广播 turnCompleted。

重启无法恢复物理连接。repository 在 manager 启动前收束遗留 active Turn/Item、恢复 queued
input 和 pending Interaction；manager 只创建 idle ThreadActor。任何恢复路径都不自动执行模型。

## 17.4 Agent control plane

模型工具名继续使用 spawn_agent、send_message、interrupt_agent、list_agents、wait_agents、
read_agent_session 和 close_agent；它们以 agentPath 解析 ThreadId。Thread directory 保存
root/parent/role/path/status/progress，不保存第二份 timeline 或 last turn outcome。

`wait_agents` 订阅 directory watch 后重读 snapshot，只因 progress、interaction 或 terminal
变化返回；没有 timer、轮询或自动续轮。child 内部 Item 只进入 child Thread。
