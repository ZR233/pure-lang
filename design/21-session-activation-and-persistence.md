# 21 - 会话激活、唯一热状态与异步持久化

Thread owner 的内存快照是唯一可写执行状态，日志和数据库只保存已提交事实。
Studio 的 root、child 和恢复通过同一个装配入口创建独占模型会话及工具实例。
准备登记先于资源创建，发布晚于装配完成；取消等待不能丢失准备 owner。

## 冷读取与恢复

core storage 纯重放验证归属、连续序号、信封、替换和删除；Thread journal 重放进一步验证
通用执行不变量。两者均不调用模型、工具、业务 reducer 或当前渲染器。
Studio 从同一日志水位投影 Item、Turn、Interaction 和业务面板。未知格式可查看原始载荷；
继续执行必需的格式无法解释时明确失败，不默认为空状态。

启动审计缺失或损坏历史与产品关联。遗留 Running 尝试/任务/Turn 收束为 Interrupted，未交付调用
形成明确中断结果；待处理输入保留。此阶段不创建物理模型或工具。显式激活装入新资源，
旧 continuation、executor 和审批授权不得恢复为有效能力。

## 异步保存

SQLite 仅由 `sqlite` feature 启用。Thread 发布不可变 commit 后非阻塞受理冻结编码；编码、
准入或保存失败仍由 owner 保留材料。writer 最多五秒或 64 条触发，flush 固定调用时水位。
`ColdStore::flush(thread_id, sequence)` 只等待指定不可变记录，不受后续其他 Thread 排队影响。

待保存字节默认 64 MiB/Thread、256 MiB/store 暂停新执行，低于一半恢复；已受理结果、取消、
查询、交互收束和关闭仍能提交。队列阈值不是进程内存硬上限，已完成结果不得因超限丢失。

## 关闭与产品目录

关闭封闭准入并中断当前代次，保存真实在途结果、取消待答交互，等待模型和工具关闭，再保存
最终 commit。保存失败显示 Closing 并保留 owner；Closed 需要最终水位 durable。
共享 SQLite store 在全部 Thread 关闭后排空 writer、join、关闭连接池，再释放数据库文件锁。

Studio 产品目录 writer 与通用 journal writer 独立，不再存在旧 ThreadRepository 写入适配。
`studio.sqlite` 保存项目/目录等产品事实，`sessions.sqlite` 保存不透明 journal 和资源；
两库不以业务表或双写协议复制 Thread 状态。旧格式协调重置见 [25](./25-session-entry-storage.md)。
