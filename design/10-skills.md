# 10 - 项目级 Skills

## 10.1 目标与所有权

Skills 是可复用的任务知识文档，供 agent 在需要时按需读取。pl-tool 的 skill 模块提供与来源
无关的 Provider、注册表、发现观测和 Turn 级冻结 catalog；Studio 只拥有进程级 Provider 注册
表、项目 catalog 投影与系统资源目录。系统默认只把自学习产物写入当前项目，避免把项目经验
污染到用户全局配置。当前不实现在线 Hub、安装市场、文件 watcher、平铺 `<name>.md` 或用户级
自动写入。

默认能力闭环为：发现本地与远端项目 skills；向模型注入简短索引；通过工具读取完整 skill 或
支持文件；通过工具创建、修补和删除项目级 skill；每轮结束后由后台 reviewer 自动沉淀可复用
经验。

来源注册、冻结 catalog 构造和 Skill 工具安装是公共产品宿主能力，不是 Studio 私有实现细节。
产品可能从容器 sidecar、只读 Review revision、产品缓存或其他 durable 资源得到 Skill 根；
这些资源的准备和生命周期属于产品，但 SKILL.md 解析、winner 选择、冻结 generation、正文与
支持文件读取、激活事实和模型工具语义必须由 PL 统一拥有。任意工具宿主必须能够注册产品拥有的
Skill source、在一次 Turn 前冻结唯一 catalog，并把同一 catalog 同时用于用户显式调用、模型
索引和普通 Skill 工具；Provider 的 locator 与读取责任不得被强制收回 Studio，也不得要求产品
通过 JSON 或兼容适配层重建 PL Skill。`Project` 是 winner 排序与产品语义，不代表该 Skill 一定
位于默认项目目录，也不授权 PL 向默认目录写入另一个 Provider 的状态；产品提供的 Review
revision、容器卷或缓存快照可以标记为 Project，但仍保持只读；成功激活后的使用统计若存在，
必须由持有冻结 locator 的 Provider 在自己的信任根内处理。本文约束功能语义而不锁定具体
Rust 接口。

## 10.2 目录与优先级

运行时按以下优先级发现 skills：

