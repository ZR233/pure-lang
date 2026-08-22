//! Typed Interaction aggregates and lifecycle state machines.

mod state;

pub use state::*;

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::event::{UserInputAnswer, UserQuestion};
use crate::labeled::LabeledEnum;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionKind {
    UserInput,
    ToolApproval,
    PlanConfirmation,
}

impl InteractionKind {
    pub fn as_str(self) -> &'static str {
        self.label()
    }
}

/// Interaction lifecycle 的只读投影；不作为可写状态保存。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Expired,
}

impl InteractionStatus {
    pub fn as_str(self) -> &'static str {
        self.label()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionScope {
    pub thread_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_path: Option<String>,
}

/// Interaction kind 与其 payload/state 的唯一绑定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum InteractionContent {
    UserInput(UserInputInteraction),
    ToolApproval(ToolApprovalInteraction),
    PlanConfirmation(PlanConfirmationInteraction),
}

/// 一个有 identity、revision 与 typed content 的 Interaction 聚合。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionRequest {
    pub interaction_id: String,
    pub scope: InteractionScope,
    pub revision: u64,
    pub content: InteractionContent,
    pub created_at: i64,
    pub updated_at: i64,
}

impl InteractionRequest {
    pub fn user_input(
        interaction_id: impl Into<String>,
        scope: InteractionScope,
        questions: Vec<UserQuestion>,
        created_at: i64,
    ) -> Self {
        let interaction_id = interaction_id.into();
        Self {
            content: InteractionContent::UserInput(UserInputInteraction::new(
                questions,
                interaction_id.clone(),
            )),
            interaction_id,
            scope,
            revision: 0,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn tool_approval(
        interaction_id: impl Into<String>,
        scope: InteractionScope,
        request: ToolApprovalRequest,
        created_at: i64,
    ) -> Self {
        let interaction_id = interaction_id.into();
        Self {
            content: InteractionContent::ToolApproval(ToolApprovalInteraction::new(
                request,
                interaction_id.clone(),
            )),
            interaction_id,
            scope,
            revision: 0,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn plan_confirmation(
        interaction_id: impl Into<String>,
        scope: InteractionScope,
        plan_id: impl Into<String>,
        content: impl Into<String>,
        created_at: i64,
    ) -> Self {
        let interaction_id = interaction_id.into();
        Self {
            content: InteractionContent::PlanConfirmation(PlanConfirmationInteraction::new(
                plan_id.into(),
                content.into(),
                interaction_id.clone(),
            )),
            interaction_id,
            scope,
            revision: 0,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn kind(&self) -> InteractionKind {
        match &self.content {
            InteractionContent::UserInput(_) => InteractionKind::UserInput,
            InteractionContent::ToolApproval(_) => InteractionKind::ToolApproval,
            InteractionContent::PlanConfirmation(_) => InteractionKind::PlanConfirmation,
        }
    }

    pub fn status(&self) -> InteractionStatus {
        match &self.content {
            InteractionContent::UserInput(value) => value.status(),
            InteractionContent::ToolApproval(value) => value.status(),
            InteractionContent::PlanConfirmation(value) => value.status(),
        }
    }

    pub fn resolution(&self) -> Option<InteractionResolution> {
        match &self.content {
            InteractionContent::UserInput(value) => value.resolution(),
            InteractionContent::ToolApproval(value) => value.resolution(),
            InteractionContent::PlanConfirmation(value) => value.resolution(),
        }
    }

    /// 比较不受 lifecycle 变化影响的请求 identity 与 payload。
    pub fn same_request(&self, other: &Self) -> bool {
        self.interaction_id == other.interaction_id
            && self.scope == other.scope
            && self.created_at == other.created_at
            && match (&self.content, &other.content) {
                (InteractionContent::UserInput(left), InteractionContent::UserInput(right)) => {
                    left.questions() == right.questions()
                }
                (
                    InteractionContent::ToolApproval(left),
                    InteractionContent::ToolApproval(right),
                ) => left.request() == right.request(),
                (
                    InteractionContent::PlanConfirmation(left),
                    InteractionContent::PlanConfirmation(right),
                ) => left.plan_id() == right.plan_id() && left.content() == right.content(),
                (
                    InteractionContent::UserInput(_),
                    InteractionContent::ToolApproval(_) | InteractionContent::PlanConfirmation(_),
                )
                | (
                    InteractionContent::ToolApproval(_),
                    InteractionContent::UserInput(_) | InteractionContent::PlanConfirmation(_),
                )
                | (
                    InteractionContent::PlanConfirmation(_),
                    InteractionContent::UserInput(_) | InteractionContent::ToolApproval(_),
                ) => false,
            }
    }

    pub fn decide(
        &self,
        command: InteractionCommand,
    ) -> Result<InteractionTransitionDecision, InteractionTransitionError> {
        if command.interaction_id() != self.interaction_id {
            return Err(InteractionTransitionError::wrong_interaction(
                self,
                command.name(),
                command.interaction_id(),
            ));
        }
        if command.expected_revision() != self.revision {
            return Err(InteractionTransitionError::stale_revision(
                self,
                command.name(),
                command.expected_revision(),
            ));
        }
        let current = self.status();
        let next_content = match (&self.content, command) {
            (
                InteractionContent::UserInput(value),
                InteractionCommand::ResolveUserInput(command),
            ) => InteractionContent::UserInput(
                value
                    .decide(UserInputCommand::Resolve(command))
                    .map_err(|error| error.at(self))?,
            ),
            (
                InteractionContent::ToolApproval(value),
                InteractionCommand::ResolveToolApproval(command),
            ) => InteractionContent::ToolApproval(
                value
                    .decide(ToolApprovalCommand::Resolve(command))
                    .map_err(|error| error.at(self))?,
            ),
            (
                InteractionContent::PlanConfirmation(value),
                InteractionCommand::ResolvePlanConfirmation(command),
            ) => InteractionContent::PlanConfirmation(
                value
                    .decide(PlanConfirmationCommand::Resolve(command))
                    .map_err(|error| error.at(self))?,
            ),
            (InteractionContent::UserInput(value), InteractionCommand::Cancel(command)) => {
                InteractionContent::UserInput(
                    value
                        .decide(UserInputCommand::Cancel(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (InteractionContent::ToolApproval(value), InteractionCommand::Cancel(command)) => {
                InteractionContent::ToolApproval(
                    value
                        .decide(ToolApprovalCommand::Cancel(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (InteractionContent::PlanConfirmation(value), InteractionCommand::Cancel(command)) => {
                InteractionContent::PlanConfirmation(
                    value
                        .decide(PlanConfirmationCommand::Cancel(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (InteractionContent::UserInput(value), InteractionCommand::Expire(command)) => {
                InteractionContent::UserInput(
                    value
                        .decide(UserInputCommand::Expire(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (InteractionContent::ToolApproval(value), InteractionCommand::Expire(command)) => {
                InteractionContent::ToolApproval(
                    value
                        .decide(ToolApprovalCommand::Expire(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (InteractionContent::PlanConfirmation(value), InteractionCommand::Expire(command)) => {
                InteractionContent::PlanConfirmation(
                    value
                        .decide(PlanConfirmationCommand::Expire(command))
                        .map_err(|error| error.at(self))?,
                )
            }
            (
                InteractionContent::UserInput(value),
                InteractionCommand::ReopenRecovered(command),
            ) => InteractionContent::UserInput(
                value
                    .decide(UserInputCommand::ReopenRecovered(command))
                    .map_err(|error| error.at(self))?,
            ),
            (_, command) => {
                return Err(
                    InteractionTransitionError::new(self.kind(), current, command.name()).at(self),
                );
            }
        };
        Ok(InteractionTransitionDecision {
            changed: next_content != self.content,
            next_content,
        })
    }

    pub fn apply(&mut self, decision: InteractionTransitionDecision, updated_at: i64) {
        if decision.changed {
            self.content = decision.next_content;
            self.revision = self.revision.saturating_add(1);
            self.updated_at = updated_at;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionCommand {
    ResolveUserInput(ResolveUserInput),
    ResolveToolApproval(ResolveToolApproval),
    ResolvePlanConfirmation(ResolvePlanConfirmation),
    Cancel(CancelInteraction),
    Expire(ExpireInteraction),
    ReopenRecovered(ReopenRecoveredInteraction),
}

impl InteractionCommand {
    fn name(&self) -> &'static str {
        match self {
            Self::ResolveUserInput(_) => "resolveUserInput",
            Self::ResolveToolApproval(_) => "resolveToolApproval",
            Self::ResolvePlanConfirmation(_) => "resolvePlanConfirmation",
            Self::Cancel(_) => "cancel",
            Self::Expire(_) => "expire",
            Self::ReopenRecovered(_) => "reopenRecovered",
        }
    }

    fn interaction_id(&self) -> &str {
        match self {
            Self::ResolveUserInput(command) => &command.interaction_id,
            Self::ResolveToolApproval(command) => &command.interaction_id,
            Self::ResolvePlanConfirmation(command) => &command.interaction_id,
            Self::Cancel(command) => &command.interaction_id,
            Self::Expire(command) => &command.interaction_id,
            Self::ReopenRecovered(command) => &command.interaction_id,
        }
    }

    fn expected_revision(&self) -> u64 {
        match self {
            Self::ResolveUserInput(command) => command.expected_revision,
            Self::ResolveToolApproval(command) => command.expected_revision,
            Self::ResolvePlanConfirmation(command) => command.expected_revision,
            Self::Cancel(command) => command.expected_revision,
            Self::Expire(command) => command.expected_revision,
            Self::ReopenRecovered(command) => command.expected_revision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionTransitionDecision {
    pub next_content: InteractionContent,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionTransitionError {
    interaction_id: String,
    kind: InteractionKind,
    current: InteractionStatus,
    command: &'static str,
    violation: InteractionTransitionViolation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InteractionTransitionViolation {
    IllegalTransition,
    WrongInteraction { actual: String },
    StaleRevision { expected: u64, actual: u64 },
}

impl InteractionTransitionError {
    fn at(mut self, interaction: &InteractionRequest) -> Self {
        self.interaction_id = interaction.interaction_id.clone();
        self
    }

    pub(crate) fn new(
        kind: InteractionKind,
        current: InteractionStatus,
        command: &'static str,
    ) -> Self {
        Self {
            interaction_id: String::new(),
            kind,
            current,
            command,
            violation: InteractionTransitionViolation::IllegalTransition,
        }
    }

    fn wrong_interaction(
        interaction: &InteractionRequest,
        command: &'static str,
        actual: &str,
    ) -> Self {
        Self {
            interaction_id: interaction.interaction_id.clone(),
            kind: interaction.kind(),
            current: interaction.status(),
            command,
            violation: InteractionTransitionViolation::WrongInteraction {
                actual: actual.to_owned(),
            },
        }
    }

    fn stale_revision(
        interaction: &InteractionRequest,
        command: &'static str,
        actual: u64,
    ) -> Self {
        Self {
            interaction_id: interaction.interaction_id.clone(),
            kind: interaction.kind(),
            current: interaction.status(),
            command,
            violation: InteractionTransitionViolation::StaleRevision {
                expected: interaction.revision,
                actual,
            },
        }
    }
}

impl fmt::Display for InteractionTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.violation {
            InteractionTransitionViolation::IllegalTransition => write!(
                formatter,
                "interaction {}/{:?}/{:?} does not accept command {}",
                self.interaction_id, self.kind, self.current, self.command
            ),
            InteractionTransitionViolation::WrongInteraction { actual } => write!(
                formatter,
                "interaction {} rejected command {} for different interaction {}",
                self.interaction_id, self.command, actual
            ),
            InteractionTransitionViolation::StaleRevision { expected, actual } => write!(
                formatter,
                "interaction {} rejected command {} at revision {}; current revision is {}",
                self.interaction_id, self.command, actual, expected
            ),
        }
    }
}

impl std::error::Error for InteractionTransitionError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum InteractionResolution {
    UserInput(UserInputResolution),
    ToolApproval(ToolApprovalResolutionPayload),
    PlanConfirmation(PlanConfirmationResolutionPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInputResolution {
    pub answers: HashMap<String, UserInputAnswer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResolutionPayload {
    pub decision: ToolApprovalResolution,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanConfirmationResolutionPayload {
    pub decision: PlanConfirmationResolution,
    pub content: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ToolApprovalResolution {
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlanConfirmationResolution {
    ImplementFreshContext,
    ContinuePlanning,
    Dismiss,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InteractionChangedEvent {
    pub interaction: InteractionRequest,
}

crate::impl_labeled_enum!(InteractionKind, "InteractionKind", [
    InteractionKind::UserInput => "userInput",
    InteractionKind::ToolApproval => "toolApproval",
    InteractionKind::PlanConfirmation => "planConfirmation",
]);

crate::impl_labeled_enum!(InteractionStatus, "InteractionStatus", [
    InteractionStatus::Pending => "pending",
    InteractionStatus::Resolved => "resolved",
    InteractionStatus::Cancelled => "cancelled",
    InteractionStatus::Expired => "expired",
]);

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn scope() -> InteractionScope {
        InteractionScope {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: None,
            tool_id: None,
            agent_path: Some("/root".to_string()),
        }
    }

    fn user_input() -> InteractionRequest {
        InteractionRequest::user_input(
            "interaction-1",
            scope(),
            vec![UserQuestion {
                id: "question-1".to_string(),
                header: "Choice".to_string(),
                question: "Continue?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            }],
            10,
        )
    }

    fn resolve_user_input(
        interaction_id: &str,
        expected_revision: u64,
        operation_id: &str,
    ) -> InteractionCommand {
        InteractionCommand::ResolveUserInput(ResolveUserInput {
            interaction_id: interaction_id.to_string(),
            expected_revision,
            operation_id: operation_id.to_string(),
            resolved_at: 20,
            answers: HashMap::from([(
                "question-1".to_string(),
                UserInputAnswer {
                    answers: vec!["yes".to_string()],
                },
            )]),
        })
    }

    #[test]
    fn user_input_resolution_is_typed_revisioned_and_round_trips() {
        let mut interaction = user_input();
        let decision = interaction
            .decide(resolve_user_input("interaction-1", 0, "resolve-1"))
            .expect("pending user input should resolve");
        assert!(decision.changed);
        interaction.apply(decision, 20);

        assert_eq!(interaction.revision, 1);
        assert_eq!(interaction.status(), InteractionStatus::Resolved);
        let encoded = serde_json::to_string(&interaction).expect("interaction should serialize");
        let decoded = serde_json::from_str(&encoded).expect("interaction should deserialize");
        assert_eq!(interaction, decoded);

        let repeated = interaction
            .decide(resolve_user_input("interaction-1", 1, "resolve-1"))
            .expect("exact operation retry should be accepted");
        assert!(!repeated.changed);
    }

    #[test]
    fn interaction_rejects_wrong_identity_revision_and_resolution_kind() {
        let interaction = user_input();
        let wrong_identity = interaction
            .decide(resolve_user_input("interaction-2", 0, "resolve-1"))
            .expect_err("another interaction id must be rejected");
        assert!(wrong_identity.to_string().contains("different interaction"));

        let stale = interaction
            .decide(resolve_user_input("interaction-1", 9, "resolve-1"))
            .expect_err("stale revision must be rejected");
        assert!(stale.to_string().contains("current revision is 0"));

        let mismatch = interaction
            .decide(InteractionCommand::ResolveToolApproval(
                ResolveToolApproval {
                    interaction_id: interaction.interaction_id.clone(),
                    expected_revision: interaction.revision,
                    operation_id: "resolve-1".to_string(),
                    resolved_at: 20,
                    decision: ToolApprovalResolution::Approved,
                    reason: None,
                },
            ))
            .expect_err("tool resolution must not resolve user input");
        assert!(mismatch.to_string().contains("resolveToolApproval"));
    }

    #[test]
    fn only_cancelled_user_input_can_be_reopened_for_recovery() {
        let mut interaction = user_input();
        let cancelled = interaction
            .decide(InteractionCommand::Cancel(CancelInteraction {
                interaction_id: interaction.interaction_id.clone(),
                expected_revision: interaction.revision,
                operation_id: "cancel-1".to_string(),
                reason: "restart".to_string(),
                cancelled_at: 15,
            }))
            .expect("pending interaction should cancel");
        interaction.apply(cancelled, 15);
        let reopened = interaction
            .decide(InteractionCommand::ReopenRecovered(
                ReopenRecoveredInteraction {
                    interaction_id: interaction.interaction_id.clone(),
                    expected_revision: interaction.revision,
                    operation_id: "recover-1".to_string(),
                    reopened_at: 16,
                },
            ))
            .expect("cancelled user input should reopen");
        interaction.apply(reopened, 16);
        assert_eq!(interaction.status(), InteractionStatus::Pending);
        assert_eq!(interaction.revision, 2);

        let tool = InteractionRequest::tool_approval(
            "tool-1",
            scope(),
            ToolApprovalRequest {
                name: "shell".to_string(),
                arguments: serde_json::json!({}),
                working_directory: None,
                parent_agent_id: None,
            },
            10,
        );
        assert!(
            tool.decide(InteractionCommand::ReopenRecovered(
                ReopenRecoveredInteraction {
                    interaction_id: tool.interaction_id.clone(),
                    expected_revision: tool.revision,
                    operation_id: "recover-2".to_string(),
                    reopened_at: 16,
                },
            ))
            .is_err()
        );
    }

    #[test]
    fn legacy_flat_interaction_json_is_rejected() {
        let legacy = serde_json::json!({
            "interactionId": "interaction-1",
            "kind": "userInput",
            "status": "pending",
            "scope": scope(),
            "payload": { "questions": [] },
            "createdAt": 10,
            "updatedAt": 10
        });
        assert!(serde_json::from_value::<InteractionRequest>(legacy).is_err());
    }
}
