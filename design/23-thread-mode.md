# 23 - Thread Mode

## 23.1 领域边界

Mode 是 root `Thread` 的执行配置，源码统一归属 `thread` 命名空间。Mode 不是 Skill、Agent Profile、
provider wire mode 或另一种会话类型；Simple、Task 与后续自定义 Mode 继续使用同一套
Thread、Turn、模型循环和工具运行时。

跨 crate 的稳定 ID 是 `ThreadModeId`，wire 仍使用 `mode.<id>` 字符串。`pl-protocol::thread::mode`
拥有 ID 与目录 DTO；`pl-core::thread` 拥有内存注册表、不可变快照、预设图编译和状态工具；
`pl-studio-runtime::studio::thread::mode` 拥有随二进制发布的内置 Mode。

普通 Skill 系统不解析、发现、保护、投影或加载 Mode。Mode 不使用 `SKILL.md`、frontmatter、
Skill Provider、`skills_list`、`skill_view` 或 Skill 调用策略。

## 23.2 注册表

`ThreadModeRegistration` 是拥有所有权的注册输入，包含 ID、展示名称、描述、排序、Mode Prompt 和
可选 `WorkflowDefinition`。`ThreadModeManager` 按来源原子替换一批注册：先在锁外完成所有 ID、
Prompt、重复项和工作流编译校验，全部成功后才在短写锁内发布新的不可变
`ThreadModeCatalogSnapshot`。读取方取得 `Arc` 快照后不再持锁，也不跨 `.await` 访问可变目录。

同一 ID 不能由两个来源同时提供；内置来源不可被外部来源覆盖。批次失败保留上一份完整快照。
结构 revision/hash 只覆盖规范化后的工作流图，Prompt 和展示元数据不参与图 hash。注册表不读取文件；
未来文件加载器只能在边界层把文件解析为 `ThreadModeRegistration` 后调用同一接口。

内置 Mode 使用 `&'static str` 与静态 state/transition 切片组成的 const-friendly 描述，Studio 启动时转换成
拥有所有权的注册输入并发布。内置资源不写入用户目录、不物化到系统 Skill 目录、不保存旧版本；
二进制版本就是内置 Prompt 与图的版本。

## 23.3 Turn 与持久化

root TurnFactory 在调用 provider 前捕获一个 Mode 快照。本 Turn 的 Prompt、工具、图和状态校验全部
使用该快照；注册更新从下一个 root Turn 生效。child Thread 使用冻结的 Agent Profile，不创建、继承
或推进 root workflow。

带工作流的 Mode 要求当前 root route 支持 function calling；Turn 准备阶段在 provider 请求前显式拒绝
不具备该能力的模型，不能为了迁就 hosted Web Search 的 exclusive 路径而静默卸载工作流工具。无图的
Mode 和 child Agent 不受这项约束。

只选择 Mode 不创建 run。带图的 Mode 在收到根用户输入且没有活动 run 时自动从初态创建 run；
用户输入与新 working state 在第一次 Thread checkpoint 一起提交，准备或提交失败不得调用 provider。
进入 terminal 后，下一条根用户输入创建新 lineage。切换 Mode 会归档旧活动 run，但不会在没有用户
输入时创建新 run。

每个 run 只保存 Mode ID、图 hash、lineage/run ID、生命周期、当前 state、CAS revision、转换记录与
归档关系，不保存 Mode Prompt 或完整图。下一个 Turn 若发现相同 Mode 的图 hash 已变化，先以
`modeUpdated` 归档旧 run，再以同 lineage 创建 replacement；仅 Prompt 变化时继续当前 run 并使用
最新 Prompt。所选 ID 不再可用时在 provider 前返回 `ThreadModeUnavailable`，不得静默回退。

模型输入注入 `<preloaded_thread_mode_prompt>` 与由注册图和 run 派生的精简当前状态；完整图按需通过
工具读取。Prompt 不进入 Studio 数据库或 workflow working state。

Studio 数据库 schema 18 是破坏性边界：旧 schema 继续按统一数据库不兼容规则重建，不维护 Mode、
Workflow 或旧 Skill 形状的迁移与双读。

## 23.4 预设图与状态工具

