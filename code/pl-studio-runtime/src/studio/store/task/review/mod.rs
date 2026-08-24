//! ReviewRound 的冷加载、恢复修复与存储模型转换。
//!
//! 职责划分:
//! - `query`: ReviewRound 的只读查询;
//! - `record`: ReviewRound model 与领域 record 的转换及状态 CAS;

mod query;
mod record;

pub(super) use record::{review_round_state, update_review_round_state};
