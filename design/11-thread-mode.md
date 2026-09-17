# 11 - Thread Mode 与预设工作流

本文是 Thread Mode 注册、图协议、workflow 工具契约与持久化上限的唯一权威源；协作编排（child
派发、交付证据、reviewer 门禁）见 [12](./12-collaboration.md)，Plan 确认状态机见
[13](./13-plan.md)，通用工具运行时见 [09](./09-tool-runtime.md)。

## 11.1 领域边界

Mode 是 root Thread 的执行配置。Mode 不是 Skill、Agent Profile、provider wire mode 或另一种
会话类型：Simple、Task 与后续自定义 Mode 继续使用同一套 Thread、Turn、模型循环和工具运行时；
是否使用工作流由注册 Mode 是否携带图决定。工作流只影响模型提示和合法状态边，不关闭文件、
命令、Git、Agent 或最终回复能力。所有 root Mode 统一通过 `complete` 工具提交完成事实并结束
Turn。

跨 crate 的稳定 ID 是 ThreadModeId，wire 使用 `mode.<id>` 字符串（内置值为 `mode.simple` 与
`mode.task`，未来自定义模式仍使用 `mode.<id>`）。ID 与目录 DTO 归 pl-protocol；内存注册表、
不可变快照、预设图编译和状态工具归 Studio 的 mode 模块；内置 Mode 随 Studio 二进制发布。
普通 Skill 系统不解析、发现、保护、投影或加载 Mode；Mode 不使用 SKILL.md、frontmatter、
Skill Provider、`skills_list`、`skill_view` 或 Skill 调用策略，也不进入普通 skills 目录。

## 11.2 注册与图编译

注册输入是拥有所有权的结构，包含 ID、展示名称、描述、排序、Mode Prompt 和可选
WorkflowDefinition；不经过 Skill frontmatter、调用策略或 Provider。Mode 管理器按来源原子替换
一批注册：先在锁外完成所有 ID、Prompt、重复项和工作流编译校验，全部成功后才在短写锁内发布
新的不可变目录快照；读取方取得快照后不再持锁。批次失败保留上一份完整快照。同一 ID 不能由
两个来源同时提供，内置来源不可被外部来源覆盖，其他来源之间不设 winner precedence——重复 ID
使整个注册批次失败。注册表不读取文件；未来文件加载器只能在边界层把文件解析为注册输入后调用
同一接口。结构 revision/hash 只覆盖规范化后的工作流图，Prompt 和展示元数据不参与图 hash。

### 图协议

工作流协议采用 W3C SCXML 与 XState 的扁平确定性状态机原则，并借鉴 Amazon States Language
在注册阶段完成静态图校验。Pure-Lang 只实现一个受限子集：每个 run 恰有一个活动 state、恰有
一个 initial state、final state 显式标注、transition 使用明确 source/target，guard 必须是无
副作用的声明性条件。协议不支持层级/并行 state、history state、entry/exit action、可执行
表达式或运行时解释器。这里的 guard 是提供给 Agent 判断并在调用转换工具时声明已满足的自然
语言条件，不是由 runtime 执行的脚本；显式 target 加 CAS 决定唯一转换，不依赖顺序选择第一条
可用边。

WorkflowDefinition 的 canonical 字段是 title、goal、initialStateId、states 与 transitions；
state 显式携带穷尽的 kind（atomic | final），transition 使用 sourceStateId、targetStateId 与
声明性 guard。限制与规则：

- 最多 32 个 state、96 条边，规范化 JSON 最大 64 KiB；state ID 只允许小写 ASCII、数字、
  `-`、`_`，长度 1–64。
- 非终态必须有指令与完成标准，且至少有一条出边；终态禁止出边。允许自环、循环和返工；
  同一 source/target 只能出现一次。
- 编译按固定次序规范化并拒绝非法图：唯一 state ID、唯一 initial、已知 source/target、无
  重复 transition、final state 无出边；从初态正向遍历拒绝不可达节点，从所有终态反向遍历
  拒绝无法终结的非终态。编译复杂度为 O(V + E)。

编译结果建立按 ID 和 source 索引的只读结构，保留声明顺序用于 UI，并从规范化协议数据计算
稳定 hash——数组顺序是 definition 的一部分，相同规范化定义必然得到相同 hash。编译器不执行
workflow，也不接受模型输入；模型没有定义、编译、patch 或 supersede 图的入口。

