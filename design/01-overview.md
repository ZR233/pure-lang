# 01 - 系统总览

Pure-Lang 是自然语言编译器。Pure Studio 的业务核心为 `pl-studio-runtime::StudioRuntime`，
Flutter/FRB 与独立 HTTP server 是两个 transport，不拥有另一套业务状态。

## 运行路径

```text
Flutter → pl-studio-bridge ─┐
                           ├→ StudioRuntime → StudioThreadFactory → StudioThreadAssembler
HTTP → pl-studio-server ───┘                                         ↓
                                                         pl-core ThreadHandle
                                                           ↙             ↘
                                                   pl-model 会话      pl-tool 实例
```

上图最后两条表示装配后的调用关系；crate 依赖方向是 model/tool → core。
Thread owner 串行拥有输入、Turn、模型尝试、工具任务、交互、扩展状态与不可变日志。
模型和工具执行期间仍可受理消息、读取日志和撤销待执行权限；这些命令不改写已冻结的请求。
冻结工具集合分别携带通用调用模式及必须独立执行的工具 ID。只有全部可见工具都要求独立执行时使用 Sequential；混合目录允许普通工具并行，适配器明确列举必须单独调用的工具。内核继续拒绝把独立工具与其他调用混合的响应。

## 概念与事实源

- Thread 是通用执行 owner，每个 Thread 独占模型会话、工具注册表和工具实例。
- Turn 是一次输入驱动的有界执行；model attempt 与工具 task 保存独立生命周期。
- 通用上下文包含角色、来源、文本、资源引用、不透明内容及工具调用关联。
- Item、Agent Profile、Mode、workflow 和 Plan 是产品概念，由 Studio 从保存的事实投影。
- Interaction 和 permission 是通用 Pending/Resolved/Cancelled 或许可决定事实；问题和回答正文由上层解释。

运行时事实支持完整替换与按 source 原子 patch；独立生产者只 patch 自己的 source，空内容清除该 source，遗漏 source 保持不变。两者在 owner 串行边界执行，待交付工具调用存在时拒绝修改。

活动 Thread 内存快照是唯一可写执行事实源。`sessions.sqlite` 可选保存通用 journal 与不可变资源；
`studio.sqlite` 保存项目、配置关联和产品目录。两库不共享业务事务。UI snapshot 和 Turn 分页从
同一日志水位生成，历史模型正文使用保存时的上下文，不调用当前工具重建。

## 所有权

`pl-core` 定义通用模型/工具接口、Thread 编排、上下文和存储，不依赖 protocol、model、tool、
trace 或 Studio。`pl-model` 解释 provider 协议、模型目录、路由和缓存；`pl-tool` 实现工具与物理服务；
`pl-trace` 只读观察 core。Studio 保存配置并完成 root、child、冷恢复的同一路径装配。
具体依赖见 [02](./02-crates.md)，通用框架契约见 [27](./27-core-boundaries-and-replay.md)。

## 恢复与关闭

纯重放只应用已保存事实，不创建模型、工具或外部副作用。恢复把遗留运行收束为 Interrupted，
保留待处理输入，清空物理 continuation 和旧 executor 授权；显式激活后装入当前服务实例。
关闭先封闭准入并取消当前代次，收束结果及交互，再关闭模型/工具并保存最终 commit。
失败保留 owner 和重试入口；归档等待整棵 Thread 树关闭成功。

不兼容的旧会话格式由 Studio 在独占启动锁下先备份再协调重建；core 独立打开只报错保留原库。
配置、凭据、工作区与无法确认所有权的资源不参与会话重建。

工具目录异步准备可使用 `register_tools_if_extensions` 携带冻结快照的 `extension_sequence` 条件发布。
Owner 在同一 mailbox 操作内验证扩展水位再替换完整目录；过期候选保持未安装并由移交方异步关闭，
拒绝不修改当前目录或权限。无条件注册仍用于已独占装配；关闭失败同时保留原类型化拒绝原因、cleanup source 与原实例供明确重试。
