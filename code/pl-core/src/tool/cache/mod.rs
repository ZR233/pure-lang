//! Turn-scoped tool result caching and reuse.

mod entry;
mod execution;
mod failure;
mod key;
mod read_file;
mod state;

use std::sync::{Arc, Mutex};

use state::TurnToolCache;

/// 工具结果在一次 turn 内的复用策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolCachePolicy {
    #[default]
    Never,
    WithinTurn,
    UntilWorkspaceMutation,
}

/// turn-scoped 只读工具缓存。
#[derive(Debug, Clone, Default)]
pub struct TurnToolCacheHandle {
    inner: Arc<Mutex<TurnToolCache>>,
}

/// 单次 provider response 工具批次共享的缓存 epoch 快照。
#[derive(Debug, Clone)]
pub(crate) struct TurnToolCacheSnapshot {
    cache: TurnToolCacheHandle,
    workspace_epoch: u64,
}

#[cfg(test)]
mod unit_tests;
