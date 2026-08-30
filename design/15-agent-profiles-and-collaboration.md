# 15 - Agent Profile 与统一协作

## 15.1 边界

Studio 的 child Agent 使用与 root 相同的 Thread/Turn/Tool 框架和 Project workspace。产品层不创建
Git worktree、branch、completion、delivery review 或 merge 记录，也不把 Git 状态作为协作门禁。
父 Agent 负责拆分工作、避免冲突、选择并行度和整合结果。

Agent 的行为与模型选择来自 `AgentProfile`。运行实例与 Profile 是两类对象：Profile 是可用能力
目录，`list_agents` 返回的则是已经生成的运行实例。

## 15.2 用户 Agent 文件

用户 Profile 位于 Studio home 的 `agents/` 目录；默认路径是
`~/.pure/agents/<agent-id>.toml`。目录只扫描第一层普通 `.toml` 文件，文件名 stem 是稳定 id，
不递归读取临时、隐藏或备份文件。单个文件完整表达一个 Agent：

```toml
schema_version = 1
enabled = true
display_name = "Rust 执行者"
description = "实现和重构 Rust 模块"
suitable_tasks = ["Rust 实现", "测试修复"]
system_instructions = """
遵循项目规范完成实现，并验证相关测试。
"""

[model]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"
```

文件分别解析、校验和原子保存。无效文件保留原字节、从有效目录排除，并以脱敏 warning 暴露；
一个文件失败不能重置主配置或其他 Profile。合法但 provider/model 当前不可解析的 Profile 保留在
Settings 中并标记为 unavailable，不能用于 spawn。

## 15.3 系统预设

Studio 启动时由 Rust `BuiltinAgentProfile` 注册 `explorer`、`planner`、`executor`、`reviewer`。
预设不产生 TOML；id、名称、介绍、适用任务、系统指令和模型绑定不可编辑，也不可删除。
主配置的 `[agents].disabled_system_agents` 只保存禁用 id。用户文件与系统 id 冲突时失败关闭，
不得覆盖预设。

禁用或配置变化只影响未来 spawn。每个 child 创建时冻结 Profile id、正文、provider、model、effort
和配置 revision；运行中的 child 不随配置热变更。

## 15.4 工具合同

`list_agent_profiles` 只返回 enabled 且 available 的 Profile：id、展示名、介绍、适用任务、provider、
model 与 effort。`spawn_agent` 接收 `profileId`、handoff、fork policy 与可选 metadata，不再接收固定
role。root 自主选择 Profile；child 不拥有 root 的 `workflow_state`。

`list_agents`、`send_message`、`wait_agents`、`close_agent` 继续处理实例。root 与 child 共享 Project
workspace 和普通工具权限；`AgentWorkspace` 仍是路径与权限的唯一边界，但不因 Profile 或工作流阶段
创建额外目录或 Git 规则。

## 15.5 GUI 与验证

Settings 的 Agents 页面显示只读系统卡片和启用开关，并对用户文件提供创建、编辑、禁用和删除。
删除用户 Profile 只删除该精确 TOML，必须使用显式 id 解析和原子/可恢复文件操作；已经运行的实例
保持冻结快照。

验收覆盖目录发现、无效文件隔离、系统预设防覆盖、disabled 持久化、provider 不可用状态、Profile
快照，以及多个 Agent 在同一 workspace 下由父 Agent 自主协调的行为。任何协作测试不得要求 Git、
worktree 或 commit。
