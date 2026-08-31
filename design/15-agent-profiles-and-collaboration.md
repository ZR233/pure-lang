# 15 - Agent Profile 与统一协作

## 15.1 边界

Studio 的 child Agent 使用与 root 相同的 Thread/Turn/Tool 框架。Profile 冻结模型路由与工作区模式，
但不恢复旧 Task/WorkUnit、completion、delivery review 或自动 merge 体系。父 Agent 负责拆分工作、
避免冲突、审查成果，并用普通 Git 显式整合 worktree child 的 commit。

工作区有三种模式：

- `unrestricted`：Profile 不增加额外项目隔离，root 是 Project root；项目内外仍遵循会话 Permission Mode。
- `directory`：root 仍是 Project root；`writablePaths` 只限制 Pure 内置文件 mutation 工具在项目内的写入。
  它不是 OS 沙箱，shell、Git 与 MCP 可以绕过，工具描述、child 固定上下文和 GUI 必须共同提示该边界。
- `worktree`：root 是独立 Git worktree，boundary 为 `Confined`，worktree 内全可写；主工作区未提交内容
  不复制过去，成果不会自动合并。

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
workspace_mode = "directory"
system_instructions = """
遵循项目规范完成实现，并验证相关测试。
"""

[model]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"
```

用户 Profile 可选择三种模式。旧文件缺少 `workspace_mode` 时按 `directory` 解释，下一次保存写回
canonical 字段。文件分别解析、校验和原子保存；无效文件保留原字节、从有效目录排除，并以脱敏
warning 暴露。合法但 provider/model 当前不可解析的 Profile 保留在 Settings 中并标记 unavailable。

## 15.3 系统预设

Studio 注册五个系统 Profile：`explorer`、`planner`、`reviewer` 固定为 `unrestricted`，`executor`
固定为 `directory`，`worktree_executor` 固定为 `worktree`。系统 id、名称、用途、指令和模式不可编辑，
但 Agents 设置页可以配置启用状态、provider/model 和由模型声明驱动的 effort。禁用 `planner` 只从
子代理目录排除它，不影响 root 继续使用 planner route。

配置变化只影响未来 spawn。每个 child 创建时冻结 Profile id、正文、provider、model、effort、配置
revision 与 `AgentWorkspaceAssignmentSnapshot`；运行中的 child 不随设置变化，也不在每轮回读 SQLite。

## 15.4 spawn 与目录写策略

`spawn_agent` 接收可选 `writablePaths`。只有 `directory` Profile 接受该字段：省略表示整个项目可写，
空数组表示项目内只读；条目是项目相对目录前缀。runtime 拒绝绝对路径、`..`、非法分隔以及解析后
越界或经过不安全 symlink 的路径，规范化和去重后冻结。其他模式传入该字段直接返回参数错误，避免
形成虚假隔离预期。

spawn receipt 返回模式、实际 root、canonical 可写目录；worktree 模式额外返回 branch 与 base commit。
所有 Pure 内置文件 mutation（apply patch、write/delete/copy/move、项目 Skill 写入）都调用同一中央
路径策略。读取不受 `writablePaths` 限制，项目外路径仍只由 Permission Mode 决定。

## 15.5 worktree 生命周期

本地和 SSH 后端都以 spawn 时解析的 `HEAD` 执行 `git worktree add -b`，禁用 hooks 和 credential
helper，最长 120 秒。路径为 `<repo>/.pure/worktrees/<root-thread-id>/<child-id>`，分支使用 Pure-owned
`pure-agent-*` 名称。非 Git 项目或无 HEAD 时 typed 失败。

`studio_objects` 保存版本化 lease：`prepared | active | preserved | cleanupRequested | cleaned`，以及
repo、path、branch、base 与 revision。spawn 任一阶段失败都按 `NoSideEffects | MayHaveCreated` 分类补偿
Thread、热资源、worktree 与 branch。启动恢复只按 durable lease 对账；资源部分缺失或身份不匹配时
保留现场并发布 Recovery issue，不盲删目录或非 Pure 分支。

`close_agent` 对 worktree child 接受 `workspaceDisposition = preserve | cleanup`，默认 `preserve`。
关闭不自动 commit、merge、cherry-pick 或修改主分支。父 Agent 应先审查 child commit、用普通 Git 显式
整合，再请求 cleanup。已经 preserved 的 lease 在 Agents/Recovery 中显示 revision、branch、base/head、
dirty 与 changed-files 预览，并提供显式清理。

## 15.6 GUI 与验证

Agents 是 canonical Agent 配置中心；不再保留重复 Roles 设置页。系统卡片显示固定模式徽标、启用开关、
provider/model/effort 控件，用户编辑器额外显示三模式选择。所有设置 mutation 携带
`expectedSettingsRevision`，成功后以返回的完整 canonical settings snapshot 原子刷新 UI。

确定性验收覆盖 schema 迁移、模式冻结、目录允许/拒绝/外部路径/symlink、shell 可绕过的显式合同、
本地与 SSH worktree 创建和补偿、preserve/cleanup、重启 reconcile 及 GUI revision。真实验收入口为
`cargo xtask verify-subagents --live --gui`：使用隔离的临时 Studio home 与 Git fixture，从 GUI 配置并
提交真实 prompt，证明两种 executor 的 spawn receipt、目录拒绝、worktree 分支、显式整合、cleanup、
最终测试、截图和 terminal receipt。