## 11.3 Turn 快照与 run 生命周期

root Turn 在调用 provider 前捕获一个 Mode 快照；本 Turn 的 Prompt、工具、图和状态校验全部
使用该快照，注册更新从下一个 root Turn 生效。child Thread 使用冻结的 Agent Profile，不创建、
继承或推进 root workflow。模式切换要求 root idle 且没有 pending interaction，runtime 负责归档
旧活动 run。

带工作流的 Mode 要求当前 root route 支持 function calling；Turn 准备阶段在 provider 请求前
显式拒绝不具备该能力的模型，不能为了迁就 hosted Web Search 的 exclusive 路径而静默卸载
workflow 工具。无图的 Mode 和 child Agent 不受这项约束。

只选择 Mode 不创建 run。带图的 Mode 在收到根用户输入且没有活动 run 时自动从初态创建 run；
用户输入与新 working state 在第一次 Thread checkpoint 一起提交，准备或提交失败不得调用
provider。进入 terminal 后，下一条根用户输入创建新 lineage。切换 Mode 会归档旧活动 run，但
不会在没有用户输入时创建新 run。

每个 run 只保存 Mode ID、图 hash、lineage/run ID、生命周期（active/terminal）、当前 state、
CAS revision、转换记录与归档关系，不保存 Mode Prompt 或完整图。下一个 Turn 若发现相同
Mode 的图 hash 已变化，先以 modeUpdated 归档旧 run，再以同 lineage 创建 replacement；仅
Prompt 变化时继续当前 run 并使用最新 Prompt。所选 ID 不再可用时在 provider 前返回类型化的
Mode 不可用错误，不得静默回退。

模型输入注入 `<preloaded_thread_mode_prompt>` 与由注册图和 run 派生的精简当前状态；完整图
按需通过工具读取。Prompt 不进入 Studio 数据库或 workflow working state。持久化结构变化
遵循 [17](./17-studio-storage.md) 的迁移契约；Mode/Workflow 历史形状在迁移边界转换，
运行时只使用当前结构。

## 11.4 持久化与上下文投影

`studio.workflow` Thread 扩展保存 WorkflowSessionState：单调 revision、当前 run、最近 16 个
归档摘要、滚动摘要以及最近 32 个成功 operation receipt。当前 run 保存 lineage/run id、
Mode ID、图 hash、生命周期、当前 state、时间和最近 64 条 transition。完整 typed state 最大
256 KiB——图由本 Turn 的 Mode 快照提供，旧细节由 Thread timeline/trace 审计，热状态只保存
有界尾部。

模型上下文不直接暴露完整 JSON：Studio 上下文投影派生保留 id `pl.workflow`，只包含当前
state 指令、完成标准、允许边、最近摘要和下一次 CAS 参数。workflow 状态变化后，下一模型步骤
把最新 `pl.workflow` 作为完整运行上下文快照追加到历史，不改写已发送前缀；相同内容不重复
追加；compaction 移除旧快照后重新补入当前 projection。

## 11.5 Workflow 工具合同

root-only 工具拆为六个，均实现 core 不透明 Tool 并注册到 root Thread；child 不注册 root 的
workflow 工具，因此不能替 root 改写 run：

- `workflow_current`：读取 Mode、run、图 hash、当前 state 与 CAS revision。
- `workflow_next`：读取当前 state 的直接出边及条件。
- `workflow_graph`：读取本 Turn 快照中的完整预设图。
- `workflow_history`：读取当前 run 的有序转换记录。
- `workflow_transition`：以 run/revision/state 三重 CAS 进入直接后继，表示完成当前阶段。
- `workflow_restart`：归档当前 run 并按同一预设图创建新 run。

四个查询工具为 Coexist 且不修改 revision；两个写工具与 `complete` 为 Solo——同一 response
中若还有任何其他调用则整批拒绝且无副作用。查询工具读取同一只读扩展快照；写工具从只读扩展
快照计算 CAS 候选，显式授予扩展更新权限并独占批次。runtime 校验 schema、图、CAS、合法直接
边、大小限制与 operation identity；guard、阶段完成标准和证据真实性由 Agent 判断。工具不
保存第二个 working set 或兼容注册器。

