# 20 - Studio 状态查询与领域生命周期

## 20.1 CQS

Studio 使用 Command Query Separation。查询只读取 owner 已发布的 canonical snapshot；初始化、激活、
扫描 Skill/Profile、修复、重连和关闭只能由明确 typed command 触发。Widget rebuild、stream resync 与
`read*` 查询不得写 SQLite/配置、访问网络或创建 runtime owner。

进程运行期间 Project、Thread、Agent、Workflow、Recovery 和服务目录的内存 owner 是活动事实源。
SQLite 仅提供 activation 基线、历史冷分页与异步持久化。

## 20.2 公共 snapshot

StudioState 聚合 projectDirectory、threadDirectory、agentDirectory、modeCatalog、settings、recovery、
MCP/LSP、provider usage 与 updater。Thread workspace 单独包含 timeline、pending Interaction、
ThreadRuntimeView 和 `WorkflowSessionState` projection。不存在 taskDirectory。

Product event 携带完整领域 snapshot 或明确 revision：ProjectDirectoryChanged、
ThreadDirectoryChanged、AgentDirectoryChanged、ModeCatalogChanged、ThreadRuntimeChanged 等。Dart reducer
拒绝旧 revision，并可用一次全量 snapshot 从 stream lag 恢复；它不自行推导 workflow transition。
一次目录命令可以同时改变 Project 与 Thread，但每个实际变化的领域最多发布一次事件；空 delta
不得提升 revision 或发布空事件。冷记录进入驻留/归档命令的内存索引属于 owner 准备步骤，不单独
形成产品事实或广播，最终业务 mutation 才通过 directory command 发布 canonical delta。

## 20.3 Activation

选择冷 Thread、提交输入或后台 child 继续时显式 activation。runtime 在一致读视图中校验并加载 Thread、
working state、transcript window 与 pending Interaction，全部成功后一次安装 owner。Mode snapshot 和
workflow projection 与 session 同时恢复，不存在独立 TaskRuntime 恢复扫描。

## 20.4 配置目录

Mode catalog 直接投影 `pl-core::thread::ThreadModeManager` 的内存 snapshot；内置 Mode 由
Studio 启动时以静态描述注册，既不扫描 Skill，也不读取或复制用户目录资源。未来外部 loader 只能
先把文件解析为同一拥有所有权的 registration，再调用公开注册接口。Agent Profile catalog 合并
Rust builtin 与用户 TOML；完整 Agent 配置投影属于 Settings snapshot。系统启停、系统 route 更新和
用户 Profile 保存都携带 `expectedSettingsRevision`；成功后返回最新完整 canonical snapshot，Flutter
原子替换 Settings 领域，不只修改本地 draft。

## 20.5 Shutdown

shutdown 命令阻止新 mutation，停止/等待活动 Turn，flush 所有 Thread checkpoint，关闭 Agent、MCP、
LSP 与订阅，最后发布 Stopped。GUI 只有收到该终态才可正常销毁 engine；Driver harness 还需确认完整
原生子进程树已退出。

## 20.6 Automatic title lifecycle

`CreateThreadRequest.title = None` 表示由首条已接受的文本 prompt 触发一次自动 title；显式 title
不会触发自动任务。运行时只保存有界的 title task handle 与每任务一次性取消发送端，不把命名任务
当作用户 Turn。
Explorer request 使用独立临时 session，最多 40 秒，禁止 tools、MCP 和持久化；若模型声明了
effort，则按模型定义中从弱到强数组的首项请求最弱强度。始终启用 reasoning 的 provider 获得足以容纳
隐藏思考的有界输出预算，不能把 UI title 长度误作模型总输出预算。生成提示只要求概括首条请求的具体目标并
返回一个标题，不包含字符集、词数、标点或 UI 长度规则。标题解析只读取 provider 返回的可见 assistant
文本，忽略 reasoning/思考内容；唯一处理路径折叠空白、拒绝空结果，再截取前 36 个 Unicode 字符形成
canonical title。JSON、Markdown、引号、标点和普通文本不进入不同解析或兼容分支。标题 Turn 不直接把原始
首条 prompt 作为最后一条 user 指令：先把它编码为 JSON string 形式的不可信数据，再在同一 user message
末尾给出生成 title 的当前任务，避免原请求末尾的工具或执行命令取得最近指令位置。任务在首条 Turn 被接受后注册，并等待该 Turn 空闲再占用
Explorer provider，避免后台命名与用户 Turn 争抢同一远程路由；这段等待可由手动 rename、归档或
shutdown 取消，不计入模型调用的 40 秒超时。

标题生成完成后，directory owner 在同一 mutation 临界区核对期望临时 title，再提交 write-behind
delta 与 `ThreadDirectoryChanged`。手动 rename 先提交 canonical title，因此自动任务观察到期望值变化
时必须丢弃结果；手动 rename/归档命令还会通过每线程取消令牌终止尚未结束的任务。shutdown 先阻止新 mutation，再取消并等待所有 title task，最后 flush persistence；
关闭前未完成的自动 title 不在重启时重试。shutdown 只取消并清空当前生命周期已经注册的
title task；同一个 `StudioRuntime` 再次 initialize 后，新任务必须获得新的独立一次性通道，不能继承
上一次 shutdown 的已取消状态。provider 与 Turn 不持有发送端；发送端被意外释放时接收端继续等待
provider，只有 owner 显式发送才具有取消语义。
