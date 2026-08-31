# 05 - 扩展点

## 5.1 Provider

新 wire 协议在 `pl-model` 增加 typed adapter；兼容 Responses/Chat Completions 的供应商通过 catalog
preset、模型元数据和 request profile 数据化扩展。产品配置只保存 provider 实例、凭据引用、endpoint
override 与模型选择，不在 runtime 依据厂商名称猜测能力。

## 5.2 普通 Skill

普通 Skill 沿用 provider precedence、`skill_list`、`skill_view` 与按需加载。Skill frontmatter 和正文
由 `pl-skill-core` 解析；非法资源只产生自身诊断。普通 Skill 不得使用 `mode.` 前缀。

## 5.3 Mode Skill

新增模式只需增加名为 `mode.<custom-id>` 的 Skill，并提供合法 `mode.display-name`、`mode.order`，同时
关闭普通模型/用户临时调用。无需修改 Rust enum、GUI selector 或 model loop。自定义同名模式按普通
来源优先级选 winner；`mode.simple` 与 `mode.task` 只接受 Studio 内置 Provider，外部同名候选忽略并
告警。

Mode Skill 可以要求 Agent 使用 `workflow_state.compile`，定义阶段、完成标准、terminal 与转换路径；
不使用 workflow 的 Mode 可以直接执行。运行时只编译和执行通用图协议，不理解阶段业务含义。所有
root Mode 都通过统一 `complete` 工具提交完成摘要并结束 turn。

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
