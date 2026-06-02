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
- `skill_view(name, filePath?)`：读取完整 `SKILL.md` 或支持文件，并记录使用统计。
- `skill_manage(action, ...)`：管理项目目录中的 skill。

`skill_manage` 支持 `create`、`patch`、`edit`、`delete`、`writeFile`、`removeFile`。所有写入都只作用于 `<workspace_root>/<project_dir>/`。写入动作进入现有工具审批流程。

## 13.5 Prompt 和 Subagent

当 `[skills].enabled = true` 时，核心 turn 在 base instructions 与项目记忆之间注入 skills 索引和使用规则。任务明显匹配某个 skill 时，模型必须先调用 `skill_view(name)` 读取完整内容。

`register_default_tools` 是 root agent 和 subagent 的共同工具入口，因此 subagent 与父 agent 使用同一项目 skills 上下文和同一加载优先级。

系统 skills 与用户/外部 skills 一样对 root agent 和 subagent 可见，但只读。模型如需沉淀新的项目经验，必须通过 `skill_manage` 写入项目目录。

## 13.6 自学习

`StudioRuntime::run_prompt` 完成主 turn 并保存记录后，如果 `[skills].auto_learn = true`，后台启动 reviewer 复盘本轮对话。reviewer 只注册 skills 工具，不注册 shell、文件或 subagent 工具。

自学习默认写入项目目录。reviewer 优先修补本轮已读取的项目 skill，其次修补已有项目 umbrella skill，最后才创建新的泛化 skill。reviewer 不得修改系统、用户或外部 skill；如果系统 skill 给出了通用指导，而本轮产生了项目特定经验，应创建或更新项目 skill。reviewer 不应记录一次性任务、瞬时环境失败、负面工具断言或纯用户私密偏好。

自学习失败只写日志，不影响用户 turn 的结果。
