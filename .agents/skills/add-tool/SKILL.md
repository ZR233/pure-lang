---
name: add-tool
description: Use when implementing, registering, or testing a Pure-Lang tool through the Thread-owned opaque Tool interface.
---

# 添加或迁移工具

具体工具归 `pl-tool`，Plan/workflow 等产品工具归 Studio。core 只定义通用契约与 Thread 执行机制，
不依赖工具实现、pl-model 或 pl-protocol。先阅读 `design/27-core-boundaries-and-replay.md`、
`design/28-tool-thread-boundary.md` 和同类现役工具，再沿以下边界修改。

## 实现与注册

- 实现 `pl_core::tool::opaque::Tool`，用 Registration 转移实例所有权给 Thread。参考
  `code/pl-core/examples/minimal_thread.rs` 和现役工具，禁止 StaticTool、ToolCallContext、
  SessionRuntimeBuilder 或旧 working set 兼容包装。
- 参数与声明使用带 format/version 的 OpaquePayload；工具自己解析原始字符串，model 编码模型声明。
  JSON 工具使用 typed 参数和 serde/schemars，保留原调用字节，不在 core 规范化参数或 schema。
- 通过 StudioThreadFactory / StudioThreadAssembler 在创建 root、child、恢复时显式装配，
  每 Thread 独占工具实例和命令管理器；共用物理服务捕获隔离租约，不共享可变工具状态。
- 动态注册更新同一 Thread 目录；工具稳定 ID、声明内容身份与 executor 代次分开。重连但声明未变
  保持 deferred reveal，删除/声明变更清除旧状态；已冻结调用保留旧租约但仍接受权限撤销检查。

## 结果与可信控制

- ToolOutput 分别保存完整 payload 与实际模型 context。大输出用稳定资源引用并校验长度/摘要；
  预览不能覆盖完整结果，临时 capture 或路径不能冒充已归档资源。
- 已观察副作用后失败用 ToolError::with_output 保存原输出；取消/失败不丢掉观察事实，也不应用
  迟到扩展、交互或结束控制。历史只使用已保存 delivered_context，不调用当前渲染器重建。
- 结束 Turn、扩展 CAS、交互、发现、任务等待和取消须显式注册授权并返回类型化控制值。
  payload 的 approved/endTurn 等字段、工具名字和 MCP annotations 不授予权限。
- Note/Todo/Skill/业务状态从 CallContext 只读扩展快照计算 CAS 候选，与结果一同提交；
  工具不另存可写状态机。冲突保留原结果用于提交处理，不能重新执行副作用。
- 用户问题返回 AwaitInteraction 与原始问题，core 提供关联 ID；业务回答由工具/Studio 解码，
  core 不把取消合成空回答。Plan 确认及 continuation 的原子解释属于 Studio。

## 物理执行边界

工作区、路径、文件权限、LSP、MCP、SSH、命令进程和归档由工具后端负责。ExecutionPolicy
接收工具侧访问解释，core 只管理类型化许可；模型参数不能扩大可信 CallContext 的能力。
文件能力不隐式授权 exec/Git/MCP；远程和受限工作区不能借本地路径授权放宽边界。

MCP executor 固定 server 租约、raw name 与声明；媒体先归档精确字节，再用宿主提供的模型投影。
归档失败保存明确的原始观察格式，不重跑远端调用。hosted 工具由 model 处理，core 不注册占位 executor。

工具 close 必须等待自身物理资源收束，失败保留可重试状态。被替换实例仍由原 Thread 负责关闭，
不得在注册表锁内执行 IO/await/外部回调。

## 验证

按实际风险验证真实工具行为、参数错误、授权、原文保留、取消及物理资源释放；通用执行/注册/CAS/
冷重放机制由 core 契约测试验证，产品装配在 Studio 验证，不复制旧静态 adapter 测试。
公共接口迁移在同批更新消费者、文档和生成输入。检查与交付遵循根 AGENTS.md，独立 core
配置还需无默认 feature 与显式 sqlite 两种组合；live provider 仅显式 opt-in。
