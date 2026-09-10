# 25 - 通用条目、独立存储与无副作用重放

## 通用合同

`pl_core::storage::SessionEntry` 是独立于产品协议的存储信封，包含 owner/Turn 归属、ID、顺序、
revision、时间、格式与版本。payload 保留原始 UTF-8 字符串，不要求 JSON；空白、大数字、
Unicode 和 NUL 不得被规范化。格式不授予控制权限，保留记录身份由框架分配。

`SessionEntryCommit` 保存有序 Put/Delete，替换和删除保留旧版本。`ReplayState::apply` 先验证
整个批次，再原子推进状态和水位；归属、序号或删除目标无效时保留之前的完整状态。
通用重放在默认纯内存构建可用，不调用业务 decoder、模型、工具或当前配置。

## SQLite 后端

显式启用 `pl-core/sqlite` 后可创建 `persistence::SqliteSessionStore`；默认不链接 ORM 和文件锁。
`register_resource` 接收 `OpaquePayload` 并保存不可变资源；同 ID 同内容幂等，不同内容报冲突。
元数据默认上限 1 MiB，已提交 Thread journal 不套用该限制，而由队列压力约束下一次执行准入。

Thread journal 以框架保留资源 ID 保存已冻结编码，完整业务 payload、模型上下文、调用关联和
状态同 commit。后端仅解码通用外层信封与 journal，不解释工具或产品正文，不按业务类型建表。
`read_thread_journal` 校验 owner、序号键和日志顺序；产品通过日志自行投影历史及交互。

writer 在内存保存受理水位、待保存字节、错误与关闭状态，异步事务批量写入。单资源 flush 等待
该记录水位，全局 flush 捕获调用时尾部。压力、错误与恢复对 Thread 可观察；失败不丢弃排队事实。
初始化失败关闭连接；shutdown 排空、join 并关闭共享 pool 后才释放文件锁。

## 格式与恢复

当前独立会话数据库为 schema 6；schema 5 的旧 actor/业务存储不进入新 Thread 恢复路径。
core 打开不兼容库返回 UnsupportedSchema 并保留字节，不自动重置。
Studio 启动持有独占锁，按可恢复 marker 先备份产品/会话数据库，再归档旧库、重建会话关联并
创建当前库，处理 WAL/SHM 后才发布 runtime。配置、凭据、工作区、附件与未知对象不随之删除。
未来版本、损坏库和无法确认的资源失败保留现场。

## 验证边界

默认 storage 测试验证原文与原子重放；SQLite 集成验证真实文件重开、旧格式保护、损坏/丢尾、
历史替换删除、共享 writer、单记录 flush、压力和关闭文件锁。生产服务恢复与产品投影在 Studio
验证，不为 core 测试引入 model、tool 或 protocol 依赖。具体执行结果以本次验证记录为准。
