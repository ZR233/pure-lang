# 03 - Thread / Turn / Item 流程

本文定义输入到 Turn 的通用流程、模型循环与 Interaction 模型。Plan、协作编排与 workflow 状态机
分别在 [13](./13-plan.md)、[12](./12-collaboration.md) 与 [11](./11-thread-mode.md) 单独定义，
本文只保留流程视角的摘要。

## 3.1 输入与 Turn

输入受理只在目标 Thread 空闲时创建排队 Turn；活动 Turn 的补充输入走 steer 通道；稳定输入 ID 保证
重复提交幂等。提交事务原子写入输入、Turn 与 user Item，成功后更新 owner snapshot 并广播 typed
notification；只有 presentation 为 visible 的输入生成 user Item。

MessagePresentation（visible / hidden）是所有消息共用的协议属性，不属于 Plan、Interaction 或
mailbox 特例：visible 是省省略时的默认值；hidden 消息仍是 canonical Thread 上下文，必须持久化并
完整发送给 provider，但产品 projector 不为它产生 GUI Item 或 notification。presentation 在输入
创建点冻结、随 mailbox 持久化，并原样进入 Turn 请求与消息事实，宿主和 GUI 不得重新推导。上下文
压缩生成的摘要、压缩指令与 synthetic user input 使用 hidden；未来其他内部输入也在创建点选择
取值。可见输入的来源（user / parent agent）独立记录：在 runtime 创建输入时冻结并随 mailbox
持久化；产品 projector 将 parent agent 来源投影为独立文本通道，Studio 不根据 Thread 层级、消息
正文或工具名猜测来源；旧记录缺失时按 user 恢复。

Turn 从同一 Thread 事实装配：统一 root 指令、项目 AGENTS.md、当前 Thread Mode 快照、可选的
workflow 投影、普通 Skill、工具快照与 provider route。Simple/Task 等 Mode 不选择不同的模型循环；
root 一律继承统一 planner route。

## 3.2 模型循环与工具批处理

provider response 依次形成 assistant Item、tool call、tool result 与下一轮输入。普通工具可以并批；
workflow 变更类工具声明 Solo——同一 response 中若还有任何其他调用则整批拒绝且无副作用；workflow
查询工具为 Coexist。工作流状态调用在 working-state 克隆上计算；成功调用的 assistant tool call、
tool result 与新 working state 必须由一个 Thread checkpoint 原子提交，失败时三者共同回滚。工具
结果在同一 Turn 立即返回新阶段约束，下一 Turn 与 context compaction 后由最新 workflow 投影继续
约束。完整工具契约见 [09](./09-tool-runtime.md) 与 [11](./11-thread-mode.md)。

普通 `write_file`、`apply_patch` 与 `write_stdin` 可同批共存：Thread 按模型给出的调用顺序等待每个
前台调用完成再执行下一个；它们不因此获得结束 Turn 或扩展变更等控制权限，真正 Solo 的工具仍禁止
混批。该顺序不是外部文件系统事务，不承诺阻止已有后台任务或外部进程访问文件。

## 3.3 Interaction

Interaction 只有通用 UserInput 与 ToolApproval 两种。计划生命周期由 Task root 使用独立的
`plan_*` 工具状态机管理：`plan_submit` 提交以一级 Markdown 标题开头的完整计划并请求批准或修订；
缺失信息、澄清与普通选项输入使用 `request_user_input`，它不得询问是否实施、继续或批准完整计划，
计划完整时必须直接提交计划。两者都生成同一种通用 UserInput Interaction，Plan 只以 typed purpose
绑定状态机，不增加专门的 Interaction 种类；Plan confirmation 是唯一的实施授权入口。完整合同见
[13](./13-plan.md)。

Interaction 请求可携带通用的不可变 continuation 预设：声明决议后用户消息的内容来源与
MessagePresentation，并参与 request identity 与持久化。Studio 对 Plan 确认问题的特判只影响
pending UI 形态，不得用来推导 continuation 内容或可见性；没有预设的 UserInput 不得猜测
continuation。pending Interaction 随 Thread 恢复，响应必须匹配 interaction identity 与 Plan
revision，且只能决议一次。

## 3.4 工作流生命周期（摘要）

带图 Mode 的首个根用户输入在 provider 前自动生成 lineage 与 run。`workflow_transition` 以
run/revision/当前状态三重 CAS 完成当前阶段并沿直接边进入下一阶段；进入终态后 run 立即终止，但
Turn 可继续交付。图 hash 变化由下一个 Turn 自动归档旧 run 并创建同 lineage 的 replacement；正常
终态后的新任务自动创建新 lineage。所有 root Mode 在完成工作后都调用 `complete` 结束当前 Turn。

Workflow 不拥有代码、文件、Git 或 Agent；任何阶段都可使用普通工具，图只约束状态记录和后续提示。
Mode Prompt 可以在不裁剪工具的前提下声明合作式角色边界（例如要求 Task root 把普通实现交给
child），该约束属于冻结 Mode 指令，不是 runtime effect 白名单或 OS 沙箱。状态机协议、工具契约
与持久化上限见 [11](./11-thread-mode.md)。

## 3.5 协作（摘要）

root 通过 `list_agent_profiles` 选择 Profile，以 `spawn_agent` 创建 child；生成时冻结 Profile
指令、模型路由与 workspace assignment。child 使用普通 Thread/Turn/Tool，但不拥有 root 的
workflow 工具。Task root 维护成本感知的任务依赖 DAG，按 ready frontier 派发 child，收齐 durable
delivery 后释放下一前沿；最终 wave 的只读 reviewer 必须提交 durable approval，任一阻塞 finding
都回到实现阶段返工。不存在隐式 merge 或 delivery gate。派生消息与后续消息都作为
visible + parent-agent 输入进入 child 的 canonical transcript 与 Timeline。完整编排合同见
[12](./12-collaboration.md)。
