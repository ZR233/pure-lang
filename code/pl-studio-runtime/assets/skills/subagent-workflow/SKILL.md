---
name: subagent-workflow
description: 当任务需要通过糊来帮 Agent Profile 使用子代理进行并行探索、独立实现或静态审查时使用。
category: agents
---

# 子代理协作

用户要求子代理、并行探索、跨 crate 分析、独立验证或按角色分工，且任务适合独立委派时使用。

## 选择 Profile

尚不清楚合适的 Profile 时，先调用 `list_agent_profiles`，再将选定的稳定 `profileId` 传给
`spawn_agent`。子代理创建时冻结创建指令和物理工作区；宿主可在下一 Turn 边界应用当前的
provider、model 和 effort 配置。禁用或不可用的 Profile 不能派发。

首次调用就使用工具 schema 的 camelCase 字段。标准形状如下：

- 无目录限制的探索者或审查者：`{"profileId":"explorer","forkTurns":"none","taskSummary":"检查指定组件","message":"..."}`，按角色替换 `profileId`。
- 本地目录执行者：`{"profileId":"executor","forkTurns":"none","writablePaths":["src/module"],"taskSummary":"实现指定模块","message":"..."}`。
- 工作树执行者：`{"profileId":"worktree_executor","forkTurns":"none","taskSummary":"在独立工作树实现指定模块","message":"..."}`。

每次派发必须提供简洁的 `taskSummary`，空白归一化后为 1–80 个 Unicode 字符，用作 GUI
子代理列表标题；`message` 包含完整任务。主代理和子代理在首次工具调用前、重要发现、阶段
切换、等待及阻塞等有意义的节点，用 1–3 句 commentary 汇报。

不得发送 `profile_id`、`fork_turns` 或 `writable_paths`。只有 directory Profile 接受
`writablePaths`，unrestricted 和 worktree Profile 不接受。首次调用前确认 Profile、模式及
最窄目录范围。遇到类型化参数错误时修正 schema 用法，不重复原参数。刻意触发的目录边界
拒绝记为 `expected_rejection`，不得重试或通过 shell、Git、MCP 绕过。

内置子代理 Profile 为 `explorer`、`executor`、`worktree_executor` 和 `reviewer`，不可修改，
可以禁用。`planner` 是主代理路由，不是子代理 Profile，不能派发或禁用；历史 planner 子代理
仅可读取，不可续跑。用户 Profile 按每个 Profile 一个 TOML 文件加载。按能力选择 Profile，
不能假定某个工作流阶段必然对应某个 Profile。

子代理不继承主 Thread Mode 的工作流工具或运行时状态。主代理只能查询及推进宿主已注册
的图；主代理和子代理都不能编译工作流定义。

## 派发时机与工作区

`spawn_agent` 用于边界明确的异步任务。独立探索使用 `forkTurns:none` 并行派发，由主代理
综合证据；需要检查实例时使用 `list_agents`，没有独立工作时使用 `wait`。真实依赖保持顺序，
所有权重叠的修改不并行。Task 的 `editing_documents` 阶段仅由主代理写入 `design/**`。

单个执行者交付就安排对应 reviewer，不等待整批完成。审查目标是该执行者的实际工作区：
本地 directory 改动已在当前工作空间，只有通过审查的 worktree 提交需要主代理合入。
Profile 选择、审查范围和 Git 所有权应分别明确。

派发内容、局部验证、审查、增量合入、汇报、等待及续跑遵循系统提示词的统一协作合同。
本技能提供 Profile 选择和调用示例，不另设完成协议或确认步骤。派发正文及 Turn 报告没有
应用层长度限制；只有 `taskSummary` 是简短列表标题。
