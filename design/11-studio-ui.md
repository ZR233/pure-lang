# 11 - Pure Studio UI

## 11.1 边界

Pure Studio 是 Flutter 桌面应用，使用 Material 3、Riverpod、`go_router` 与 typed FRB。UI 只能通过
bridge 访问 StudioRuntime，不读取 SQLite、Agent TOML 或 Skill 文件。Flutter Web 只用于 demo
integration 验收，不能伪造原生 provider、文件系统或进程能力。

data 层负责 FRB DTO 到 domain 的一次转换；reducer 只接收 canonical snapshot/notification；Widget
只负责展示与发命令。窗口关闭必须等待 typed shutdown 完成并回收 Flutter、DTD、MCP/LSP 和 child
process tree。

## 11.2 动态模式

新 Thread 默认选择 `mode.simple`。composer 的模式 selector 读取 `ThreadModeCatalogSnapshot`，使用
稳定 `modeId` 作为值、`displayName` 作为文案，不写死 Simple/Task enum。模式切换命令只在 root idle、
无 pending interaction 时可用；旧活动 run 由 runtime 归档，拒绝结果由 canonical snapshot 恢复 UI。

Thread Mode 不出现在普通 Skills 设置和按需调用列表。两个内置模式不可删除或覆盖。

## 11.3 Workflow status

Thread runtime 只向 GUI 暴露状态栏需要的通用 workflow projection：mode、run、revision、lifecycle
与当前阶段。未开始的图模式显示“未开始”；Simple 只显示 Mode。

GUI 不提供完整 graph、history、展开详情或人工 transition，也不根据阶段 ID 推演动作。状态变更只来自
bridge snapshot，不执行本地乐观 transition。旧 recovery、WorkUnit、delivery review、merge 和
completion gate UI 全部不存在。

## 11.4 通用 Interaction

composer dock 只响应 `UserInput` 与 `ToolApproval`。任务计划由 `plan_submit` 通过固定 Plan 状态机发起，
协议上仍是通用 UserInput；GUI 从其中稳定 ID 为 `plan_confirmation` 的唯一问题派生 Plan 展示，不增加
Interaction kind、持久化状态或第二套 continuation。pending 计划在 Timeline 尾部显示摘要卡，点击后在
右侧独立滚动面板展示完整 Markdown；宽窗口并排，窄窗口覆盖，展开状态仅为 per-Thread 临时 UI 状态。

Plan confirmation 期间，普通 composer 被计划反馈栏替换。用户可直接输入非空修改意见并提交 `Revise`
resolution，或直接确认 `Approve`；右侧计划面板只读且不重复放置操作。Interaction resolved 后这些派生 UI
同时消失并恢复普通 composer。普通澄清仍由 `request_user_input` 发起并使用既有分步问题 dock。

## 11.5 Agents 设置

Agents 页是唯一 Agent 配置中心，不再保留重复 Roles 页。系统项名称、用途、指令和固定 workspace
mode 只读，无删除入口；可配置 enabled、provider/model 与模型声明驱动的 effort。用户项按单 TOML
文件原子创建、保存或删除，并可选择三种 workspace mode。无效文件以独立诊断展示，不阻断页面其余项。
preserved worktree 显示 revision、branch、base/head、dirty/changed-files preview 与显式 cleanup。运行中
Agent 目录与 Profile 设置目录明确分区；所有 mutation 使用 settings revision CAS 并以 canonical
snapshot 原子刷新。

Agents 导航、配置页、preserved worktree recovery 和用户 Profile 详情中的固定界面文案必须跟随
Studio locale，并由统一 l10n catalog 提供。Profile ID、provider/model、effort、workspace mode 的
持久化值，以及分支、路径、commit、worktree 状态和诊断数据保持 canonical 原值；本地化只影响
展示标签、说明、操作和校验反馈，不得改写配置或运行时数据。

## 11.6 联网搜索设置

General 页把 OpenAI Web Search 与 DeepSeek 原生联网搜索显示为两张独立卡片。OpenAI 卡片保留
mode、context size、域名和位置等配置，并明确文案只表示 OpenAI 搜索；DeepSeek 卡片只提供启用
开关，不展示官方未承诺的 cached/indexed、域名、位置或上下文选项。

两张卡片都消费 bridge 返回的 configured、effective、availability、selected provider/model。
当前 DeepSeek route 可用时其原生搜索优先，OpenAI 卡片仍可显示“可用但未选中”。保存必须携带
Settings CAS revision，并以返回的完整 canonical snapshot 原子更新 UI；不得本地推演 backend
仲裁、凭据或模型能力。

## 11.7 驱动验收

原生 GUI 必须通过 `cargo xtask run-gui --driver` 启动，Flutter Driver 使用稳定 key 操作项目、Thread、
模式 selector、composer、通用 Interaction、workflow status、Thread title 与 shutdown。workflow live
harness 从 provider wire、tool receipt 与 canonical runtime snapshot 验证完整历史，并在 terminal 后读取
canonical history，不依赖 GUI 详情或轮询瞬时阶段。

## 11.8 Thread title

新会话首条 prompt 提交后，侧栏和会话页眉立即显示 prompt 摘要；Explorer model 生成的最终 title
通过 `ThreadDirectoryChanged` 更新，两处始终从同一个 `StudioState` projection 渲染。UI 不显示独立
的“正在命名”状态，也不在本地维护第二份 title。

Thread tile 在悬停或键盘聚焦时提供 rename action，保存对话框提交 typed rename command；空标题和
超过 80 个字符的输入在 UI 与 runtime 两侧都拒绝。Driver 使用稳定 key 验证临时 title、自动 title、
手动 title 以及关闭重开后的恢复。

展开侧栏中的项目与 Thread 标题在鼠标悬停时显示 canonical name/title 的完整文本，紧凑侧栏也以
对应的完整 name/title 标识图标；截断只影响行内渲染，不改变提示内容。项目路径仍只作为展开布局的
辅助信息；项目或 Thread 存在 recovery issue 时，诊断详情优先于名称提示。
