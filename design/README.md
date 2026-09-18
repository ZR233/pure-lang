# 设计文档索引

本目录是 Pure-Lang / anywork 的架构设计文档集。文档只描述设计目的、原理、接口形状、
逻辑与约定；具体实现细节留在代码、类型签名与必要注释中。

## 阅读顺序

| 编号 | 文档 | 内容 |
| --- | --- | --- |
| 一、总览与架构 | | |
| 01 | [系统总览](./01-overview.md) | 定位、核心概念与事实源、运行路径、恢复与关闭摘要 |
| 02 | [Crate 边界](./02-crates.md) | 依赖方向与各 crate 稳定职责（唯一权威源） |
| 03 | [Thread / Turn / Item 流程](./03-pipeline.md) | 输入受理、模型循环、工具批处理、Interaction 模型 |
| 04 | [安全边界](./04-security.md) | 权限模式（唯一权威源）、路径与凭据边界、transport 边界 |
| 05 | [通用约定](./05-conventions.md) | 参数与类型设计、模块导出、生命周期状态机模式、后台进程与外部服务 |
| 二、模型与流 | | |
| 06 | [模型层](./06-model.md) | provider 适配、模型目录、transport、流归一化、effort 机制、计量 |
| 07 | [Thread 实时流](./07-streaming.md) | 帧模型、通知、背压、reducer、product stream、transport |
| 三、工具与扩展 | | |
| 08 | [扩展点索引](./08-extension.md) | Provider / Skill / Mode / Profile / 工具的扩展入口 |
| 09 | [工具调用运行时](./09-tool-runtime.md) | 不透明工具身份、注册与撤销、调度声明、MCP 租约、文件缓存 |
| 10 | [项目级 Skills](./10-skills.md) | 目录优先级、格式、工具合同、注入与召回、自学习 |
| 四、协作与编排 | | |
| 11 | [Thread Mode 与预设工作流](./11-thread-mode.md) | Mode 注册、图协议与编译、workflow 工具契约、持久化上限 |
| 12 | [Agent Profile 与协作编排](./12-collaboration.md) | 工作区模式、spawn、durable delivery、wait、reviewer 门禁、返工 |
| 13 | [Thread Plan 状态机](./13-plan.md) | Plan 聚合、`plan_*` 工具合同、确认 UI 边界 |
| 五、运行时与存储 | | |
| 14 | [Thread Runtime 宿主](./14-runtime-host.md) | 统一装配、激活、输入受理、步骤限制、后台任务与消息唤醒 |
| 15 | [会话条目存储](./15-session-storage.md) | 存储信封、原子重放、异步保存与背压、冷恢复 |
| 16 | [Core 内核契约](./16-core-contracts.md) | 不透明载荷、模型调用、扩展 CAS、能力归属、计量回执 |
| 17 | [Studio 存储与诊断](./17-studio-storage.md) | 双库、checkpoint 与分页、恢复与版本迁移、实现缺口、诊断、LRU |
| 18 | [Studio 状态查询](./18-studio-state.md) | CQS、公共 snapshot、activation、shutdown、title、产品投影 |
| 六、Studio 产品 | | |
| 19 | [anywork UI](./19-studio-ui.md) | UI 边界、模式与状态、主题、设置页组织、Timeline、历史阅读 |
| 20 | [Studio 持久化配置](./20-config.md) | 配置 schema、provider/model、提示词、MCP/Skills/LSP、凭据 |
| 21 | [LSP Runtime](./21-lsp.md) | catalog 与 driver、registry CQS、状态模型、工具能力 |
| 22 | [SSH 远程开发](./22-ssh-remote.md) | helper 能力代理、最小协议、连接管理、凭据与路径、helper 嵌入 |
| 23 | [发布与更新](./23-release-update.md) | 发布渠道与信任根、Windows 包边界、更新清单与状态机、诊断 |

配套资产：`concepts/` 保存产品概念设计（如项目侧栏），`prototypes/` 保存视觉原型
（HTML 稿，不含架构约定），`assets/` 保存文档引用的图片。

## 写作规范

- 只描述设计目的、原理、接口形状、逻辑与约定；可用关系图和最小配置或协议示例说明契约，
  不展开实现代码、方法签名、内部模块路径、源文件路径或行号。crate 名、协议/工具/事件/wire
  字段名、配置键与库表名属于接口形状，可以保留。
- 单一事实源：每个事实只有一个权威文档，其他文档链接它，不重复展开。上表括号中标注了
  各权威源。架构演进原则归 [AGENTS.md](../AGENTS.md)，数据库与会话迁移归 [17](./17-studio-storage.md)，
  配置迁移归 [20](./20-config.md)；规范与实现存在差距时明确标注，不能把目标契约写成已验证能力。
- 行为数值契约（步骤上限、持久化上限、超时、截断长度等）在对应文档中维护；schema 版本、
  依赖版本、模型价格等易变运营数据以代码中的常量与目录为准，文档不保存快照。
- 命名口径：面向用户与 Agent 系统提示词使用产品名「糊来帮」（英文 `anywork`）；仓库与技术标识
  为 `pure-lang`，Rust crate 统一 `pl-` 前缀。
- 小节使用 `N.M` 编号，与文档编号对应；命令入口、CI 与验收操作细节归 AGENTS.md、技能
  与仓库工具，不写入设计文档。