`workflow_transition` 的 mutation 输入只使用一份规范结构：run/revision/state/target CAS 位于
顶层，完成声明统一位于 `completion`，其内部包含 reason、summary 与 evidence。reason 不在
顶层，也不接受两种布局——转换原因与完成证据属于同一个领域对象，provider schema 与持久化
转换记录保持同构。规范形状为：

```json
{
  "expectedRunId": "run-...",
  "expectedRevision": 3,
  "expectedStateId": "working",
  "targetStateId": "integrating",
  "completion": {
    "reason": "所有实现 owner 已交付",
    "summary": "实现及聚焦验证完成",
    "evidence": ["focused tests passed"]
  }
}
```

稳定拒绝码为 `workflowNotStarted`、`runMismatch`、`staleRevision`、`stateMismatch`、
`modeSnapshotMismatch`、`invalidCompletion`、`invalidReason`、`transitionNotAllowed` 与
`operationIdentityConflict`；成功和幂等重放分别返回 `transitioned | restarted` 与
`alreadyApplied`。所有语义拒绝返回 `accepted: false`、稳定 code、最新 canonical snapshot、
constraint prompt 与 recovery actions，且无副作用。

幂等键是 `(turnId, callId, argumentHash)`：完全重放返回 `alreadyApplied`；同一 turn/call
使用不同参数返回 identity conflict。成功 operation receipt 最近保留 32 条。

成功 mutation 由 assistant tool call、tool result 与 working state 的统一 Thread checkpoint
原子提交（见 [09](./09-tool-runtime.md)）；提交失败保留完成对象用于重试，不再次执行副作用。
transition result 内含最新 stage constraint，供同一 Turn 后续 inference 使用；下一 Turn 从
working state 派生 `pl.workflow`，context compaction 后重新捕获最新 projection，不复用压缩前
阶段。

Task Mode Prompt 规定每次 `workflow_transition` 前必须产生一个独立的只读 tool response，
同时调用 `workflow_current` 与 `workflow_next`，并只使用这次返回的 run、revision、current
state 和直接后继；不得从注入摘要或旧 mutation receipt 推测 CAS。首次和进入终态前的读取还
必须包含 `workflow_graph` 与 `workflow_history`。只读查询可以并发，mutation 仍必须单独占用
下一次 tool response。

## 11.6 内置 Mode

`mode.simple` 不带 workflow，也不要求阶段转换：它直接工作、按风险验证，并在完成时调用
`complete`；不增加 Git、固定审查轮次或交付门禁。

`mode.task` 注册预设图：

```text
planning -> editing_documents
editing_documents -> working
working -> integrating
integrating -> working | reviewing
reviewing -> working | editing_documents | completed
所有非终态 -> stopped
completed、stopped 为终态
```

`completed` 与 `stopped` 都是无任何出边的 final state，停止边只从非终态进入 `stopped`。代码
finding 回到 working，设计 finding 回到 editing_documents；两条返工路径都必须重新经过
integrating 和 reviewing。状态指令、完成标准和每条边的 guard 属于图；协作、工具使用与角色
约束属于 Mode Prompt（完整编排合同见 [12](./12-collaboration.md)）。

完整计划必须在 planning 中通过 `plan_current`、`plan_next` 和 `plan_submit` 请求批准或修订；
只有 `plan_current` 返回 `approved` 后才可 transition 到 editing_documents。计划确认不属于
Mode 图：它由 Plan 固定状态机和整套 `plan_*` 工具管理（见 [13](./13-plan.md)），planning
期间的澄清、提交、要求修订和重新批准都保持 workflow state 为 `planning`，Plan 已批准是
`planning -> editing_documents` 的声明性条件。进入 `completed` 后调用 `complete`。

## 11.7 Studio 与 GUI

GUI 的 Mode selector 消费独立的 Mode 目录快照。Thread 状态栏只显示 Mode 与当前状态；不提供
完整图、历史详情、展开面板或人工状态切换，Flutter 不从 state ID 推演状态；Driver 从
canonical snapshot、工具回执和 wire 读取完整历史。

工具目录刷新捕获 Mode 扩展身份和扩展水位：异步准备后的发布必须条件验证该水位，防止旧
Mode 目录覆盖已完成切换的新工具；过期发布不记为已安装、不清空当前目录，关闭候选后安排
重新准备。刷新指纹包含 Mode 扩展身份；模型/工具执行期间发生的普通状态事件不单独触发目录
重建（通用条件发布机制见 [01](./01-overview.md)）。