工作流协议采用 [W3C SCXML](https://www.w3.org/TR/scxml/) 与
[XState](https://stately.ai/docs/transitions) 的扁平确定性状态机原则，并借鉴
[Amazon States Language](https://states-language.net/spec.html) 在注册阶段完成静态图校验。Pure-Lang
只实现一个受限子集：每个 run 恰有一个活动 state、恰有一个 initial state、final state 显式标注、
transition 使用明确 source/target，guard 必须是无副作用的声明性条件。协议不支持层级/并行 state、
history state、entry/exit action、可执行表达式或运行时解释器。

`WorkflowDefinition` 的 canonical 字段是 `initial_state_id`、`states` 与 `transitions`；state 使用
`WorkflowStateKind::Atomic | Final`，transition 使用 `source_state_id`、`target_state_id` 和
`guard`。这里的 guard 是提供给 Agent 判断并在调用转换工具时声明已满足的自然语言条件，不是由
Runtime 执行的脚本。显式 target 加 CAS 决定唯一转换，因此不依赖顺序选择第一条可用边。

注册编译按固定次序规范化并拒绝非法图：ID/文本与数量上限、唯一 state ID、唯一 initial、已知
source/target、重复 transition、final state 无出边、所有 state 可从 initial 到达、所有非 final state
至少有一条出边且可到达某个 final state。编译结果建立按 ID 和 source 索引的只读结构，并从规范化
协议数据计算稳定 hash；Prompt 与展示元数据不参与。编译器不执行 workflow，也不接受模型输入。

Simple 只有 Prompt，不安装工作流工具。Task 的内置图为：

```text
planning -> editing_documents
editing_documents -> working
working -> integrating
integrating -> working | reviewing
reviewing -> working | editing_documents | completed
所有非终态 -> stopped
completed、stopped 为终态
```

状态指令、完成标准和每条边的 guard 属于图；协作、工具使用与角色约束属于 Mode Prompt。图在注册时由
框架编译，模型不能提交、patch、compile 或 supersede definition。

计划确认不属于 Mode 图。它由 `session::plan` 的固定状态机和整套 `plan_*` 工具管理；planning 期间的
澄清、提交、要求修订和重新批准都保持 workflow state 为 `planning`，Plan 已批准是
`planning -> editing_documents` 的声明性条件。完整合同见
[24-agent-session-plan.md](24-agent-session-plan.md)。

root 工作流工具拆为：

- `workflow_current`：读取 Mode、run、图 hash、当前 state 与 CAS revision；
- `workflow_next`：读取当前 state 的直接出边及条件；
- `workflow_graph`：读取本 Turn 快照中的完整预设图；
- `workflow_history`：读取当前 run 的有序转换记录；
- `workflow_transition`：以 run/revision/state CAS 进入直接后继；
- `workflow_restart`：归档当前 run 并按同一预设图创建新 run。

查询工具为 `Coexist` 且不改变 revision；写工具为 `Solo`。语义条件由 Agent 判断，Runtime 只验证
直接边、图 hash、CAS、大小和 operation identity。成功 mutation 仍由 tool call、tool result 与
working state 的统一 checkpoint 原子提交；持久化 receipt 尾部中同一 operation 和参数重放返回已有
receipt，不重复转换，相同 identity 携带不同参数则拒绝。
六个工具都实现统一的 `StaticTool` 合同，并作为一个 `ToolInstallGroup` 注册到当前 root Agent 的
`AgentToolSet`；查询通过 `ToolPolicy::read_only().with_parallel_tool_calls()` 冻结为共享执行，mutation
通过 `AgentControl + Solo` 冻结为独占执行，不保留旧 `Tool` trait 或第二套注册器。

`workflow_transition` 的 mutation 输入只使用一份规范结构：run/revision/state/target CAS 位于顶层，
完成声明统一位于 `completion`，其内部包含 `reason`、`summary` 与 `evidence`。`reason` 不在顶层，也不
接受两种布局；这样转换原因与完成证据属于同一个领域对象，provider schema 与持久化转换记录保持
同构。规范形状为：

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

Task Mode Prompt 规定每次 `workflow_transition` 前必须产生一个独立的只读 tool response，同时调用
`workflow_current` 与 `workflow_next`，并只使用这次返回的 run、revision、current state 和直接后继；
不得从注入摘要或旧 mutation receipt 推测 CAS。首次和进入终态前的读取还必须包含
`workflow_graph` 与 `workflow_history`，从而让真实 provider 路径同时覆盖注册图、历史、可达边和转换。
只读查询可以并发，mutation 仍必须单独占用下一次 tool response。

## 23.5 Studio 与验收

GUI 的 Mode selector 消费独立 `ThreadModeCatalogSnapshot`。Thread 状态栏只显示 Mode 与当前状态；
不提供完整图、历史详情、展开面板或人工状态切换，Flutter 不从 state ID 推演状态。

确定性测试覆盖批次原子性、来源冲突、内置 const 转换、合成内存 Mode、Prompt-only 更新、图更新、
首次自动启动、Mode 切换、终态新 lineage、非法边、CAS、幂等和移除。真实验收使用临时 Studio home、
临时 Git 项目、原生 GUI Driver 与真实 provider，必须覆盖：向用户提问、用户要求补充和修改计划、
重新批准、至少两个互斥 directory child 与至少两个独立 worktree child 并行、显式整合、只读 reviewer、
终态和完整清理。验收保留 provider wire、interaction/plan revision、child/workspace ledger、工作流回执、
截图、render tree、最终 diff、worktree/branch 清理和进程树回收证据；不得 fallback 到 scripted provider。
