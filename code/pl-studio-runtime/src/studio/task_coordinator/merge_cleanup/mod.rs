//! Accepted merge cleanup 的命令驱动状态机。

mod state;

pub(crate) use state::{
    MergeCleanupCommand, MergeCleanupResult, MergeCleanupState, MergeCleanupTransitionDecision,
    MergeCleanupTransitionError,
};
