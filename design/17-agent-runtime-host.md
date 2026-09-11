# 17 - Thread Runtime 与 Agent 宿主

## 统一装配

StudioThreadFactory 解析配置、Profile、Mode、项目和物理工作区绑定，StudioThreadAssembler
登记准备 owner 并创建 core Thread。root、child、冷恢复走同一装配入口。
准备中的 owner 不对外发布；失败或等待者取消不丢失释放责任。关闭先封闭装配，等待准备收束，
再按子到父顺序关闭，失败保留重试入口。

每 Thread 独占模型会话与工具实例；同批声明和 executor 由 core 冻结。配置、Profile 与工作区
语义由 Studio 解释，不进入 core 类型；服务可按隔离身份共享租约。模型切换由 owner 串行执行，
关闭旧会话后清空旧 continuation 与重试计划，再创建新会话，失败明确保持模型不可用。

## 输入与协作

输入以稳定 ID 幂等受理；Start/Steer/Queue 由 core 原子选择。受理不伪造 Turn ID，消费与
模型请求准入同 commit 发生。准备失败保留输入，驱动暂停不自动产生新的收费尝试。
root 与 child 使用同一 Thread 机制；Profile、workspace assignment、父子目录、spawn/close
及消息路由由 Studio 协调。工具层只调用宿主端口，不拥有第二套子代理运行时。

Profile 的 directory/worktree 边界由物理工具后端执行；shell/Git/MCP 的能力需单独授权。
冷恢复不重新创建 worktree，不重演已保存的副作用；物理资源身份冲突保留现场并报告错误。

## 交互与业务状态

Studio 从通用 interaction/permission snapshot 投影产品问题，并按带 Thread 归属的 opaque ID
路由回答。Plan 和 workflow 由 Studio 解释，更新使用扩展 CAS；回答、扩展、实际上下文与
continuation 输入在同一 core commit 中提交。重复回答不重复入队或自动驱动。

Mode 切换使用 idle reconfigure：核对水位并拒绝活动 Turn/任务/输入/交互，原子替换业务扩展、
上下文及工具目录；旧历史正文不按当前 Mode 重新渲染。

## 生命周期与观测

启动只审计和收束保存的 journal，不构造模型或工具。缺失已注册历史或产品关联明确失败，
不创建空会话掩盖数据丢失；实际服务在显式激活时装配。

订阅使用同一快照水位和只读日志句柄；执行 owner 关闭后仍可读取最终事实。停止命令在 owner
内部核对预期 Turn，再取消该执行代次并暂停驱动。归档等待整棵 Thread 树关闭和保存成功。
SQLite writer 的失败与未保存事实必须可观察；目录 writer 只保存产品关联，不代理 Thread commit。

执行预算、步骤限制和取消属于通用类型化执行政策；Profile、角色和父子生命周期的决定属于宿主。
不得通过产品 JSON 字段改变 core 控制行为。详见 [27](./27-core-boundaries-and-replay.md)。

## Timeline 分页

Studio 的 Timeline 条目索引从同一 canonical journal 与提交水位派生，可以重建；不改变 core
日志格式，不建立第二份历史数据库。索引随 Thread 驻留释放，稳定 item 身份用于双向游标和
锚点查询。分页返回覆盖边界、双向游标、水位与相关 Turn 元数据，不以整 Turn 限制页大小。
