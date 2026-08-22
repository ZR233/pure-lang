use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct IdleContinuation {
    revision: u64,
    slice_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActiveContinuation {
    revision: u64,
    source_turn_id: String,
    slice_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingStartContinuation {
    revision: u64,
    source_turn_id: String,
    slice_count: u32,
    limit: pl_protocol::BudgetLimitSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttentionContinuation {
    revision: u64,
    source_turn_id: String,
    slice_count: u32,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum ExecutorContinuationState {
    Idle(IdleContinuation),
    Compacting(ActiveContinuation),
    PendingStart(PendingStartContinuation),
    PlannerWakePending(ActiveContinuation),
    NeedsAttention(AttentionContinuation),
}

impl ExecutorContinuationState {
    pub(crate) fn idle(revision: u64, slice_count: u32) -> Self {
        Self::Idle(IdleContinuation {
            revision,
            slice_count,
        })
    }

    pub(crate) fn pending_start(
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
        limit: pl_protocol::BudgetLimitSnapshot,
    ) -> Self {
        Self::PendingStart(PendingStartContinuation {
            revision,
            source_turn_id,
            slice_count,
            limit,
        })
    }

    pub(crate) fn planner_wake_pending(
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
    ) -> Self {
        Self::PlannerWakePending(ActiveContinuation {
            revision,
            source_turn_id,
            slice_count,
        })
    }

    pub(crate) fn needs_attention(
        revision: u64,
        source_turn_id: String,
        slice_count: u32,
        detail: String,
    ) -> Self {
        Self::NeedsAttention(AttentionContinuation {
            revision,
            source_turn_id,
            slice_count,
            detail,
        })
    }

    pub(crate) const fn kind(&self) -> ExecutorContinuationStateKind {
        match self {
            Self::Idle(_) => ExecutorContinuationStateKind::Idle,
            Self::Compacting(_) => ExecutorContinuationStateKind::Compacting,
            Self::PendingStart(_) => ExecutorContinuationStateKind::PendingStart,
            Self::PlannerWakePending(_) => ExecutorContinuationStateKind::PlannerWakePending,
            Self::NeedsAttention(_) => ExecutorContinuationStateKind::NeedsAttention,
        }
    }

    pub(crate) const fn revision(&self) -> u64 {
        match self {
            Self::Idle(value) => value.revision,
            Self::Compacting(value) | Self::PlannerWakePending(value) => value.revision,
            Self::PendingStart(value) => value.revision,
            Self::NeedsAttention(value) => value.revision,
        }
    }

    pub(crate) const fn slice_count(&self) -> u32 {
        match self {
            Self::Idle(value) => value.slice_count,
            Self::Compacting(value) | Self::PlannerWakePending(value) => value.slice_count,
            Self::PendingStart(value) => value.slice_count,
            Self::NeedsAttention(value) => value.slice_count,
        }
    }

    pub(crate) fn source_turn_id(&self) -> Option<&str> {
        match self {
            Self::Idle(_) => None,
            Self::Compacting(value) | Self::PlannerWakePending(value) => {
                Some(&value.source_turn_id)
            }
            Self::PendingStart(value) => Some(&value.source_turn_id),
            Self::NeedsAttention(value) => Some(&value.source_turn_id),
        }
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        match self {
            Self::NeedsAttention(value) => Some(&value.detail),
            Self::Idle(_)
            | Self::Compacting(_)
            | Self::PendingStart(_)
            | Self::PlannerWakePending(_) => None,
        }
    }

    pub(crate) fn budget_limit(&self) -> Option<&pl_protocol::BudgetLimitSnapshot> {
        match self {
            Self::PendingStart(value) => Some(&value.limit),
            Self::Idle(_)
            | Self::Compacting(_)
            | Self::PlannerWakePending(_)
            | Self::NeedsAttention(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExecutorContinuationStateKind {
    Idle,
    Compacting,
    PendingStart,
    PlannerWakePending,
    NeedsAttention,
}
