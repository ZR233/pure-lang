# 09 - 工具调用运行时与 Thread 边界

本文是通用工具身份、注册、调度与执行边界的唯一权威源；workflow 专属工具契约见
[11](./11-thread-mode.md)，协作工具（spawn_agent 等）的参数与工作区语义见
[12](./12-collaboration.md)，会话后台任务交付见 [14](./14-runtime-host.md)。

## 9.1 身份与所有权

具体工具只实现 core 的不透明 Tool 接口，以注册句柄将实例所有权转移给 Thread。工具定义和参数是
带 format/version 的不透明载荷：tool 解释参数，model 解释模型声明，core 不解析 JSON。参数
schema、业务权限分类、JSON 解析、工作区约束和输出投影属于 pl-tool。每次请求冻结同一工具目录
与 executor，稳定工具身份和原调用参数进入日志。工具输出分别保留完整 payload 与模型可见
context；已观察到副作用的失败携带输出事实保存。Note、Todo 和 Skill 激活使用 Thread 扩展，
工具不持有第二份会话状态。

工具不能由名字、MCP annotations 或输出正文取得控制权：结束 Turn、扩展 CAS、交互、发现、任务
等待与取消均使用显式注册授权及类型化控制值。完整 payload 与实际 delivered context 分开保存；
取消或失败保留观察结果，但不得提交迟到控制或扩展更新。Plan/workflow 工具与状态机属于
Studio，不进入 core。

## 9.2 动态注册、撤销与热目录刷新

动态注册与撤销在 Thread owner 中完成，失败不发布部分目录。重连不改变声明身份，已冻结调用
持有旧执行租约；准入前、执行前和控制提交前仍校验权限未被撤销。

热目录刷新订阅产品配置、MCP generation、Skill 目录以及语言服务目录事件。刷新只重新构造具体
工具声明与执行器，不重放历史、不重读指令、不刷新 SQLite；核心注册表原子替换后，旧模型 step
的执行器继续持有原租约，已撤销权限按新目录拒绝后续准入。一个 Thread 的命令管理器跨目录刷新
复用，缓存只持弱引用；同 ID 的新 owner 不继承旧 owner 的物理资源。已冻结的 workspace/SSH
目标发生变化时清空不可再授权的目录并报告必须重新激活，不能偷偷改为访问新路径。

SSH 配置或凭据变更单独通知目录刷新并撤销旧物理绑定；日志和信号不携带 secret。目录刷新失败
关闭可调用目录并发布恢复问题，下一次相关配置/目录变更重新尝试；物理目标变化必须显式重新
激活。关闭观察 barrier 成功后回收绑定缓存，失败时保留以便重试。

过期工具候选的关闭失败由 Studio 持有原始可重试资源及拒绝原因，并绑定原 Thread incarnation；
失败候选清理完成前不再次构造同 owner 的候选，不把失败指纹标记为已安装。目录 watcher 采用
协作停止，完成在途资源移交后再退出；shutdown 重试未完成候选关闭，失败返回错误并保留
owner，取消 shutdown 等待也不得丢失资源。

## 9.3 调度声明与批处理

注册的执行等待策略与模型批次约束分开声明，共三类：

- 前台独占（foreground）：必须单独调用，不能与任何其他调用混批。
- 可同批前台（foreground_coexisting）：可与其他前台工具同批，按模型给出的调用顺序等待每个
  调用终态，并继承本轮取消。
- 后台任务：一秒交付窗口后返回运行回执，结果经消息收件箱送达（见 [14](./14-runtime-host.md)）。

真正控制权限（结束 Turn、扩展变更等）始终要求前台独占，不可通过可同批前台能力降级。
`write_file`、`apply_patch` 和 `write_stdin` 使用可同批前台能力；现有 writer 互斥保持，不新增
跨后台或外部进程的读写事务保证——该顺序不是外部文件系统事务，不承诺阻止已有后台任务或外部
进程访问文件。`write_stdin` 只等待写入完成，不等待 exec 进程终态。

## 9.4 进度预览与物理身份

命令进度通过任务进度通道上报有界的通用内容预览。预览只属于原任务、原执行器和仍在运行的
Thread；更新不提升历史水位、不进入模型 context 或持久化状态。Thread 终态提交清除预览，
历史只保存已提交的完整结果。命令采集端合并至 64 KiB 预览并标记截断，完整输出仍由归档服务
保留；预览传输与命令执行同生命周期，不派生无 owner 的后台队列。

Provider/core 调用 ID 进入命令进程与归档边界时，转换为有界 `task-` 前缀加 SHA-256 的物理
身份，不能直接作文件名。`exec` 与 `write_stdin` 使用同一映射，目录重建继续引用同一 Thread
的进程管理器；不同 Thread 的管理器保持隔离。

## 9.5 Programmatic 调用资格

Programmatic 调用资格由具体工具的声明提供：LSP、只读 Git、已解析为只读的 MCP 工具和 MCP
资源查询携带允许调用方及输出 schema。未知效果、写操作、交互和状态更新不自动获得资格；模型
适配只按已验证的 route/hosted capability 决定是否编码这些已声明资格，不按名字推断权限。

## 9.6 MCP 能力声明与租约

MCP resource façade 属于本轮冻结工具目录。Runtime 必须读取 server 在 initialize/discovery 中
声明的 `resources` capability，只把支持该能力的 server 写入 lease 的 resource assignment；
没有任何此类 server 时不向模型暴露 `list_mcp_resources`、`list_mcp_resource_templates` 或
`read_mcp_resource`。聚合查询只访问 assignment 中的 server；显式指定未声明该能力的 server
必须在发送请求前返回稳定参数错误，不能用一次预期的 `Method not found` 探测能力，也不能因此
把正常 MCP transport 标记为 unavailable。模型可见 schema 与执行路径必须消费同一冻结
assignment，避免暴露必然首次失败的工具。

MCP tool executor 必须捕获创建它的轮次租约（McpTurnLease）、server identity、raw tool name
与 generation；旧工具计划即使在新 generation 发布后仍调用旧 lease，最后一个 executor/plan
释放后才能回收旧连接。远端展示 metadata 不得提升 effect、并行、programmatic、cache 或权限
策略。

## 9.7 文件读取缓存与可见内容

`read_file` 的精确请求可复用同一工作区 epoch 内的结果，但命中仍返回所请求的文件内容，不能只
返回"此前已读取"的摘要。不同的行范围按独立请求处理；模型可能是在输出截断或上下文压缩后补读，
不能把旧范围被覆盖等同于正文仍在模型上下文中。文件 IO 的精确请求去重及变更失效规则保持
有效。协作控制调用可能触发或观察 child 的文件变更，因此使父代理的 workspace 缓存 epoch
前进；不能跨 child 派发、续跑或交付边界复用旧文件视图。
