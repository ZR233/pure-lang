# 01 - 系统总览

## 1.1 定位

Pure-Lang 是自然语言编译器：接收自然语言需求，将其编译为可执行的计划、代码生成意图与后续动作建议。
anywork 桌面应用的唯一业务核心是 pl-studio-runtime 内的 Studio 运行时；Flutter/FRB 桌面端与独立
HTTP server 只是两个 transport，共享同一 runtime，不拥有第二套业务状态。

## 1.2 运行路径

```text
Flutter → pl-studio-bridge ─┐
                           ├→ Studio 运行时 → Thread 工厂 → Thread 装配器
HTTP → pl-studio-server ───┘                                  ↓
                                                     pl-core Thread owner
                                                       ↙             ↘
                                               pl-model 会话      pl-tool 实例
```

上图最后两条表示装配后的调用关系；crate 依赖方向是 model/tool → core，见
[02](./02-crates.md)。

## 1.3 核心概念与事实源

- Thread 是通用执行 owner：每个 Thread 独占模型会话、工具注册表和工具实例。
- Turn 是一次输入驱动的有界执行；model attempt 与工具 task 保存独立生命周期。
- 通用上下文条目包含角色、来源、文本、资源引用、不透明内容及工具调用关联。
- Item、Agent Profile、Mode、workflow 和 Plan 是产品概念，由 Studio 从保存的事实投影。
- Interaction 与 permission 是通用 pending/resolved/cancelled 事实或许可决定；问题和回答正文由
  上层解释。

模型和工具执行期间 owner 仍可受理消息、读取日志和撤销待执行权限；这些命令不改写已冻结的请求。

运行时事实支持完整替换与按 source 原子 patch：独立生产者只 patch 自己的 source，空内容清除该
source，遗漏 source 保持不变；两者都在 owner 串行边界执行，存在待交付工具调用时拒绝修改。

活动 Thread 的内存快照是唯一可写执行事实源。`sessions/<thread-id>.sqlite` 保存通用 journal 与不可变资源；
TOML 保存用户、模型与工作空间声明，`studio.sqlite` 保存动态项目状态与产品目录，各存储不共享业务事务。UI snapshot 与 Turn 分页从同一
日志水位生成；历史模型正文使用保存时的上下文，不调用当前工具重建。

## 1.4 恢复与关闭

纯重放只应用已保存事实，不创建模型、工具或外部副作用。恢复把遗留运行收束为 Interrupted，保留
待处理输入，清空物理 continuation 与旧 executor 授权；显式激活后装入当前服务实例。关闭先封闭准入
并取消当前代次，收束结果与交互，再关闭模型/工具并保存最终 commit；失败保留 owner 与重试入口，
归档只按目录装载冷历史，不为归档重新激活历史会话；先结束已驻留会话树的活动工作
（中断当前 Turn、取消运行中任务、丢弃未消费输入）并清理该会话树的 worktree 工作树，
全部关闭成功后再归档；
不能结束的会话保持显式失败，不静默丢弃现场。

anywork 数据格式演进由 Studio 在正常运行前协调迁移，契约与当前实现缺口见
[17](./17-studio-storage.md)；配置与凭据关联迁移见 [20](./20-config.md)。
通用存储与无副作用重放合同见 [15](./15-session-storage.md) 与 [16](./16-core-contracts.md)。

## 1.5 工具目录的条件发布

扩展（MCP、LSP 等）异步准备工具目录时，可以携带冻结快照的扩展水位条件发布：owner 在同一 mailbox
操作内验证水位后原子替换完整目录；过期候选保持未安装并由移交方异步关闭，拒绝不修改当前目录或
权限。已独占装配使用无条件注册；关闭失败保留类型化拒绝原因、cleanup 来源与原实例供明确重试。
工具身份、注册与批处理契约见 [09](./09-tool-runtime.md)。
