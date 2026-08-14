# 13 - 项目级 Skills

## 13.1 目标

Skills 是可复用的任务知识文档，供 agent 在需要时按需读取。系统默认只把自学习产物写入当前项目，避免把项目经验污染到用户全局配置。

首版只实现本地闭环：

- 发现本地 skills。
- 向模型注入简短索引。
- 通过工具读取完整 skill 或支持文件。
- 通过工具创建、修补和删除项目级 skill。
- 每轮结束后由后台 reviewer 自动沉淀可复用经验。

首版不实现在线 Hub、安装市场、GUI 管理页或用户级自动写入。

## 13.2 目录和优先级

运行时按以下优先级发现 skills：

1. 项目目录：`<workspace_root>/skills/`
2. 用户目录：`~/.pure/skills/`
3. 系统目录：`~/.pure/skills/.system/`
4. 配置里的外部目录：`[skills].external_dirs`

同名 skill 只保留最高优先级来源。项目目录是唯一写入目标；用户目录、系统目录和外部目录仅参与只读发现。若模型尝试修改来自用户目录、系统目录或外部目录的 skill，工具必须拒绝原地修改，并提示在项目目录创建同名项目覆盖或新建项目 skill。

`[skills].project_dir` 是相对工作区根目录的路径，默认 `skills`。解析后必须位于 `workspace_root` 内。

项目 skills 路径按主机文件边界处理：项目目录、skill 目录、`SKILL.md`、支持文件和使用统计的已有祖先都不能是 symbolic link 或 Windows reparse point。发现和支持文件索引跳过链接入口；`skill_view` 与 `skill_manage` 直接访问链接时拒绝。删除 skill 时，skill 子树内的链接只删除入口，不能递归进入或修改其目标。用户、系统和显式 external source 仍是只读来源，但其内部发现同样不跟随链接。

系统 skills 是编译进 `pl-core` 的内置能力。启动或加载 skills 时，系统将内置资源同步到 `~/.pure/skills/.system/`。该目录由 Pure 管理，用户不应手动编辑；若需要覆盖系统 skill，应在项目目录创建同名 skill。

## 13.3 Skill 格式

每个 skill 是一个目录，主文件固定为 `SKILL.md`。文件开头使用 YAML frontmatter：

```markdown
---
name: rust-workflow
description: Rust workspace exploration and test workflow.
category: development
platforms: ["windows", "linux", "macos"]
---

# Rust Workflow

...
```

`name` 和 `description` 必填。`category` 和 `platforms` 可选。`platforms` 缺失表示所有平台可用。

支持文件只允许放在以下目录：

- `references/`
- `templates/`
- `scripts/`
- `assets/`

工具读取和写入支持文件时必须拒绝 path traversal，并拒绝访问这些目录之外的文件。

## 13.4 工具

默认工具集中新增：

- `skills_list(category?)`：列出启用 skill 的简短索引。
- `skill_view(name, filePath?)`：读取完整 `SKILL.md` 或支持文件；Simple 模式同时记录项目 skill 使用统计，Task 模式只读且不更新统计。省略 `filePath`、传空字符串、`.` 或 `SKILL.md` 都表示读取主文档；只有真正的支持文件路径才必须位于 `references/`、`templates/`、`scripts/` 或 `assets/` 下。
- `skill_manage(action, ...)`：管理项目目录中的 skill。

`skill_manage` 支持 `create`、`patch`、`edit`、`delete`、`writeFile`、`removeFile`。所有写入都只作用于 `<workspace_root>/<project_dir>/`。写入动作进入现有工具审批流程。`patch.oldString` 首先按 `SKILL.md` 原文字面量匹配；若完全匹配失败，运行时只允许把看起来像 JSON string fragment 的模型输出交给 `serde_json` 解码一层，再按同一匹配数量规则替换，避免因为 JSON/Markdown 二次转义噪声导致可恢复 patch 失败。运行时不维护额外的手写转义替换表。

## 13.5 Prompt 和 Subagent

当 `[skills].enabled = true` 时，核心 turn 在 base instructions 与项目记忆之间注入 skills 索引和使用规则。任务明显匹配某个 skill 时，模型必须先调用 `skill_view(name)` 读取完整内容。

`register_default_tools` 是 root agent 和 subagent 的共同工具入口，因此 subagent 与父 agent 使用同一项目 skills 上下文和同一加载优先级。

系统 skills 与用户/外部 skills 一样对 root agent 和 subagent 可见，但只读。模型如需沉淀新的项目经验，必须通过 `skill_manage` 写入项目目录。

Studio 状态栏的 Skills 只展示当前会话已激活的 skills。激活定义为该会话中 `skill_view` 成功返回并把 skill 内容或支持文件内容写入上下文；仅出现在索引中但未 `skill_view` 的 skill 不计入。成功激活由后端记录为 `SkillActivated` runtime fact，并持久化到会话级 skill activation 表；前端只消费 `sessionRuntime.activeSkills` 和实时 `SkillActivated` 后附带的 runtime snapshot，不解析工具输出 JSON。

Studio 设置页的 Skills 标签页展示 `SkillCatalogRuntime` 已发布的项目 catalog。进入标签页只读取
缓存，不访问文件系统；`discoverSkills(projectId)` 或 Project 激活 command 才按
project/user/system/external 规则扫描并整体发布新 revision。该列表不调用 `skill_view`，不改变
会话 active skills，也不写入使用统计。

TurnFactory 获取并冻结当前 catalog revision。`skills_list` 与 `skill_view` 只使用冻结 catalog，
不会在每次工具调用重新 discover；`skill_manage` 也以冻结 catalog 校验目标，写入成功后通知 owner
为未来 Turn 重建 catalog，当前 Turn 仍保持原 revision。system Skills 只在
`startStudioRuntime` 安装，不设置隐式 filesystem watcher。

## 13.6 自学习

`StudioRuntime::run_prompt` 完成 Simple 模式主 turn 并保存记录后，如果 `[skills].auto_learn = true`，后台启动 reviewer 复盘本轮对话。reviewer 只注册 skills 工具，不注册 shell、文件或 subagent 工具。Task 模式不启动自学习 reviewer；从规划开始到任务终态，项目 workspace 的写入由 Task coordinator 独占。

自学习默认写入项目目录。reviewer 优先修补本轮已读取的项目 skill，其次修补已有项目 umbrella skill，最后才创建新的泛化 skill。reviewer 不得修改系统、用户或外部 skill；如果系统 skill 给出了通用指导，而本轮产生了项目特定经验，应创建或更新项目 skill。reviewer 不应记录一次性任务、瞬时环境失败、负面工具断言或纯用户私密偏好。

自学习失败只写日志，不影响用户 turn 的结果。
