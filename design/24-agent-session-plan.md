# 24 - Thread Plan

## 24.1 领域边界

Plan 是 Thread 内独立于 Mode workflow 的计划确认聚合，源码统一归属
`pl-protocol::agent_session::plan` 与 `pl-studio-runtime::plan_tool`。`thread` 的 Mode 状态图描述一项任务的阶段；
Studio Plan 状态机 只描述当前 Thread 的一份计划从起草、请求确认、要求修订到批准的生命周期。Mode 图不能定义、编译、
覆盖或直接推进 Plan 状态，Plan 工具也不能提交外部图。

Plan 内核是唯一的 `AgentSessionPlanMachine` 对象。协议类型只保存状态和审计事实，工具、Interaction、
Prompt 与产品宿主都必须把命令交给该对象决定和应用，不能各自复制状态判断。状态机采用 SCXML/XState
的显式初态、合法事件和 guard 原则，但它是代码内置的封闭机器，不使用 `WorkflowDefinition`、运行时图
解释器或注册表。

## 24.2 固定状态与转换

Plan 使用以下穷尽状态：

```text
drafting -> awaitingConfirmation             plan_submit
revisionRequested -> awaitingConfirmation    plan_submit
awaitingConfirmation -> approved             用户 Approve
awaitingConfirmation -> revisionRequested    用户 Revise 或补充修改意见
approved -> drafting                         plan_restart
revisionRequested -> drafting                plan_restart
```

`drafting` 是初态；`approved` 表示当前文档已批准，但可用 `plan_restart` 显式开始新的计划 lineage。
`awaitingConfirmation` 期间不能 restart 或再次 submit，以免遗留可回答但已失去 owner 的 pending
Interaction。`revisionRequested` 可以直接提交完整修订版，也可先 restart 清空旧文档。

每条转换的 source、target、命令、条件和恢复动作由 `AgentSessionPlanMachine` 的固定规则表提供。
`AgentSessionPlanState` 不是开放字符串，所有 match 必须穷尽；新增状态或转换必须同时修改状态机、协议测试、
Prompt 和真实验收，不能只修改提示词。

## 24.3 状态、CAS 与幂等

每个 Thread 的 `studio.plan` Thread 扩展 保存有界 `AgentSessionPlanState`：状态 revision、当前状态、计划文档及内容 hash、
文档 version、pending Interaction ID、最近修订意见、转换历史和最近 operation receipts。缺失 Plan 字段
等价于 revision 0 的 `drafting`，查询不会为了物化默认值而写状态。

Plan 不跨 Thread 共享。工具读取当前 CallContext 的不透明扩展快照，解码为产品 Plan 状态后
由同一纯状态机计算候选，返回扩展 CAS；core 与工具完整结果及实际上下文一次提交。
工具不持有可写 Arc 状态机或第二份会话 working set，提交失败不会留下私有状态更新。

child Agent 的 session 由消息 fork 策略创建，不继承 parent 的 Plan 内核、Plan working state、revision 或
工具句柄。child 若需要执行父计划，父 Agent 必须把相关已批准基线明确写进 `spawn_agent.message`；不得通过
跨 session 查询、共享 Arc、root registry 或 SQLite 旁路读取。child 自己注册的 Plan 工具只操作 child
自己的 session 状态。

每个 mutation 使用当前 plan revision 做 CAS。模型工具 operation identity 来自
`sessionId/turnId/callId`，并与 canonical argument hash 一起写入 receipt；Interaction resolution 使用
interaction ID、Plan revision 和 resolution hash。相同 identity 与相同参数重放返回
`alreadyApplied`，相同 identity 携带不同参数返回 `operationIdentityConflict`。历史和 receipts 都使用
有界尾部与滚动摘要，整个 Plan working state 有固定大小上限。

`plan_submit` 在同一工具提交中保存 awaitingConfirmation 状态、实际结果和不透明确认问题。
问题身份由 core 的调用关联生成。Studio 回答入口核对问题、计划 revision、内容 hash 与 pending ID，
由产品状态机解释 Approve/Revise，然后原子提交交互回答、Plan 扩展、实际上下文与 continuation 输入。
冲突仍保持 Pending；重复回答只返回原回执，不再次推进或入队。

confirmation continuation mail 以完整、原样的 Markdown Plan question 作为用户输入内容。宿主注册
session Plan 工具时通过 `AgentSessionPlanOptions::with_submitted_plan_presentation` 预设
`MessagePresentation::Hidden | Visible`，默认及
内置 Task Mode 使用 `hidden`；模型工具参数不能覆盖该值。两种 presentation 都向 provider 产生同一条
user message，区别只在 GUI Timeline 是否投影。Approve/Revise 的 typed 结果仍由 Plan 状态和 pinned
`pl.plan` context 表达，因此模型既能读到被确认的完整计划，也能读到修改意见。这样计划进入同一
Thread transcript，同时默认不制造第二张用户可见计划卡片；Plan 正文仍可由 `plan_current` 查询。

## 24.4 Plan 工具合同

Plan 工具作为一组安装，读取当前 Thread 的同一扩展快照：

- `plan_current`：从当前 Thread 读取 revision、当前状态、完整计划文档、pending Interaction、最近修改意见和允许操作；同一 session 的实施期可继续读取已批准计划；
- `plan_next`：从同一 session 快照读取当前状态的固定直接转换、触发方、条件和准确工具/用户动作；
- `plan_history`：从同一 session 快照读取有界转换历史与归档摘要；
- `plan_submit`：以 `expectedRevision` 和完整一级标题 Markdown 提交当前或修订计划，并作为请求用户批准
  实施该完整计划的唯一工具；不得先用 `request_user_input` 或普通文本重复询问是否实施、继续或批准；
