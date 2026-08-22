//! ReviewRound 的创建、授权、结算与查询。
//!
//! 职责划分:
//! - `begin`: 发起 delivery / integrated review 并写入新的 ReviewRound;
//! - `authorize`: reviewer Thread 的 spawn 授权、激活与失败回滚;
//! - `settle`: reviewer 结论、拒绝与 Turn 结束后的结算;
//! - `query`: ReviewRound 的只读查询;
//! - `record`: ReviewRound model 与领域 record 的转换及状态 CAS;
//! - `helpers`: 事务收尾与前置校验等共享查询。

mod authorize;
mod begin;
mod helpers;
mod query;
mod record;
mod settle;

pub(super) use record::{review_round_state, update_review_round_state};
