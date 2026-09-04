# 21 - 会话激活、热状态与异步持久化

## 21.1 Activation 与唯一热状态

Thread 是唯一会话 owner。用户选择 Thread、向冷 Thread 提交输入或后台 Agent 继续执行时，运行时
通过显式 activation command 在同一个一致读视图中加载版本化 working state、有效 transcript、
最近 Timeline 窗口、pending Interaction 与活动 Turn。所有校验完成后才一次性安装
`ActiveSessionState`，不得发布半恢复状态。

activation 完成后，类型化 Rust 内存对象是唯一运行时事实源。模型请求、拆分后的 workflow 工具、
GUI snapshot 与普通工具不得再次回读 SQLite 构造活动状态。SQLite 仅负责冷恢复、异步落库以及
用户主动向前翻 Timeline 时的 keyset 冷分页。

## 21.2 Working state 与 checkpoint

`AgentWorkingState` 可包含 `WorkflowSessionState` 和冻结的 `AgentWorkspaceAssignmentSnapshot`。
Thread working state 只持久化 Mode ID、图 revision/hash、当前状态、转换尾部、归档关系与幂等
receipt，不保存 Mode prompt 或完整图；恢复时以 `ThreadModeManager` 当前 snapshot 重新绑定图。
这些轻量事实随 Thread working state 原子持久化，不新增工作流业务表。完整状态上限、历史归档和
digest 规则由 [16-task-orchestration.md](./16-task-orchestration.md) 定义。

热状态只持有领域对象，不持有 SeaORM entity 或数据库 DTO。一次 provider response 的 assistant
tool call、tool result 与新 working state 必须在同一 Thread checkpoint 中提交；checkpoint 失败时
三者共同回滚。模型下一 Turn 使用从 canonical working state 派生的 `pl.workflow` section，
上下文压缩后重新捕获最新 projection。

Mailbox metadata 同样使用递归 typed value；只有 repository DTO 与 provider/tool wire 边界执行
JSON 转换。write-behind queue 保存不可变 typed snapshot，worker 才负责编码、hash 和 SQLite
transaction。worker panic 必须发布 `Blocked` 并保留待提交事实。

## 21.3 提交、批量与释放

每次 mutation 必须先被 writer 原子接受，再替换 owner 热 snapshot 并广播。接受失败时业务状态、
revision 与事件均不得变化。writer 的五秒最大延迟不被后续写入重置；累计 64 条、显式 flush、
owner 淘汰、shutdown、上下文 replacement 或不可逆外部动作触发立即提交。

`workflow_transition` 与 `workflow_restart` 使用 `ToolBatchPolicy::Solo`。同一 provider response
中若 mutation 与任何第二个工具并存，整批在产生副作用前拒绝；`workflow_current`、
`workflow_next`、`workflow_graph` 与 `workflow_history` 是可并发的只读查询。Solo 只约束状态
mutation 批次，不限制任何阶段之后可使用的文件、命令、Git、Agent 或回复能力。

模型热 Context 从 transcript segments 重建，不单独持久化。未完成 assistant 流式草稿不进入
transcript；崩溃恢复时遗留 running Turn 收束为 interrupted/cancelled。Timeline 采用独立热窗口，
冷页先转为领域对象再合并热状态。

协作工具对 child 的完整会话诊断使用 durable Timeline keyset 分页，不读取或伪装 provider transcript。
驻留目标先在不持 actor 锁的情况下等待当前 runtime revision 落库，再从 repository 读取；冷目标直接
读取持久层，因此关闭、淘汰和重启不会截断可查范围。cursor 绑定目标、排序、详细级别和首次查询水位，
后续追加不能改变既有分页窗口；该只读查询不得激活目标或向 ThreadEventBus 合并冷页。

## 21.4 驻留

当前选中 Thread、存在活动 Turn 或 pending Interaction 的 Thread，以及仍有运行中子 Agent 的根
Thread 会被 pin。GUI 切换不能淘汰后台工作。未选中且无活动工作的 Thread 进入 LRU；淘汰前必须
通过 durability barrier。工作流阶段本身不是“正在执行”的证明，idle 判断聚合整棵 Agent tree 的
活动 Turn。

## 21.5 验收

确定性测试覆盖 activation、checkpoint 共同提交/回滚、Solo 无副作用拒绝、同 Turn 与下一 Turn
阶段提示、压缩后 projection、CAS、幂等 replay、Thread Mode、Agent Profile/workspace assignment 冻结和
重启恢复。

真实验收入口为：

```text
cargo xtask verify-workflow --live --headless
cargo xtask verify-workflow --live --gui
```

`--live` 是真实凭据与费用门禁，不允许 scripted/demo provider 或 fallback。GUI 路径通过
`cargo xtask run-gui --driver --log-level debug` 启动原生 Studio，在无 `.git` 的临时 Rust 项目中
完成计划展示、通用用户确认、文档更新、实施、验证、复核、`completed` 终态与重启恢复。

wire capture 必须分别包含注册的 `mode.simple`、`mode.task`、统一 `complete`、普通工作区与通用协作工具；
任务模式的首个 provider 请求必须已经包含 Thread Mode prompt、初始 `planning` projection 和六个
`workflow_*` 工具 schema，且没有任何工作流调用或结果；后续请求必须包含真实查询/转换调用、结果
和 `pl.workflow`。简洁模式不得包含 workflow 调用或投影；任何请求都不得包含图编译指令、旧
planner/Task prompt、`task_*`、WorkUnit、review 或 merge 工具。
artifact 写入 `target/workflow-live-artifacts/`，只保留脱敏协议、
workflow snapshot、日志、截图、文件 diff、验证输出与进程树，不记录认证头。
