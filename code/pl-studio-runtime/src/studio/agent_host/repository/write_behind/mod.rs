//! Thread commit 的 write-behind 批量落库 writer。
//!
//! 内存 snapshot 是唯一权威实例；commit 进入本进程内队列后即可发布，后台 task
//! 按 FIFO 分批在单个 SQLite 事务中应用。瞬时错误永久保留批次并自动退避重试；
//! 修订冲突等不变量错误进入 Blocked，但不会删除任何待落库事实。
//!
//! 职责划分:
//! - `queue`: 批量常量、typed mutation 队列条目、合并策略与批量分组;
//! - `handle`: 队列与后台 task 的共享状态及对外句柄;
//! - `worker`: supervisor 与 writer 主循环（取批、应用、重试、恢复）;
//! - `apply`: 批次的 SQLite 事务应用与错误分类;
//! - `durability`: 耐久修订推进与屏障完成;
//! - `state`: PersistenceState 的计算与发布、屏障失败处理。

mod apply;
mod durability;
mod handle;
mod queue;
mod state;
mod thread_fact;
mod worker;

pub(in crate::studio) use handle::ThreadWriteBehindWriter;
