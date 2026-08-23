//! TaskRun 六状态生命周期及各状态携带的数据。

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskRunStateKind {
    Planning,
    PendingConfirmation,
    EditingDocuments,
    Working,
    Reviewing,
    Completed,
}

impl TaskRunStateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::PendingConfirmation => "pendingConfirmation",
            Self::EditingDocuments => "editingDocuments",
            Self::Working => "working",
            Self::Reviewing => "reviewing",
            Self::Completed => "completed",
        }
    }

    pub(crate) const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed)
    }

    pub(crate) const fn allows_executor_spawn(self) -> bool {
        matches!(self, Self::Working)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanningState {
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingConfirmationState {
    generation: u64,
    plan_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditingDocumentsState {
    generation: u64,
    plan_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkingState {
    generation: u64,
    document_edit_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntegratedReviewTarget {
    pub(crate) review_round_id: String,
    pub(crate) reviewed_head: String,
    pub(crate) changed_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewingState {
    generation: u64,
    target: IntegratedReviewTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletedState {
    generation: u64,
    outcome: TaskOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TaskFailureKind {
    UnableToProceed,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum TaskReviewGate {
    NotRequiredNoDelivery,
    NotRequiredSingleExecutor { work_unit_id: String },
    IntegratedReview { review_round_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum TaskOutcome {
    Succeeded {
        summary: String,
        completed_at: i64,
        review_gate: TaskReviewGate,
    },
    Failed {
        kind: TaskFailureKind,
        summary: String,
        evidence: String,
        cause: String,
        completed_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub(crate) enum TaskRunState {
    Planning(PlanningState),
    PendingConfirmation(PendingConfirmationState),
    EditingDocuments(EditingDocumentsState),
    Working(WorkingState),
    Reviewing(ReviewingState),
    Completed(CompletedState),
}

impl TaskRunState {
    pub(crate) const fn new() -> Self {
        Self::Planning(PlanningState { generation: 0 })
    }

    pub(crate) const fn kind(&self) -> TaskRunStateKind {
        match self {
            Self::Planning(_) => TaskRunStateKind::Planning,
            Self::PendingConfirmation(_) => TaskRunStateKind::PendingConfirmation,
            Self::EditingDocuments(_) => TaskRunStateKind::EditingDocuments,
            Self::Working(_) => TaskRunStateKind::Working,
            Self::Reviewing(_) => TaskRunStateKind::Reviewing,
            Self::Completed(_) => TaskRunStateKind::Completed,
        }
    }

    pub(crate) const fn generation(&self) -> u64 {
        match self {
            Self::Planning(state) => state.generation,
            Self::PendingConfirmation(state) => state.generation,
            Self::EditingDocuments(state) => state.generation,
            Self::Working(state) => state.generation,
            Self::Reviewing(state) => state.generation,
            Self::Completed(state) => state.generation,
        }
    }

    pub(crate) const fn outcome(&self) -> Option<&TaskOutcome> {
        match self {
            Self::Completed(state) => Some(&state.outcome),
            Self::Planning(_)
            | Self::PendingConfirmation(_)
            | Self::EditingDocuments(_)
            | Self::Working(_)
            | Self::Reviewing(_) => None,
        }
    }

    pub(crate) const fn review_target(&self) -> Option<&IntegratedReviewTarget> {
        match self {
            Self::Reviewing(state) => Some(&state.target),
            _ => None,
        }
    }

    pub(crate) const fn plan_revision(&self) -> Option<u64> {
        match self {
            Self::PendingConfirmation(state) => Some(state.plan_revision),
            Self::EditingDocuments(state) => Some(state.plan_revision),
            _ => None,
        }
    }

    pub(crate) fn document_edit_summary(&self) -> Option<&str> {
        match self {
            Self::Working(state) => Some(&state.document_edit_summary),
            _ => None,
        }
    }

    pub(crate) fn advance_generation(self) -> Result<Self> {
        let generation = self
            .generation()
            .checked_add(1)
            .context("task generation overflow")?;
        Ok(match self {
            Self::Planning(_) => Self::Planning(PlanningState { generation }),
            Self::PendingConfirmation(state) => {
                Self::PendingConfirmation(PendingConfirmationState {
                    generation,
                    plan_revision: state.plan_revision,
                })
            }
            Self::EditingDocuments(state) => Self::EditingDocuments(EditingDocumentsState {
                generation,
                plan_revision: state.plan_revision,
            }),
            Self::Working(state) => Self::Working(WorkingState {
                generation,
                document_edit_summary: state.document_edit_summary,
            }),
            Self::Reviewing(state) => Self::Reviewing(ReviewingState {
                generation,
                target: state.target,
            }),
            Self::Completed(_) => bail!("completed task generation cannot advance"),
        })
    }

    pub(crate) fn submit_plan(self, plan_revision: u64) -> Result<Self> {
        match self {
            Self::Planning(state) => Ok(Self::PendingConfirmation(PendingConfirmationState {
                generation: state.generation,
                plan_revision,
            })),
            state => bail!("{} cannot submit a plan", state.kind().as_str()),
        }
    }

    pub(crate) fn confirm_plan(self, plan_revision: u64) -> Result<Self> {
        match self {
            Self::PendingConfirmation(state) if state.plan_revision == plan_revision => {
                Ok(Self::EditingDocuments(EditingDocumentsState {
                    generation: state.generation,
                    plan_revision,
                }))
            }
            Self::PendingConfirmation(_) => bail!("plan confirmation targets a stale plan"),
            state => bail!("{} cannot confirm a plan", state.kind().as_str()),
        }
    }

    pub(crate) fn request_plan_revision(self) -> Result<Self> {
        match self {
            Self::PendingConfirmation(state) => Ok(Self::Planning(PlanningState {
                generation: state.generation,
            })),
            state => bail!("{} cannot request a plan revision", state.kind().as_str()),
        }
    }

    pub(crate) fn finish_document_editing(self, summary: String) -> Result<Self> {
        let summary = summary.trim().to_string();
        if summary.is_empty() {
            bail!("document edit summary must not be empty");
        }
        match self {
            Self::EditingDocuments(state) => Ok(Self::Working(WorkingState {
                generation: state.generation,
                document_edit_summary: summary,
            })),
            state => bail!("{} cannot finish document editing", state.kind().as_str()),
        }
    }

    pub(crate) fn begin_review(self, target: IntegratedReviewTarget) -> Result<Self> {
        match self {
            Self::Working(state) => Ok(Self::Reviewing(ReviewingState {
                generation: state.generation,
                target,
            })),
            Self::Reviewing(state)
                if state.target.reviewed_head == target.reviewed_head
                    && state.target.changed_files == target.changed_files =>
            {
                Ok(Self::Reviewing(ReviewingState {
                    generation: state.generation,
                    target,
                }))
            }
            Self::Reviewing(_) => bail!("integrated review continuation changed frozen target"),
            state => bail!("{} cannot begin integrated review", state.kind().as_str()),
        }
    }

    pub(crate) fn return_to_working(self, summary: String) -> Result<Self> {
        match self {
            Self::Reviewing(state) => Ok(Self::Working(WorkingState {
                generation: state.generation,
                document_edit_summary: summary,
            })),
            state => bail!("{} cannot return to working", state.kind().as_str()),
        }
    }

    pub(crate) fn complete(self, outcome: TaskOutcome) -> Result<Self> {
        if self.kind().is_terminal() {
            bail!("completed task is immutable");
        }
        Ok(Self::Completed(CompletedState {
            generation: self.generation(),
            outcome,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_lifecycle_uses_only_the_six_canonical_states() {
        let state = TaskRunState::new();
        assert_eq!(state.kind(), TaskRunStateKind::Planning);
        let state = state.submit_plan(3).unwrap();
        assert_eq!(state.kind(), TaskRunStateKind::PendingConfirmation);
        let state = state.confirm_plan(3).unwrap();
        assert_eq!(state.kind(), TaskRunStateKind::EditingDocuments);
        let state = state
            .finish_document_editing("document contract committed".to_string())
            .unwrap();
        assert_eq!(state.kind(), TaskRunStateKind::Working);
        let state = state
            .begin_review(IntegratedReviewTarget {
                review_round_id: "review-1".to_string(),
                reviewed_head: "head-1".to_string(),
                changed_files: vec!["src/lib.rs".to_string()],
            })
            .unwrap();
        assert_eq!(state.kind(), TaskRunStateKind::Reviewing);
        let state = state
            .complete(TaskOutcome::Succeeded {
                summary: "delivered".to_string(),
                completed_at: 1,
                review_gate: TaskReviewGate::IntegratedReview {
                    review_round_id: "review-1".to_string(),
                },
            })
            .unwrap();
        assert_eq!(state.kind(), TaskRunStateKind::Completed);
        assert!(state.advance_generation().is_err());
    }

    #[test]
    fn stop_generation_preserves_the_business_state_and_frozen_review_target() {
        let target = IntegratedReviewTarget {
            review_round_id: "review-1".to_string(),
            reviewed_head: "head-1".to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
        };
        let state = TaskRunState::new()
            .submit_plan(1)
            .unwrap()
            .confirm_plan(1)
            .unwrap()
            .finish_document_editing("documents ready".to_string())
            .unwrap()
            .begin_review(target.clone())
            .unwrap();

        let stopped = state.advance_generation().unwrap();

        assert_eq!(stopped.kind(), TaskRunStateKind::Reviewing);
        assert_eq!(stopped.generation(), 1);
        assert_eq!(stopped.review_target(), Some(&target));
    }

    #[test]
    fn replacement_integrated_review_must_keep_the_frozen_target() {
        let state = TaskRunState::new()
            .submit_plan(1)
            .unwrap()
            .confirm_plan(1)
            .unwrap()
            .finish_document_editing("documents ready".to_string())
            .unwrap()
            .begin_review(IntegratedReviewTarget {
                review_round_id: "review-1".to_string(),
                reviewed_head: "head-1".to_string(),
                changed_files: vec!["src/lib.rs".to_string()],
            })
            .unwrap();
        let replacement = state.clone().begin_review(IntegratedReviewTarget {
            review_round_id: "review-2".to_string(),
            reviewed_head: "head-1".to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
        });
        assert!(replacement.is_ok());
        assert!(
            state
                .begin_review(IntegratedReviewTarget {
                    review_round_id: "review-2".to_string(),
                    reviewed_head: "head-2".to_string(),
                    changed_files: vec!["src/lib.rs".to_string()],
                })
                .is_err()
        );
    }
}