- `plan_restart`：以 `expectedRevision` 和非空 reason 从允许状态清空旧计划并回到 `drafting`。

前三个查询工具为 `Read + Coexist`；两个 mutation 为 `AgentControl + Solo`。不存在 `submit_plan`、
`plan_transition`、`plan_exit` 或兼容别名，也不存在 definition、compile、patch、supersede 输入。
Approve/Revise 不是模型工具，而是 pending Interaction 的用户决定。

五个工具实现核心不透明 Tool，经 Registration 转移给各自 Thread。查询读取只读快照，
mutation 显式授予扩展和交互控制权限并独占批次。注册选项只控制批准后输入的 presentation，
工具参数不能覆盖。child 只能操作自己的扩展，不能共享 parent 的计划状态。

状态、CAS 或 identity 拒绝使用结构化 `accepted: false` 响应，不把关键诊断压缩成一句错误。响应必须包含
稳定 code、尝试的 operation、当前 state/revision、canonical snapshot、当前允许的全部转换及条件、
失败条件和可直接执行的 recovery actions；例如 stale revision 必须明确要求先调用 `plan_current`，
awaiting 状态的重复 submit 必须指出等待哪个 Interaction，而 approved 状态必须指出先调用
`plan_restart`。schema/Markdown/大小等输入错误仍使用 typed tool validation error，且不得改变状态。

## 24.5 与 Task Mode、Prompt 和 GUI 的边界

Task Mode 的 workflow planning 阶段可以发生多轮澄清、计划提交和修订；这些行为不产生 workflow
transition。只有 `plan_current` 已返回 `approved` 后，root 才能把 Task workflow 从 `planning`
转换到 `editing_documents`。因此 Task 预设图不包含 `awaiting_confirmation`，Mode 状态栏在整个计划
确认期间持续显示 `planning`。`request_user_input` 只收集会实质影响计划的缺失事实或偏好；它不能承担
完整计划批准或实施授权。完整计划形成后 root 必须直接调用 `plan_submit`，不得以普通问题或 final 文本
把是否实施交回用户。

Turn 模型上下文从当前 Thread Plan state 派生只读 `pl.plan` section，提供当前 state/revision、文档 hash、修改意见
和允许动作；批准后的完整计划作为隐藏 user message 存在 canonical transcript 中，并继续存在 canonical
tool/Interaction 状态。GUI 不新增 Plan 协议状态、持久化实体或第二套 reducer，而是从 pending `UserInput`
中稳定 ID 为 `plan_confirmation` 的唯一问题派生只读展示：Timeline 尾部显示一张紧凑摘要卡，点击后在
右侧打开完整 Markdown Plan，详情面板与 Timeline 分别保存滚动位置。宽窗口使用并排侧栏，窄窗口使用
覆盖式侧栏；展开状态只属于当前 Thread 的临时 UI 状态，不写入 runtime 或 store。Plan 详情打开时优先于
Todo 侧栏，关闭后 Todo 仍按自己的 UI 状态恢复。

Plan confirmation pending 期间，普通 prompt composer 必须被 Plan 专用反馈栏替换，避免同时存在“普通消息”
和“计划修改”两个输入语义。反馈栏始终直接提供修改意见输入、提交修改与确认计划三个能力，不先进入
Approve/Revise 二级选择；提交修改映射为同一 `plan_confirmation` answer 的 `Revise` 加非空反馈，确认映射为
`Approve`。右侧详情面板只负责阅读，不再包含第二份输入或确认控件。Interaction resolved 后摘要、详情和
反馈栏一起消失，普通 composer 恢复。以上均响应同一个 durable Interaction，不能复制 Plan 正文、resolution
或生命周期状态。

## 24.6 验收

确定性测试覆盖完整状态矩阵、非法操作的完整提示、CAS、operation identity 幂等、内容 hash、修订意见、
stale Interaction、restart、working-state 恢复，以及 pending/resolve 与 session 的 owner commit 原子性。
默认工具目录必须包含整套 `plan_*`，并明确不存在旧 `submit_plan`。

真实 Task GUI 验收在 workflow `planning` 中先提出缺失信息，再提交计划；用户要求补充后 Plan 进入
`revisionRequested`，模型读取当前状态并提交修订版，用户批准后 Plan 进入 `approved`，随后才允许
workflow 进入 `editing_documents`。后续多目录隔离、多 worktree 并行子代理、整合、只读 reviewer、
终态、重启和清理证据继续由 Thread Mode workflow 验收，不得因 Plan 重构而弱化。

真实 Plan-only GUI 回归使用 `cargo xtask verify-workflow --live --gui --plan-only`。专用完整需求不留下
需要澄清的事实；Driver 要求修订首版计划、批准完整修订版并等待 canonical `plan_current` 返回
`approved`。该验收必须拒绝 `request_user_input`、workflow transition、`complete` 和任何项目文件修改，
保留 provider wire、通用 UserInput、Timeline Plan 摘要、右侧详情、替换式反馈栏、GUI snapshot、截图、
render tree、workspace diff、shutdown 与进程树证据后停止，不继续实施或完整 workflow 验收。

确定性测试还要覆盖同一 Thread 的五个工具读取同一 canonical 扩展、跨 Turn 从 session 恢复、不同
Thread 状态互相隔离、child fork 不继承 Plan、presentation 预设不能由工具覆盖，以及 Approve 后下一
Turn 的 provider request 含完整 Plan user message；hidden 预设下 GUI snapshot/timeline 不含该输入，user
预设下恰好投影一次。真实子代理验收通过 child spawn message 显式传递
已批准基线，不允许以 child `plan_current` 读取 parent Plan 作为证据。
