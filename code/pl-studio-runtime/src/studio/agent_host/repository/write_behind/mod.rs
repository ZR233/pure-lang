//! Thread commit 的 write-behind 批量落库 writer。
//!
//! 内存 snapshot 是唯一权威实例；commit 进入本进程内队列后即可发布，后台 task
//! 按 FIFO 分批在单个 SQLite 事务中应用。瞬时错误永久保留批次并自动退避重试；
//! 修订冲突等不变量错误进入 Blocked，但不会删除任何待落库事实。
//!
//! 每次受理分配单调 `ticket`，`durable_ticket` 只在批次成功提交后推进；`flush_through(ticket)`
//! 只等待调用时固定的目标水位，不等待整个系统空闲。队列按条目数/字节/年龄上报压力，并对
//! 目录事实做有界合并。
//!
//! 职责划分:
//! - `queue`: 批量常量、typed mutation 队列条目、合并策略与批量分组;
//! - `handle`: 队列与后台 task 的共享状态及对外句柄;
//! - `worker`: supervisor 与 writer 主循环（取批、应用、重试、恢复）;
//! - `apply`: 批次的 SQLite 事务应用与错误分类;
//! - `state`: PersistenceState 的计算与发布。
//!
//! 模型调用/计费事实不属于本 writer 的事实源：它们只在 `storage::calls::CallsStore` 的唯一
//! 逻辑 writer 队列中落库，本 writer 只承载目录事实与 worktree lease。性能投影通过调用库查询
//! 结果刷新，不再经本队列持久化第二份事实。

mod apply;
mod handle;
mod queue;
mod state;
mod worker;

pub(in crate::studio) use handle::ThreadWriteBehindWriter;
