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

Mode catalog 扫描通过 Skill Provider 触发，结果是动态 selector 的事实源。Agent Profile catalog 合并
Rust builtin 与用户 TOML；系统禁用状态来自主配置。设置命令保存成功后必须返回或发布最新 canonical
snapshot，Flutter 不只修改本地 draft。

## 20.5 Shutdown

shutdown 命令阻止新 mutation，停止/等待活动 Turn，flush 所有 Thread checkpoint，关闭 Agent、MCP、
LSP 与订阅，最后发布 Stopped。GUI 只有收到该终态才可正常销毁 engine；Driver harness 还需确认完整
原生子进程树已退出。
