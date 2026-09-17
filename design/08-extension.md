# 08 - 扩展点索引

扩展优先数据化：新能力尽量表现为 catalog、preset 或配置数据，而不是新的代码分支；不猜测厂商
能力。各扩展方向的完整合同见对应专篇，本文只给入口与边界。

| 扩展方向 | 边界摘要 | 专篇 |
| --- | --- | --- |
| Provider | 新 wire 协议在 pl-model 增加 typed adapter；兼容 Responses/Chat Completions 的供应商通过 catalog preset、模型元数据与 request profile 数据化扩展。产品配置只保存 provider 实例、凭据引用、endpoint override 与模型选择。 | [06](./06-model.md)、[20](./20-config.md) |
| 普通 Skill | 按优先级目录发现，`skill_list` / `skill_view` 按需加载；frontmatter 与正文由 pl-skill-core 统一解析，非法资源只产生自身诊断；普通 Skill 不得使用 `mode.` 前缀。 | [10](./10-skills.md) |
| Thread Mode | 注册制扩展，不复用 Skill、Provider 或发现目录。内置 `mode.simple` 与 `mode.task` 随二进制发布；未来文件加载器只把外部配置转换为相同的所有权结构后按来源原子注册，不同来源不能注册同一 ID，内置 ID 不可覆盖。注册输入可携带预设图，runtime 在发布目录前完成编译；模型只能用拆分后的查询与转换工具读取和推进图。 | [11](./11-thread-mode.md) |
| Agent Profile | 用户可在数据目录的 agents 子目录按 TOML 文件增加 Profile，文件名 stem 是稳定 id。系统 Profile 在内置注册表注册，全字段只读且不可删除，只能通过 `disabled_system_agents` 配置启停；协作工具只接受 profileId，不暴露任意临时 system prompt 注入入口。 | [12](./12-collaboration.md)、[20](./20-config.md) |
| 工具 | 新增持久状态工具应采用 typed args/result、稳定错误码、显式 CAS 与 operation identity，并在 working-state 克隆上计算后进入统一 checkpoint；批处理声明 Coexist 或 Solo。业务工具不得绕开 Thread owner 写第二套事实。 | [09](./09-tool-runtime.md) |
