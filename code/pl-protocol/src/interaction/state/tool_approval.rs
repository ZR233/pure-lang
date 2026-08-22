use serde::{Deserialize, Serialize};

use super::{
    CancelInteraction, CancelledInteractionState, ExpireInteraction, ExpiredInteractionState,
    PendingInteractionState,
};
use crate::interaction::{
    InteractionResolution, InteractionStatus, InteractionTransitionError, ToolApprovalResolution,
    ToolApprovalResolutionPayload,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_directory: Option<String>,
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalInteraction {
    request: ToolApprovalRequest,
    state: ToolApprovalState,
}
impl ToolApprovalInteraction {
    pub fn new(request: ToolApprovalRequest, operation_id: String) -> Self {
        Self {
            request,
            state: ToolApprovalState::Pending(PendingInteractionState::new(operation_id)),
        }
    }
    pub fn request(&self) -> &ToolApprovalRequest {
        &self.request
    }
    pub fn state(&self) -> &ToolApprovalState {
        &self.state
    }
    pub fn status(&self) -> InteractionStatus {
        self.state.status()
    }
    pub fn resolution(&self) -> Option<InteractionResolution> {
        match &self.state {
            ToolApprovalState::Resolved(value) => Some(InteractionResolution::ToolApproval(
                ToolApprovalResolutionPayload {
                    decision: value.decision,
                    reason: value.reason.clone(),
                },
            )),
            ToolApprovalState::Pending(_)
            | ToolApprovalState::Cancelled(_)
            | ToolApprovalState::Expired(_) => None,
        }
    }
    pub(crate) fn decide(
        &self,
        command: ToolApprovalCommand,
    ) -> Result<Self, InteractionTransitionError> {
        Ok(Self {
            request: self.request.clone(),
            state: self.state.decide(command)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ToolApprovalState {
    Pending(PendingInteractionState),
    Resolved(ResolvedToolApprovalState),
    Cancelled(CancelledInteractionState),
    Expired(ExpiredInteractionState),
}
impl ToolApprovalState {
    fn status(&self) -> InteractionStatus {
        match self {
            Self::Pending(_) => InteractionStatus::Pending,
            Self::Resolved(_) => InteractionStatus::Resolved,
            Self::Cancelled(_) => InteractionStatus::Cancelled,
            Self::Expired(_) => InteractionStatus::Expired,
        }
    }
    fn decide(&self, command: ToolApprovalCommand) -> Result<Self, InteractionTransitionError> {
        match (self, command) {
            (Self::Pending(_), ToolApprovalCommand::Resolve(value)) => {
                Ok(Self::Resolved(ResolvedToolApprovalState::new(value)))
            }
            (Self::Pending(_), ToolApprovalCommand::Cancel(value)) => {
                Ok(Self::Cancelled(CancelledInteractionState::new(
                    value.operation_id,
                    value.cancelled_at,
                    value.reason,
                )))
            }
            (Self::Pending(_), ToolApprovalCommand::Expire(value)) => Ok(Self::Expired(
                ExpiredInteractionState::new(value.operation_id, value.expired_at),
            )),
            (Self::Resolved(current), ToolApprovalCommand::Resolve(value))
                if current.matches(&value) =>
            {
                Ok(self.clone())
            }
            (_, command) => Err(InteractionTransitionError::new(
                crate::InteractionKind::ToolApproval,
                self.status(),
                command.name(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedToolApprovalState {
    operation_id: String,
    resolved_at: i64,
    decision: ToolApprovalResolution,
    reason: Option<String>,
}
impl ResolvedToolApprovalState {
    fn new(value: ResolveToolApproval) -> Self {
        Self {
            operation_id: value.operation_id,
            resolved_at: value.resolved_at,
            decision: value.decision,
            reason: value.reason,
        }
    }
    fn matches(&self, value: &ResolveToolApproval) -> bool {
        self.operation_id == value.operation_id
            && self.resolved_at == value.resolved_at
            && self.decision == value.decision
            && self.reason == value.reason
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
    pub fn decision(&self) -> ToolApprovalResolution {
        self.decision
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveToolApproval {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub resolved_at: i64,
    pub decision: ToolApprovalResolution,
    pub reason: Option<String>,
}
pub(crate) enum ToolApprovalCommand {
    Resolve(ResolveToolApproval),
    Cancel(CancelInteraction),
    Expire(ExpireInteraction),
}
impl ToolApprovalCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Resolve(_) => "resolveToolApproval",
            Self::Cancel(_) => "cancel",
            Self::Expire(_) => "expire",
        }
    }
}
