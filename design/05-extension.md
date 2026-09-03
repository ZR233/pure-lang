# 05 - 扩展点

## 5.1 Provider

新 wire 协议在 `pl-model` 增加 typed adapter；兼容 Responses/Chat Completions 的供应商通过 catalog
preset、模型元数据和 request profile 数据化扩展。产品配置只保存 provider 实例、凭据引用、endpoint
override 与模型选择，不在 runtime 依据厂商名称猜测能力。

## 5.2 普通 Skill

普通 Skill 沿用 provider precedence、`skill_list`、`skill_view` 与按需加载。Skill frontmatter 和正文
由 `pl-skill-core` 解析；非法资源只产生自身诊断。普通 Skill 不得使用 `mode.` 前缀。

## 5.3 Thread Mode

Mode 通过 `ThreadModeRegistration` 注册，不复用 Skill frontmatter、Provider 或发现目录。内置
`mode.simple` 与 `mode.task` 使用 Rust 常量随二进制发布；未来文件加载器只负责将外部配置转换为相同
的拥有所有权结构，再按来源原子注册。不同来源不能注册同一 ID，内置 ID 不可覆盖。

注册输入可携带预设 `WorkflowDefinition`。Runtime 在发布目录前完成编译，模型只能用拆分后的查询与
转换工具读取和推进图，不能定义、compile 或 supersede 图。不使用 workflow 的 Mode 可以直接执行。
所有 root Mode 都通过统一 `complete` 工具提交完成摘要并结束 turn。

## 5.4 Agent Profile

用户可在 `~/.pure/agents/<id>.toml` 增加 Profile。文件名 stem 是稳定 id，一个文件包含 enabled、
介绍、适用任务、系统指令、provider、model 与 effort。单文件无效不阻断其他 Profile。

新增系统 Profile 需在 Rust builtin registry 注册。系统 Profile 全字段只读且不可删除，只能通过
`config.toml` 的 `disabled_system_agents` 启用或禁用。协作工具只接受 `profileId`，不暴露任意临时
system prompt 注入入口。

## 5.5 工具

工具实现声明 `ToolBatchPolicy::Coexist | Solo`。新增持久状态工具应采用 typed args/result、稳定错误码、
显式 CAS 与 operation identity，并在 working-state clone 上计算后进入统一 checkpoint。业务工具不得
绕开 Thread owner 写第二套事实。
