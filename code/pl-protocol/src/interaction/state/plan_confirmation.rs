use serde::{Deserialize, Serialize};

use super::{
    CancelInteraction, CancelledInteractionState, ExpireInteraction, ExpiredInteractionState,
    PendingInteractionState,
};
use crate::interaction::{
    InteractionResolution, InteractionStatus, InteractionTransitionError,
    PlanConfirmationResolution, PlanConfirmationResolutionPayload,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanConfirmationInteraction {
    plan_id: String,
    content: String,
    state: PlanConfirmationState,
}
impl PlanConfirmationInteraction {
    pub fn new(plan_id: String, content: String, operation_id: String) -> Self {
        Self {
            plan_id,
            content,
            state: PlanConfirmationState::Pending(PendingInteractionState::new(operation_id)),
        }
    }
    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }
    pub fn content(&self) -> &str {
        &self.content
    }
    pub fn state(&self) -> &PlanConfirmationState {
        &self.state
    }
    pub fn status(&self) -> InteractionStatus {
        self.state.status()
    }
    pub fn resolution(&self) -> Option<InteractionResolution> {
        match &self.state {
            PlanConfirmationState::Resolved(value) => Some(
                InteractionResolution::PlanConfirmation(PlanConfirmationResolutionPayload {
                    decision: value.decision,
                    content: value.content.clone(),
                    reason: value.reason.clone(),
                }),
            ),
            PlanConfirmationState::Pending(_)
            | PlanConfirmationState::Cancelled(_)
            | PlanConfirmationState::Expired(_) => None,
        }
    }
    pub(crate) fn decide(
        &self,
        command: PlanConfirmationCommand,
    ) -> Result<Self, InteractionTransitionError> {
        Ok(Self {
            plan_id: self.plan_id.clone(),
            content: self.content.clone(),
            state: self.state.decide(command)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum PlanConfirmationState {
    Pending(PendingInteractionState),
    Resolved(ResolvedPlanConfirmationState),
    Cancelled(CancelledInteractionState),
    Expired(ExpiredInteractionState),
}
impl PlanConfirmationState {
    fn status(&self) -> InteractionStatus {
        match self {
            Self::Pending(_) => InteractionStatus::Pending,
            Self::Resolved(_) => InteractionStatus::Resolved,
            Self::Cancelled(_) => InteractionStatus::Cancelled,
            Self::Expired(_) => InteractionStatus::Expired,
        }
    }
    fn decide(&self, command: PlanConfirmationCommand) -> Result<Self, InteractionTransitionError> {
        match (self, command) {
            (Self::Pending(_), PlanConfirmationCommand::Resolve(value)) => {
                Ok(Self::Resolved(ResolvedPlanConfirmationState::new(value)))
            }
            (Self::Pending(_), PlanConfirmationCommand::Cancel(value)) => {
                Ok(Self::Cancelled(CancelledInteractionState::new(
                    value.operation_id,
                    value.cancelled_at,
                    value.reason,
                )))
            }
            (Self::Pending(_), PlanConfirmationCommand::Expire(value)) => Ok(Self::Expired(
                ExpiredInteractionState::new(value.operation_id, value.expired_at),
            )),
            (Self::Resolved(current), PlanConfirmationCommand::Resolve(value))
                if current.matches(&value) =>
            {
                Ok(self.clone())
            }
            (_, command) => Err(InteractionTransitionError::new(
                crate::InteractionKind::PlanConfirmation,
                self.status(),
                command.name(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPlanConfirmationState {
    operation_id: String,
    resolved_at: i64,
    decision: PlanConfirmationResolution,
    content: Option<String>,
    reason: Option<String>,
}
impl ResolvedPlanConfirmationState {
    fn new(value: ResolvePlanConfirmation) -> Self {
        Self {
            operation_id: value.operation_id,
            resolved_at: value.resolved_at,
            decision: value.decision,
            content: value.content,
            reason: value.reason,
        }
    }
    fn matches(&self, value: &ResolvePlanConfirmation) -> bool {
        self.operation_id == value.operation_id
            && self.resolved_at == value.resolved_at
            && self.decision == value.decision
            && self.content == value.content
            && self.reason == value.reason
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn resolved_at(&self) -> i64 {
        self.resolved_at
    }
    pub fn decision(&self) -> PlanConfirmationResolution {
        self.decision
    }
    pub fn content(&self) -> Option<&str> {
        self.content.as_deref()
    }
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvePlanConfirmation {
    pub interaction_id: String,
    pub expected_revision: u64,
    pub operation_id: String,
    pub resolved_at: i64,
    pub decision: PlanConfirmationResolution,
    pub content: Option<String>,
    pub reason: Option<String>,
}
pub(crate) enum PlanConfirmationCommand {
    Resolve(ResolvePlanConfirmation),
    Cancel(CancelInteraction),
    Expire(ExpireInteraction),
}
impl PlanConfirmationCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::Resolve(_) => "resolvePlanConfirmation",
            Self::Cancel(_) => "cancel",
            Self::Expire(_) => "expire",
        }
    }
}
