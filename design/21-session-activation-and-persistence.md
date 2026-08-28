# 21 - 会话激活、热状态与异步持久化

## 21.1 Activation 与唯一热状态

程序启动只恢复目录、活动 Task 索引与必须继续运行的 owner。用户选择 Thread、后台 Task
准备执行或恢复、以及向冷 Thread 提交输入，都是显式 activation command。activation 在同一个
一致读视图中加载版本化 working object、当前有效模型 transcript、最近 Timeline 窗口、pending
Interaction 与活动 Turn；所有校验完成后才一次性安装 `ActiveSessionState`，不得发布半恢复状态。
附件 catalog 也是 activation 的组成部分：新建 child Thread 必须安装一个类型化空 catalog，恢复
Thread 则装载完整附件领域对象；不存在的热 catalog 表示未激活，而不是“没有附件”。

activation 完成后，类型化 Rust 内存对象是该 owner 的唯一运行时事实源。模型请求、业务
transition、GUI snapshot 与 Task 工具不得再次回读 SQLite 构造活动状态。SQLite 只负责冷恢复、
异步落库以及用户主动向前翻 Timeline 时的 keyset 冷分页；分页结果必须先转成领域对象并合并进
热 Timeline，随后消费者仍只读取热状态。

## 21.2 热对象与持久化边界

热状态直接持有领域对象，不持有 JSON、`serde_json::Value`、SeaORM entity、数据库 DTO 或预编码
payload。owner transition 返回新的领域状态和 typed effect；write-behind queue 保存不可变的
typed persistence snapshot。queue admission 只做 health、revision、容量、sequence、终态许可与
coalescing，不调用 serde。

Mailbox metadata 同样使用递归 typed value，不在 pending/active mailbox 中保存
`serde_json::Value`；只有 repository DTO 与 provider/tool wire 边界执行 JSON 转换。

persistence worker 取得 batch 后才把 typed snapshot 转换为 persistence DTO、序列化、计算 hash 并
执行 SQLite transaction。编码结果只在当前提交及其重试期间复用，不进入热状态。worker panic 时
in-flight typed snapshot 仍由共享 writer 状态持有；panic 必须转成 `Blocked` 并唤醒全部 durability
等待者，不能丢弃事实或留下永久等待。显式 retry 后 supervisor 必须创建新 worker 并重放恢复出的
typed batch。

Thread、Task、Project 的有界 working state 使用版本化对象表；Timeline、transcript segments、目录
关系、计费和其他无界或查询型事实继续使用专用表。对象新增 additive serde 字段不提升 SQLite
schema；新增查询维度、关系约束或无界数据流仍显式修改 schema。运行期不扫描 JSON 执行业务查询。

## 21.3 提交、批量与释放

每次 mutation 必须先被 writer 原子接受，再替换 owner 热 snapshot 并广播。接受失败时业务状态、
revision 与事件均不得变化。接受成功不等待 SQLite；进程异常退出仍可能丢失尚未提交的尾部。

writer 从首条 pending mutation 开始计算五秒最大延迟，新 mutation 不重置 deadline；累计 64 条、
显式 flush、owner 淘汰、shutdown、Task/lifecycle settlement、上下文 replacement 或不可逆外部动作
立即提交。一次 transaction 最多 64 条。

Task owner revision 是完整 Task aggregate commit 的序列，不等同于 `TaskRun.revision`。冷恢复从
Task commit receipt 装载 owner revision；相同 owner revision 与 payload hash 返回
`AlreadyApplied`，相同 revision 但不同内容进入 `Blocked`。

Executor allocation 必须把规范化后的完整 typed blueprint 同时写入 WorkUnit canonical context，不能
等 child handoff 生成后再从 Thread projection 反推。WorkUnit 的热目录投影（fingerprint、objective、
实施步骤、验收条件与验证数量）只从这份领域事实生成，后续 transition 不得依赖旧 projection 自我
保留。持久化 DTO 把 blueprint 作为 `state_json` 的 additive 顶层字段，保留既有 `$.kind` 查询判别与
索引，不修改 SQLite schema；旧行缺少该字段时，activation 可从已持久化 handoff 一次补入领域对象，
下一次 typed Task commit 完成升级。
`executorProgressRevision` 属于 child Thread owner 的瞬时进度，不是 Task aggregate 的 durable
事实；Task projection 不得从旧 projection 自我保留，也不得在 Task 冷激活时逐条回读 Thread 表来
拼装。没有显式跨 owner typed fact 时该字段保持零，保证关闭前后同一 Task 事实产生相同投影。

模型热 Context 是从 transcript segments 重建的缓存，不单独持久化。LLM request 只从热 Context
构造；上下文 replacement durable 后才可释放被压缩的旧原始 Turn。未完成 assistant 流式草稿不
进入 transcript，崩溃恢复时遗留 running Turn 收束为 interrupted/cancelled 终态。

Timeline 使用独立热窗口。冷 activation 至少装载最近 400 个 Item，并把边界扩展到完整 Turn；更早
历史由显式 keyset 分页加载并合并热状态。Timeline Item 只有 durable、位于窗口之外、且不属于活动
Turn 或 pending Interaction 时才可释放。

