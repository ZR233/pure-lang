# 13 - 项目级 Skills

## 13.1 目标

Skills 是可复用的任务知识文档，供 agent 在需要时按需读取。`pl-core` 提供与来源无关的
Provider、注册表、发现观测和 Turn 级冻结 catalog；Studio 只拥有进程级 Provider 注册表、项目
catalog 投影与系统资源目录。系统默认只把自学习产物写入当前项目，避免把项目经验污染到用户
全局配置。

当前只实现本地文件系统 Provider 闭环：

- 发现本地 skills。
- 向模型注入简短索引。
- 通过工具读取完整 skill 或支持文件。
- 通过工具创建、修补和删除项目级 skill。
- 每轮结束后由后台 reviewer 自动沉淀可复用经验。

当前不实现远程 Provider、在线 Hub、安装市场、文件 watcher、项目级 `.agents/skills`、平铺
`<name>.md` 或用户级自动写入。

## 13.2 目录和优先级

运行时按以下优先级发现 skills：

1. 项目目录：`<workspace_root>/skills/`
2. 配置用户目录：`[skills].user_dir`，默认 `~/.pure/skills/`
3. Agents 兼容用户目录：Linux `$HOME/.agents/skills/`、Windows `%USERPROFILE%\.agents\skills\`
4. Studio 系统目录：`<studio_home>/studio/skills/.system/`
5. 配置里的外部目录：`[skills].external_dirs`

这些目录由默认文件系统 Provider 映射为带不透明 locator 的候选。Provider 可以并行发现，重名
winner 依次按来源 rank、Provider 注册顺序和 Provider 本地顺序确定。同名 skill 只保留最高
优先级来源；显式配置的用户目录优先于 Agents 兼容目录，两者都标记为
`User` 来源。若两者解析为同一路径，只扫描一次。项目目录是唯一写入目标；用户目录、系统目录
和外部目录仅参与只读发现。若模型尝试修改来自用户目录、系统目录或外部目录的 skill，工具必须
拒绝原地修改，并提示在项目目录创建同名项目覆盖或新建项目 skill。

`[skills].project_dir` 是相对工作区根目录的路径，默认 `skills`。解析后必须位于 `workspace_root` 内。

项目 skills 路径按主机文件边界处理：项目目录、skill 目录、`SKILL.md`、支持文件和使用统计的已有祖先都不能是 symbolic link 或 Windows reparse point。发现和支持文件索引跳过链接入口；`skill_view` 与 `skill_manage` 直接访问链接时拒绝。删除 skill 时，skill 子树内的链接只删除入口，不能递归进入或修改其目标。用户、系统和显式 external source 仍是只读来源，但其内部发现同样不跟随链接。

Studio 预置 skills 归 `pl-studio-runtime` 所有，其 canonical 源码根目录固定为
`code/pl-studio-runtime/assets/skills/`。预置 skill 分两类来源：

- 仓库原创类（`skill-creator`、`studio-config`、`subagent-workflow`）直接检入源码树。
- 上游同步类（`canvas-design`、`docx`、`frontend-design`、`pdf`、`powerpoint`、
  `xlsx`）由 `cargo xtask sync-skills` 手动同步：浅拉取上游默认分支最新提交，完全替换
  同名技能目录并提交进源码库；源码库即 canonical 内容，构建期不访问网络。上游来源、
  最近同步 revision 与许可记录在 `code/pl-studio-runtime/THIRD_PARTY_NOTICES.md`。

同步命令通过 git 子进程完成：克隆缓存位于 `target/xtask-sync-skills/`，浅拉取远程默认
分支（大仓库使用 blobless partial clone 加 sparse checkout 缩小下载），所有 git 调用
关闭 `core.autocrlf` 保证行尾确定。替换前校验每个选中技能存在 `SKILL.md` 且 frontmatter
的 name 与目录一致、description 非空，避免把损坏技能提交进源码库；其余 frontmatter 完整
性仍由启动刷新统一校验。同步命令的技能清单必须与 `EXPECTED_SYSTEM_SKILLS` 人工保持
一致；从清单移除技能时需手动删除对应目录。上游同步类的许可必须允许再分发
（Apache 2.0、MIT 等）；`anthropics/skills` 的 `pdf`/`docx`/`pptx`/`xlsx` 使用禁止再分发
的 Anthropic 专有许可，不得预置，文档技能使用 MIT 许可的 `NousResearch/hermes-agent`
版本。不使用 build.rs 在构建期下载上游内容。

每个预置 skill 以独立目录保存，主文件仍为
`SKILL.md`；`pl-studio-runtime` 通过启用 zstd 压缩、debug embed 与确定性时间戳的
`rust-embed` 将资源打进所有构建模式，并在每次 `startStudioRuntime` 把完整资源树重建到
`<studio_home>/studio/skills/.system/`。缓存目录由 Pure 管理，不是源码，用户不应手动编辑；
若需要覆盖系统 skill，应在项目目录创建同名 skill。

启动刷新先完整验证嵌入路径与全部 `SKILL.md`，再以不跟随 symbolic link 或 Windows reparse
point 的方式删除旧目标，在同一父目录写入暂存树并通过 rename 发布。失败时 runtime 不进入
ready，且不得暴露半成品目录。刷新不使用版本 marker，连续两次启动也必须全量重建；
`[skills.system].enabled` 只控制系统来源是否参与发现，不控制资源刷新。旧
`<skills.user_dir>/.system` 只有在包含 Pure 旧 marker 且与新目录不同时才允许清理。

`rust-embed` 的压缩路径会引入 `include-flate`、zstd 与 libflate 相关编译依赖，增加编译时间和
二进制构建成本；`rust-embed 8.12.0` 上游标记为被动维护且采用 MIT license，其压缩链采用
permissive license。Studio 接受该成本以换取真正的压缩嵌入。由于 Cargo 会把同版本依赖的
feature 合并，而现有 `utoipa-swagger-ui` 的绝对生成目录与 compression feature 不兼容，仓库固定
保留 crates.io 发布包中未经修改的 `rust-embed` 与 `rust-embed-impl` source isolation 副本；副本
记录上游版本、校验和与许可，不作为可修改的产品源码，也不是 Git submodule。
PDF、DOCX 运行依赖、Git submodule 与第三方资源许可清单不属于本合同，后续单独选型。

Pure Studio 配置指南系统 skill 名为 `studio-config`，其主文件位于
`code/pl-studio-runtime/assets/skills/studio-config/SKILL.md`。配置契约及该 skill 的同步维护要求见
`10-config.md`。

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

`name` 和 `description` 必填。`category`、`platforms`、`disable-model-invocation` 与
`user-invocable` 可选；后两者分别默认 `false` 和 `true`，在核心模型中投影为正向
`model_invocable` / `user_invocable`。调用策略字段类型无效时整个 skill 失败关闭并产生 warning。
`platforms` 缺失表示所有平台可用。

支持文件只允许放在以下目录：

- `references/`
- `templates/`
- `scripts/`
- `assets/`

工具读取和写入支持文件时必须拒绝 path traversal，并拒绝访问这些目录之外的文件。上游
技能若包含白名单之外的目录（如 `canvas-design` 的 `canvas-fonts/`、hermes 文档技能的
`tests/`），内容仍随技能完整物化，但 `skill_view` 无法把它们当作支持文件读取。

## 13.4 工具

默认工具集中新增：

- `skills_list(category?)`：列出启用且允许模型调用的 skill 简短索引。
- `skill_view(name, filePath?)`：通过冻结 candidate 的 Provider 读取完整 `SKILL.md` 或支持文件；Simple 模式同时记录项目 skill 使用统计，Task 模式只读且不更新统计。省略 `filePath`、传空字符串、`.` 或 `SKILL.md` 都表示读取主文档；只有真正的支持文件路径才必须位于 `references/`、`templates/`、`scripts/` 或 `assets/` 下。
- `skill_manage(action, ...)`：管理项目目录中的 skill。

`skill_manage` 支持 `create`、`patch`、`edit`、`delete`、`writeFile`、`removeFile`。所有写入都只作用于 `<workspace_root>/<project_dir>/`。写入动作进入现有工具审批流程。`patch.oldString` 首先按 `SKILL.md` 原文字面量匹配；若完全匹配失败，运行时只允许把看起来像 JSON string fragment 的模型输出交给 `serde_json` 解码一层，再按同一匹配数量规则替换，避免因为 JSON/Markdown 二次转义噪声导致可恢复 patch 失败。运行时不维护额外的手写转义替换表。

`skill_view` 永不进入通用只读工具缓存。每次调用都重新通过 Provider 加载正文，并重新校验名称和
模型调用策略；candidate 的身份或权限已经变化时拒绝陈旧结果并使 Provider 失效。主文档响应只
返回资源基底和按需读取说明，不递归枚举资源；支持文件由胜出 Provider 的 `read_resource` 实现。
本合同不限制主文档或支持文件大小。

## 13.5 Prompt 和 Subagent

当 `[skills].enabled = true` 时，核心 turn 在 base instructions 与项目记忆之间注入允许模型调用的
skills 索引和使用规则。索引只包含空白规范化后的名称和 description，不包含路径、rank、Provider
locator 或正文。任务明显匹配某个 skill 时，模型必须先调用 `skill_view(name)` 读取完整内容。
`enabled = false` 同时关闭目录、工具和用户 `/name` 手势，工具不可见时不得注入调用指引。

`register_default_tools` 是 root agent 和 subagent 的共同工具入口，因此 subagent 与父 agent 使用同一项目 skills 上下文和同一加载优先级。

系统 skills 与用户/外部 skills 一样对 root agent 和 subagent 可见，但只读。模型如需沉淀新的项目经验，必须通过 `skill_manage` 写入项目目录。

Studio 状态栏的 Skills 只展示当前 Thread 已激活的 skills。激活定义为该 Thread 中成功的
`skill_view`，或直接用户输入中按空白边界精确匹配 `/name` 且该 skill 允许用户调用。用户手势按
首次出现顺序去重，保留原始用户文本，并把与工具加载共用的规范 `<skill_content>` 包装作为 Turn
级 user instruction 注入，同时明确提示模型无需再调用 `skill_view`。未知名称、路径、分数和用户
调用已禁用的名称继续作为普通文本；已确认可调用 skill 的加载失败则终止 Turn 准备。

每次成功加载都由后端记录为 `SkillActivated` runtime fact，来源是带不变量的
`Tool { tool_call_id }` 或 `UserGesture { invocation_id }`，
并投影为独立、终态的 durable Skill Timeline Item；重复读取同一 skill 仍保留多条激活
Item，同一 Turn 同名用户手势只产生一次。Item ID 分别由 Turn ID 与 tool call ID、Turn ID 与用户
invocation ID 确定。`ThreadRuntimeSnapshot.activeSkills` 按 Skill Item 的首次出现顺序去重，并在冷恢复时
由持久化 Item 重建，不另设平行 activation 表。前端只消费 typed Skill Item 和 runtime
snapshot，不解析工具输出 JSON。

Studio 设置页的 Skills 标签页展示 `SkillCatalogRuntime` 已发布的完整项目 catalog，包括调用策略、
Provider、warning 和完整性。进入标签页只读缓存，不访问文件系统；显式
`discoverSkills(projectId)` 强制扫描。每个新代理 Turn 的准备阶段也强制扫描一次；完整 catalog
内容不变时不增加公开 revision。该列表不调用 `skill_view`，不改变会话 active skills，也不写入
使用统计。

注册表发现返回 `SkillProviderObservation { candidates, complete, warnings }`。确认缺失的目录是完整
空结果；单个格式错误产生 warning 并跳过；意外 I/O、Provider 暂时失败或发现期间代次连续变化
产生不完整观测。代次在发现期间变化时重试一次，再次变化则发布不完整观测。已有完整 catalog 时
不完整结果保留 last-good 并发布 Degraded；首次发现失败时该 Turn 不注入目录、不注册 Skill 工具，
但普通任务仍可执行。

TurnFactory 在每个新代理 Turn 扫描后冻结 winner、Provider locator 和 revision。`SkillCatalog` 的
系统来源必须由 Studio 显式传入，不得从 `[skills].user_dir` 推导；非 Studio 调用不传系统目录。
`skills_list` 与 `skill_view` 只使用冻结 catalog，不会在同一 Turn 的模型工具迭代中重新 discover；
`skill_manage` 也以冻结 catalog 校验目标，写入成功后使注册表失效，新结果只在下一 Turn 生效。
system Skills 只在
`startStudioRuntime` 全量刷新，不在 Project discover 时刷新，也不设置隐式 filesystem watcher。

## 13.6 自学习

`StudioRuntime::run_prompt` 完成 Simple 模式主 turn 并保存记录后，如果 `[skills].auto_learn = true`，后台启动 reviewer 复盘本轮对话。reviewer 只注册 skills 工具，不注册 shell、文件或 subagent 工具。Task 模式不启动自学习 reviewer；从规划开始到任务终态，项目 workspace 的写入由 Task coordinator 独占。

自学习默认写入项目目录。reviewer 优先修补本轮已读取的项目 skill，其次修补已有项目 umbrella skill，最后才创建新的泛化 skill。reviewer 不得修改系统、用户或外部 skill；如果系统 skill 给出了通用指导，而本轮产生了项目特定经验，应创建或更新项目 skill。reviewer 不应记录一次性任务、瞬时环境失败、负面工具断言或纯用户私密偏好。

自学习失败只写日志，不影响用户 turn 的结果。
