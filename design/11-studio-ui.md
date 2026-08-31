# 11 - Pure Studio UI

## 11.1 边界

Pure Studio 是 Flutter 桌面应用，使用 Material 3、Riverpod、`go_router` 与 typed FRB。UI 只能通过
bridge 访问 StudioRuntime，不读取 SQLite、Agent TOML 或 Skill 文件。Flutter Web 只用于 demo
integration 验收，不能伪造原生 provider、文件系统或进程能力。

data 层负责 FRB DTO 到 domain 的一次转换；reducer 只接收 canonical snapshot/notification；Widget
只负责展示与发命令。窗口关闭必须等待 typed shutdown 完成并回收 Flutter、DTD、MCP/LSP 和 child
process tree。

## 11.2 动态模式

新 Thread 默认选择 `mode.simple`。composer 的模式 selector 读取 Mode Skill catalog，使用稳定
`modeId` 作为值、`displayName` 作为文案，不写死 Simple/Task enum。模式切换命令只在 root idle、无
pending interaction 且 workflow 不存在或已终止时可用；拒绝结果由 canonical snapshot 恢复 UI。

Mode Skill 不出现在普通 Skills 设置和按需调用列表。两个内置模式不可删除或覆盖。

## 11.3 Workflow panel

Thread runtime 只暴露通用 workflow projection。panel 显示 mode、run、revision、lifecycle、当前阶段、
合法后继、constraint prompt 与最近 history；graph/history 使用稳定 `ValueKey` 供 Driver 读取。未编译
时显示 compile 指引，terminal 状态仍可展开完整路径。

panel 不根据阶段 ID 推演动作，不等待可能瞬间经过的中间 Widget。状态变更只来自 bridge snapshot；
GUI 不执行本地乐观 transition。旧 recovery、WorkUnit、delivery review、merge 和 completion gate UI
全部不存在。

## 11.4 通用 Interaction

composer dock 只渲染 `UserInput` 与 `ToolApproval`。任务计划确认是普通 UserInput：显示 Agent 已输出的
计划和问题选项，用户选择后提交 typed resolution。不存在模式专用确认 dock 或 continuation。

## 11.5 Agents 设置

Agents 页同时列出系统和用户 Profile。系统项所有字段只读且无删除入口，只提供 enabled switch；用户
项按单 TOML 文件原子创建、保存或删除。无效文件以独立诊断展示，不阻断页面其余项。运行中的 Agent
目录与 Profile 设置目录明确分区。

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
模式 selector、composer、通用 Interaction、workflow panel/history 与 shutdown。workflow live harness
在 terminal 后读取 canonical history，不以轮询瞬时阶段作为通过条件。