## 21.4 驻留

当前选中 Thread、存在非终态 Task 的 root/executor/reviewer Thread，以及正在执行 provider、工具、
压缩或分页合并的 Thread 都被 pin。GUI 切换不能淘汰后台任务。未选中、无活动 Task、无短期操作的
Thread 进入 LRU，最多保留四个；第五码触发最久未使用者的 owner durability barrier 与淘汰。
flush 失败时保留热对象并发布 persistence-blocked，不能为了满足 LRU 丢弃状态。

运行时 idle/验收判断必须聚合整棵 agent tree 的 `active_turn_id`，不能只看 root，也不能把 durable
WorkUnit/Review 的非终态标签当成正在执行的证明。若全树已无活动 Turn 而 Task 仍未终结，harness
应在有限 grace 后输出热态诊断并失败，不能因残留 `Running` 投影无限等待。

## 21.5 验收

确定性测试必须证明 activation 后业务路径没有 SQLite 回读、admission 不调用 serde、五秒 deadline
不被持续写入重置、第 64 条立即提交、worker panic 保留 in-flight facts、Task pin 不被 LRU 淘汰、
Timeline 冷页先合并热状态，以及对象 additive 字段不要求数据库 migration。

Task 模式另有两层 prompt 验收：recording provider 捕获 Responses、Chat 与 Responses WebSocket 的
最终 wire body；真实模型在隔离多文件 Git fixture 中分别通过 headless 与 Flutter Driver 完成
Planner、计划确认、至少两个 Executor、Delivery Review、Merge、Integrated Review、Task 完成与
重启恢复。scripted provider 只证明确定性 prompt/wire，不得代替真实模型验收。

默认 CI 运行 recording provider 的确定性双 Executor 验收，并直接检查 transport 最终 POST body
及 WebSocket `response.create`，不能只检查 `CompletionRequest` 或 prompt 模板。验收同时冻结
planner、executor、delivery reviewer、integrated reviewer 的角色说明、handoff、热历史、working
context 和实际可见工具 schema；reviewer 的 write 工具必须在交给 provider 前从 `ToolPlan` 删除。
Planner 验收按真实生命周期分成两个 final-wire 边界：首次完整请求冻结 base/global/role/workspace/
skills/真实用户 prompt，后续完整请求冻结由热 Transcript 重建的 tool-call/tool-result 历史与实际
`task_status.completionGate` 阶段事实。不得要求首次请求包含尚未产生的未来阶段 working context。
双 Executor 必须分别对应 fixture 的 normalization 与 validation workstream，并各自产生独立
Delivery Review；Integrated Reviewer 只能拿到只读工具与 `review_exit`，不能出现文件写入、Task
transition、spawn 或 completion 工具。为避免单次 provider 输出在派发前耗尽，Planner 每次响应
只构造并派发一个紧凑 Executor 蓝图；首个派发成功后的 continuation 立即派发第二个，两个已经创建
的工作单仍可并发执行。
wire 凭证的角色与工作流识别必须解析最终 request 的结构化边界：Executor 读取独立
`studio.task_executor_handoff` JSON 中的 blueprint，Delivery Reviewer 读取“目标 WorkUnit”JSON，
Integrated Reviewer 读取明确的审查范围。不得对整个 request 字符串搜索路径或字段名，因为 canonical
用户 prompt、out-of-scope 列表和工具 schema 会同时包含两条 workstream 的文字。
fixture 初始提交必须忽略项目 skill 激活产生的 `skills/**/.usage.json` 运行时记账；该文件不是任务
交付物，不能让模型修改 `.gitignore` 或通过清理运行时事实来伪造 root worktree clean。

真实验收只有以下两个跨平台 xtask 入口，二者共用 `test-fixtures/task-live/`，禁止另建 Task 语义：

```text
cargo xtask verify-task --live --headless
cargo xtask verify-task --live --gui
```

`--live` 是显式费用与 credential 门禁，命令不允许 scripted fallback。GUI 入口必须通过
`cargo xtask run-gui --driver` 启动 native Studio，由 Flutter Driver 完成 prompt 输入/read-back、
计划确认和终态观察；首次完成后先执行 runtime durable shutdown，再使用同一隔离 Studio home
重启并恢复已完成 Task。每次运行在 `target/task-live-artifacts/<run-id>/` 保存 canonical prompt 与
hash、实际 provider protocol/model、最终 wire body、角色 prompt section 与工具 schema hash、
Task/Git/命令输出；GUI 额外保存 screenshot、render tree、Driver 与 native lifecycle 日志。捕获仅在
harness 设置 `PURE_STUDIO_WIRE_CAPTURE_DIR` 时启用，不读取或记录认证头，生产日志默认不保存 prompt。
GUI 重启前后的 project/thread/Task run 标识必须完全一致；隔离配置还必须冻结 planner、executor、
reviewer 的真实非本地 model route。环境变量凭证由 xtask 预检，系统凭证只允许 Studio runtime
解析，Headless 也必须使用 Desktop host 的同一系统凭据边界，不能使用只具备内存凭据存储的 Test
host；真实 provider request 是最终门禁，任何凭证都不写入 artifact。