1. 项目目录：`<workspace_root>/.agents/skills/`
2. 配置用户目录：`[skills].user_dir`，默认 `~/.anywork/skills/`
3. Agents 兼容用户目录：Linux `$HOME/.agents/skills/`、Windows `%USERPROFILE%\.agents\skills\`
4. Studio 系统目录：`<studio_home>/studio/skills/.system/`
5. 配置里的外部目录：`[skills].external_dirs`

这些目录由默认文件系统 Provider 映射为带不透明 locator 的候选。Provider 可以并行发现，重名
winner 依次按来源 rank、Provider 注册顺序和 Provider 本地顺序确定；同名 skill 只保留最高
优先级来源。显式配置的用户目录优先于 Agents 兼容目录，两者都标记为 User 来源；若两者解析为
同一路径，只扫描一次。项目目录是唯一写入目标；用户目录、系统目录和外部目录仅参与只读发现。
模型尝试修改非项目来源的 skill 时，工具必须拒绝原地修改，并提示在项目目录创建同名项目覆盖
或新建项目 skill。

`[skills].project_dir` 是相对工作区根目录的路径，默认 `.agents/skills`；解析后必须位于
workspace_root 内。显式配置的其他相对目录继续按配置使用，运行时不自动搬迁旧目录。

项目 skills 路径按主机文件边界处理：项目目录、skill 目录、`SKILL.md`、支持文件和使用统计的
已有祖先都不能是 symbolic link 或 Windows reparse point。发现和支持文件索引跳过链接入口；
`skill_view` 与 `skill_manage` 直接访问链接时拒绝。删除 skill 时，skill 子树内的链接只删除
入口，不能递归进入或修改其目标。用户、系统和显式 external source 仍是只读来源，但其内部
发现同样不跟随链接。

### 预置系统 Skills

Studio 预置 skills 归 pl-studio-runtime 所有，canonical 内容检入其源码树（assets/skills），
分两类来源：

- 仓库原创类（`skill-creator`、`studio-config`、`subagent-workflow`）直接检入源码树。
- 上游同步类（`canvas-design`、`docx`、`frontend-design`、`pdf`、`powerpoint`、`xlsx`）由
  仓库统一的同步命令手动同步：浅拉取上游默认分支最新提交，完全替换同名技能目录并提交进
  源码库；源码库即 canonical 内容，构建期不访问网络。替换前校验每个选中技能存在
  `SKILL.md` 且 frontmatter 的 name 与目录一致、description 非空；上游来源、最近同步
  revision 与许可记录于第三方声明文件。同步命令的技能清单必须与运行时预期的系统技能清单
  人工保持一致。上游同步类的许可必须允许再分发（Apache 2.0、MIT 等）；禁止再分发的专有
  许可技能不得预置。

每个预置 skill 以独立目录保存，主文件仍为 `SKILL.md`；pl-studio-runtime 通过启用压缩与
确定性时间戳的资源嵌入把技能打进所有构建模式，并在每次 Studio 运行时启动时把完整资源树重建
到 `<studio_home>/studio/skills/.system/`。缓存目录由 Pure 管理，不是源码，用户不应手动编辑；
若需要覆盖系统 skill，应在项目目录创建同名 skill。

启动刷新先完整验证嵌入路径与全部 `SKILL.md`，再以不跟随 symbolic link 或 Windows reparse
point 的方式删除旧目标，在同一父目录写入暂存树并通过 rename 发布。失败时 runtime 不进入
ready，且不得暴露半成品目录。刷新不使用版本 marker，连续两次启动也必须全量重建；
`[skills.system].enabled` 只控制系统来源是否参与发现，不控制资源刷新。

anywork 配置指南系统 skill 名为 `studio-config`；配置契约及该 skill 的同步维护要求见
[20](./20-config.md)。

## 10.3 Skill 格式

每个 skill 是一个目录，主文件固定为 `SKILL.md`，文件开头使用 YAML frontmatter：

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
model_invocable / user_invocable。调用策略字段类型无效时整个 skill 失败关闭并产生 warning。
`platforms` 缺失表示所有平台可用。

支持文件只允许放在 `references/`、`templates/`、`scripts/`、`assets/` 目录。工具读取和写入
支持文件时必须拒绝 path traversal，并拒绝访问这些目录之外的文件。上游技能若包含白名单之外
的目录，内容仍随技能完整物化，但 `skill_view` 无法把它们当作支持文件读取。

## 10.4 工具

默认工具集包含：

- `skills_list(category?, query?, limit?)`：列出启用且允许模型调用的 skill 简短索引；无
  query 时返回完整、按名称排序的 model-invocable catalog，有 query 时 category 先过滤，再
  使用统一召回器返回正分结果和截断状态。
- `skill_view(name, filePath?)`：通过冻结 candidate 的 Provider 读取完整 `SKILL.md` 或支持
  文件。省略 `filePath`、传空字符串、`.` 或 `SKILL.md` 都表示读取主文档；只有真正的支持文件
  路径才必须位于四个支持目录下。始终保持精确名称语义，不接受模糊名称；项目 Skill 的使用
  统计与 Thread Mode 无关，Thread Mode 不属于 Skill，也不能由该工具按需调用。
- `skill_manage(action, ...)`：管理项目目录中的 skill，支持 create、patch、edit、delete、
  writeFile、removeFile。所有写入都只作用于 `<workspace_root>/<project_dir>/`，并进入现有
  工具审批流程。`patch.oldString` 首先按 `SKILL.md` 原文字面量匹配；完全匹配失败时，运行时
  只允许把看起来像 JSON string fragment 的模型输出解码一层，再按同一匹配数量规则替换，避免
  JSON/Markdown 二次转义噪声导致可恢复 patch 失败；不维护额外的手写转义替换表。

`skill_view` 永不进入通用只读工具缓存：每次调用都重新通过 Provider 加载正文，并重新校验
名称和模型调用策略；candidate 的身份或权限已经变化时拒绝陈旧结果并使 Provider 失效。主文档
响应只返回资源基底和按需读取说明，不递归枚举资源；支持文件由胜出 Provider 的资源读取实现，
成功读取后的 Provider 专属 view 记录也委托给同一个冻结 Provider。默认文件系统 Provider 只为
自己按标准 workspace 配置发现的可写 Project Skill 更新 `.usage.json`；宿主显式注册的目录
视为只读快照，即使来源为 Project 也不写统计。必须保留"冻结 candidate 的 Provider 拥有
副作用和信任根"这一功能语义。本合同不限制主文档或支持文件大小。

## 10.5 Prompt 注入与召回

`[skills].enabled = true` 时，核心 turn 在 base instructions 与项目记忆之间注入允许模型调用
的 skills 索引和使用规则。索引只包含空白规范化后的名称和 description，不包含路径、rank、
Provider locator 或正文；description 只在模型目录投影时折叠空白并截断为 500 个 Unicode
字符（catalog、Studio 与 Provider 仍保存完整 description）。任务明显匹配某个 skill 时，模型
必须先调用 `skill_view(name)` 读取完整内容。`enabled = false` 同时关闭目录、工具和用户
`/name` 手势；工具不可见时不得注入调用指引。

每个普通 agent turn 还根据本轮原始文本，从冻结 catalog 的 name 与完整 description 中确定性
召回最多 5 个候选，并将摘要作为 Turn 级 user overlay 追加在稳定 instructions 前缀之后。召回
只考虑允许模型调用的普通 Skill，排除本轮已由用户手势直接加载的名称；无正分候选时不注入
overlay。候选建议不读取正文、不激活 Skill，也不替代 `skill_view`；独立 compaction 不重新
生成候选建议。

召回器是 pl-tool skill 模块的共享纯函数边界：query 与单个文档都有界截断，评分依次奖励完整
name 短语、精确 name、name 词项与前缀、description 词项与前缀，再加命中 query 词项数的覆盖
奖励；零分候选不返回，同分按名称稳定排序；Provider rank 只决定同名 winner，不能混入相关度
分数。前端与调用方不复制评分器，非空 query 只消费后端排序结果。

root agent 和 subagent 使用同一工具入口，因此共享同一项目 skills 上下文和同一加载优先级。
系统 skills 与用户/外部 skills 一样对 root 和 subagent 可见，但只读；模型如需沉淀新的项目
经验，必须通过 `skill_manage` 写入项目目录。

## 10.6 激活与投影

Studio 状态栏的 Skills 只展示当前 Thread 已激活的 skills。激活定义为该 Thread 中成功的
`skill_view`，或直接用户输入中按空白边界精确匹配 `/name` 且该 skill 允许用户调用。用户手势
按首次出现顺序去重，保留原始用户文本，并把与工具加载共用的规范 `<skill_content>` 包装作为
Turn 级 user instruction 注入，同时明确提示模型无需再调用 `skill_view`。未知名称、路径、
分数和用户调用已禁用的名称继续作为普通文本；已确认可调用 skill 的加载失败则终止 Turn
准备。

每次成功加载都由后端记录为 SkillActivated runtime fact，来源是带不变量的
`Tool { tool_call_id }` 或 `UserGesture { invocation_id }`，并投影为独立、终态的 durable
Skill Timeline Item；重复读取同一 skill 仍保留多条激活 Item，同一 Turn 同名用户手势只产生
一次。Item ID 分别由 Turn ID 与 tool call ID、Turn ID 与用户 invocation ID 确定。runtime
snapshot 的 activeSkills 按 Skill Item 的首次出现顺序去重，并在冷恢复时由持久化 Item
重建，不另设平行 activation 表；前端只消费 typed Skill Item 和 runtime snapshot，不解析
工具输出 JSON。

Studio 设置页的 Skills 标签页展示已发布的完整项目 catalog，包括调用策略、Provider、warning
和完整性；进入标签页只读缓存，不访问文件系统，显式发现命令强制扫描。每个新代理 Turn 的
准备阶段也强制扫描一次；完整 catalog 内容不变时不增加公开 revision。该列表不调用
`skill_view`，不改变会话 active skills，也不写入使用统计。Studio 的非空搜索通过 runtime
对已发布 catalog 执行同一召回器，返回 catalog revision、完整 Skill summaries 与截断状态；
它不触发 discover、revision 变化或 activation。Studio 搜索包含全部普通 skills，Agent 与
`skills_list` 调用方则先过滤为 model-invocable skills。

注册表发现返回观测结构（候选列表、完整性、警告）：确认缺失的目录是完整空结果；单个格式
错误产生 warning 并跳过；意外 I/O、Provider 暂时失败或发现期间代次连续变化产生不完整观测，
代次变化时重试一次，再次变化则发布不完整观测。已有完整 catalog 时不完整结果保留 last-good
并发布 Degraded；首次发现失败时该 Turn 不注入目录、不注册 Skill 工具，但普通任务仍可执行。

每个新代理 Turn 在扫描后冻结 winner、Provider locator 和 revision。Skill catalog 的系统来源
必须由 Studio 显式传入，不得从 `[skills].user_dir` 推导，非 Studio 调用不传系统目录。
`skills_list` 与 `skill_view` 只使用冻结 catalog，不在同一 Turn 的模型工具迭代中重新
discover；`skill_manage` 也以冻结 catalog 校验目标，写入成功后使注册表失效，新结果只在
下一 Turn 生效。system Skills 只在 Studio 运行时启动时全量刷新，不在项目 discover 时刷新，
也不设置隐式 filesystem watcher。

## 10.7 自学习

root turn 完成并保存记录后，如果 `[skills].auto_learn = true`，后台启动 reviewer 复盘本轮
对话。reviewer 只注册 skills 工具，不注册 shell、文件或 subagent 工具；该行为不按 Thread
Mode 分支，也不拥有或推进 root workflow，workspace 写入和 Agent 协调仍由 root 对话负责。

自学习默认写入项目目录。reviewer 优先修补本轮已读取的项目 skill，其次修补已有项目
umbrella skill，最后才创建新的泛化 skill；不得修改系统、用户或外部 skill。系统 skill 给出
通用指导而本轮产生项目特定经验时，应创建或更新项目 skill。reviewer 不应记录一次性任务、
瞬时环境失败、负面工具断言或纯用户私密偏好。自学习失败只写日志，不影响用户 turn 的结果。
