//! ReviewRound 聚合及其命令驱动的唯一生命周期状态。

mod state;

use anyhow::{Context, Result};
use serde::Serialize;

use super::{ReviewDesignReference, ReviewFileCoverage, ReviewFinding, ReviewScope, ReviewVerdict};

#[cfg(test)]
pub(crate) use state::ChangesRequiredReview;
pub(crate) use state::{
    ReviewRoundCommand, ReviewRoundState, ReviewRoundStateKind, ReviewRoundTransitionDecision,
    ReviewRoundTransitionError,
};

pub(crate) fn decode_review_round_state(value: &str) -> Result<ReviewRoundState> {
    serde_json::from_str(value).context("invalid stored ReviewRound state JSON")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewRoundRecord {
    pub(crate) id: String,
    pub(crate) task_run_id: String,
    pub(crate) round: u32,
    pub(crate) scope: ReviewScope,
    pub(crate) work_unit_id: Option<String>,
    pub(crate) completion_id: Option<String>,
    pub(crate) completion_revision: Option<u32>,
    pub(crate) reviewed_head: String,
    pub(crate) requested_by_call_id: String,
    pub(crate) state: ReviewRoundState,
    pub(crate) design_references: Vec<ReviewDesignReference>,
    pub(crate) findings: Vec<ReviewFinding>,
    #[serde(skip_serializing)]
    pub(crate) file_reviews: Option<ReviewFileCoverage>,
    pub(crate) revision: u64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl ReviewRoundRecord {
    pub(crate) const fn kind(&self) -> ReviewRoundStateKind {
        self.state.kind()
    }

    pub(crate) const fn verdict(&self) -> ReviewVerdict {
        self.state.verdict()
    }

    pub(crate) fn reviewer_thread_id(&self) -> Option<&str> {
        self.state.reviewer_thread_id()
    }

    pub(crate) fn reviewer_error(&self) -> Option<&str> {
        self.state.error()
    }

    pub(crate) fn summary(&self) -> Option<&str> {
        self.state.summary()
    }

    pub(crate) fn decide(
        &self,
        expected_revision: u64,
        command: ReviewRoundCommand,
    ) -> std::result::Result<ReviewRoundTransitionDecision, ReviewRoundTransitionError> {
        if expected_revision != self.revision {
            return Err(ReviewRoundTransitionError::StaleRevision {
                review_round_id: self.id.clone(),
                expected: expected_revision,
                actual: self.revision,
                command: Box::new(command),
            });
        }
        self.state.decide(&self.id, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_rejects_stale_revision_before_state_transition() {
        let record = ReviewRoundRecord {
            id: "review-1".to_string(),
            task_run_id: "task-1".to_string(),
            round: 1,
            scope: ReviewScope::Integrated,
            work_unit_id: None,
            completion_id: None,
            completion_revision: None,
            reviewed_head: "head".to_string(),
            requested_by_call_id: "call-1".to_string(),
            state: ReviewRoundState::pending_dispatch(),
            design_references: Vec::new(),
            findings: Vec::new(),
            file_reviews: None,
            revision: 4,
            created_at: 1,
            updated_at: 1,
        };
        let error = record
            .decide(
                3,
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: "agent-1".to_string(),
                },
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ReviewRoundTransitionError::StaleRevision {
                expected: 3,
                actual: 4,
                ..
            }
        ));
    }
}
