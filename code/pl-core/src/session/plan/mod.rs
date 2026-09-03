//! AgentSession 内固定 Plan 状态机、持久化投影与工具。

mod machine;
pub mod tools;

use std::sync::Arc;
use std::sync::RwLock;

pub use machine::{
    AgentSessionPlanConfirmationDecision, AgentSessionPlanError, AgentSessionPlanMachine,
    AgentSessionPlanResolveCommand, AgentSessionPlanRestartCommand, AgentSessionPlanSubmitCommand,
    available_transitions, validate_plan,
};

use pl_protocol::{
    AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID, AgentSessionPlanConfirmationPurpose,
    AgentSessionPlanState, ContextSectionId, InteractionContent, InteractionPurpose,
    InteractionRequest, InteractionResolution, MessagePresentation, ModelContextSectionSnapshot,
    PureError,
};

use crate::{canonical_content_hash, canonical_json_hash};

pub const PLAN_CONTEXT_SECTION_ID: &str = "pl.plan";
pub const MAX_PLAN_SESSION_STATE_BYTES: usize = 128 * 1024;

/// 当前 AgentSession 注册的全部 Plan 工具共享的 Arc 内核。
#[derive(Clone)]
pub(crate) struct AgentSessionPlanHandle {
    kernel: Arc<RwLock<AgentSessionPlanKernel>>,
}

#[derive(Debug)]
struct AgentSessionPlanKernel {
    machine: AgentSessionPlanMachine,
}

impl AgentSessionPlanHandle {
    pub(crate) fn new(state: AgentSessionPlanState) -> Result<Self, AgentSessionPlanError> {
        Ok(Self {
            kernel: Arc::new(RwLock::new(AgentSessionPlanKernel {
                machine: AgentSessionPlanMachine::new(state)?,
            })),
        })
    }

    pub(crate) fn state(&self) -> AgentSessionPlanState {
        self.kernel
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .machine
            .state()
            .clone()
    }

    pub(crate) fn mutate<R>(
        &self,
        mutate: impl FnOnce(&mut AgentSessionPlanMachine) -> (R, bool),
    ) -> Result<(R, Option<AgentSessionPlanState>), AgentSessionPlanError> {
        let mut kernel = self
            .kernel
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (result, changed) = mutate(&mut kernel.machine);
        let state = changed.then(|| kernel.machine.state().clone());
        Ok((result, state))
    }

    pub(crate) fn replace(
        &self,
        state: AgentSessionPlanState,
    ) -> Result<(), AgentSessionPlanError> {
        let machine = AgentSessionPlanMachine::new(state)?;
        let mut kernel = self
            .kernel
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kernel.machine = machine;
        Ok(())
    }
}

impl std::fmt::Debug for AgentSessionPlanHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentSessionPlanHandle")
            .finish_non_exhaustive()
    }
}

/// AgentSession Plan 注册选项；不包含或暴露 session-local Arc handle。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSessionPlanOptions {
    submitted_plan_presentation: MessagePresentation,
}

impl Default for AgentSessionPlanOptions {
    fn default() -> Self {
        Self {
            submitted_plan_presentation: MessagePresentation::Hidden,
        }
    }
}

impl AgentSessionPlanOptions {
    /// 预设 Plan confirmation 决议后完整 Plan 用户消息的 GUI presentation。
    pub const fn with_submitted_plan_presentation(
        mut self,
        presentation: MessagePresentation,
    ) -> Self {
        self.submitted_plan_presentation = presentation;
        self
    }

    pub const fn submitted_plan_presentation(&self) -> MessagePresentation {
        self.submitted_plan_presentation
    }
}

/// 验证 Plan 热状态的领域不变量和持久化大小边界。
pub fn validate_session_state_size(state: &AgentSessionPlanState) -> Result<(), PureError> {
    AgentSessionPlanMachine::new(state.clone())
        .map_err(|error| PureError::ConfigError(error.to_string()))?;
    let bytes = serde_json::to_vec(state)?.len();
    if bytes > MAX_PLAN_SESSION_STATE_BYTES {
        return Err(PureError::ConfigError(format!(
            "AgentSession Plan state exceeds {MAX_PLAN_SESSION_STATE_BYTES} bytes"
        )));
    }
    Ok(())
}

/// 从 canonical Plan 状态派生模型可见的精简上下文，不重复完整 Markdown。
pub fn plan_model_context_section(state: &AgentSessionPlanState) -> ModelContextSectionSnapshot {
    let machine = AgentSessionPlanMachine::new(state.clone())
        .expect("validated working state must construct a Plan machine");
    let snapshot = machine.snapshot();
    let document = snapshot.document.as_ref().map(|document| {
        serde_json::json!({
            "version": document.version,
            "contentHash": document.content_hash,
        })
    });
    let content = serde_json::to_string_pretty(&serde_json::json!({
        "revision": snapshot.revision,
        "state": snapshot.state,
        "document": document,
        "pendingInteractionId": snapshot.pending_interaction_id,
        "lastRevisionFeedback": snapshot.last_revision_feedback,
        "allowedTransitions": snapshot.allowed_transitions,
    }))
    .expect("Plan context is serializable");
    ModelContextSectionSnapshot {
        id: ContextSectionId::new(PLAN_CONTEXT_SECTION_ID)
            .expect("built-in Plan context ID is valid"),
        title: "AgentSession Plan State".to_string(),
        content_hash: canonical_content_hash(content.as_bytes()),
        content,
    }
}

/// 把 Plan confirmation pending Interaction 通过同一状态机重放到 actor session。
///
/// 普通 UserInput 返回 `Ok(None)`，不会触碰 Plan。
pub(crate) fn state_for_pending_interaction(
    current: Option<&AgentSessionPlanState>,
    interaction: &InteractionRequest,
) -> Result<Option<AgentSessionPlanState>, String> {
    let Some(purpose) = confirmation_purpose(interaction) else {
        return Ok(None);
    };
    let plan = confirmation_plan(interaction)?;
    let actual_hash = canonical_content_hash(plan.as_bytes());
    if actual_hash != purpose.plan_hash {
        return Err(format!(
            "Plan confirmation content hash mismatch: expected {}, got {actual_hash}",
            purpose.plan_hash
        ));
    }
    let mut machine = AgentSessionPlanMachine::new(current.cloned().unwrap_or_default())
        .map_err(|error| error.to_string())?;
    let response = machine.submit(AgentSessionPlanSubmitCommand {
        expected_revision: purpose.expected_revision,
        plan: plan.to_string(),
        interaction_id: interaction.interaction_id.clone(),
        operation_id: purpose.operation_id.clone(),
        argument_hash: purpose.argument_hash.clone(),
        submitted_at: interaction.created_at,
    });
    if !response.accepted {
        return Err(complete_rejection(&response));
    }
    Ok(Some(machine.into_state()))
}

/// 把 resolved Plan confirmation 通过同一状态机推进为 approved/revisionRequested。
///
/// 普通 Interaction 返回 `Ok(None)`。
pub(crate) fn state_for_resolved_interaction(
    current: Option<&AgentSessionPlanState>,
    interaction: &InteractionRequest,
) -> Result<Option<AgentSessionPlanState>, String> {
    let Some(purpose) = confirmation_purpose(interaction) else {
        return Ok(None);
    };
    let Some(InteractionResolution::UserInput(resolution)) = interaction.resolution() else {
        return Err("Plan confirmation requires a resolved UserInput Interaction".to_string());
    };
    let decision = confirmation_decision(&resolution)?;
    let mut machine = AgentSessionPlanMachine::new(current.cloned().unwrap_or_default())
        .map_err(|error| error.to_string())?;
    let argument_hash =
        canonical_json_hash(&serde_json::to_value(&resolution).map_err(|error| error.to_string())?);
    let response = machine.resolve(AgentSessionPlanResolveCommand {
        expected_revision: purpose.expected_revision.saturating_add(1),
        interaction_id: interaction.interaction_id.clone(),
        operation_id: format!("plan-resolution:{}", interaction.interaction_id),
        argument_hash,
        decision,
        resolved_at: interaction.updated_at,
    });
    if !response.accepted {
        return Err(complete_rejection(&response));
    }
    Ok(Some(machine.into_state()))
}

pub(crate) fn confirmation_decision(
    resolution: &pl_protocol::UserInputResolution,
) -> Result<AgentSessionPlanConfirmationDecision, String> {
    let answer = resolution
        .answers
        .get(AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID)
        .ok_or_else(|| {
            format!(
                "Plan confirmation answer `{AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID}` is missing"
            )
        })?;
    let approved = answer.answers.iter().any(|value| value == "Approve");
    let revision = answer.answers.iter().any(|value| value == "Revise");
    if approved && revision {
        return Err("Plan confirmation cannot select both Approve and Revise".to_string());
    }
    if approved {
        return Ok(AgentSessionPlanConfirmationDecision::Approve);
    }
    let feedback = answer
        .answers
        .iter()
        .filter(|value| value.as_str() != "Revise")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if revision || !feedback.is_empty() {
        return Ok(AgentSessionPlanConfirmationDecision::RequestRevision { feedback });
    }
    Err("Plan confirmation must select Approve, Revise, or provide revision feedback".to_string())
}

pub(crate) fn confirmation_purpose(
    interaction: &InteractionRequest,
) -> Option<&AgentSessionPlanConfirmationPurpose> {
    match &interaction.scope.purpose {
        InteractionPurpose::AgentSessionPlanConfirmation(purpose) => Some(purpose),
        InteractionPurpose::General => None,
    }
}

pub(crate) fn confirmation_plan(interaction: &InteractionRequest) -> Result<&str, String> {
    let InteractionContent::UserInput(user_input) = &interaction.content else {
        return Err("Plan confirmation purpose requires UserInput content".to_string());
    };
    let mut matching = user_input
        .questions()
        .iter()
        .filter(|question| question.id == AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID);
    let question = matching
        .next()
        .ok_or_else(|| "Plan confirmation question is missing".to_string())?;
    if matching.next().is_some() {
        return Err("Plan confirmation contains duplicate confirmation questions".to_string());
    }
    Ok(question.question.as_str())
}

pub(crate) fn complete_rejection(
    response: &pl_protocol::AgentSessionPlanMutationResponse,
) -> String {
    serde_json::to_string_pretty(response).unwrap_or_else(|_| {
        format!(
            "Plan state machine rejected operation with code {:?} at revision {}",
            response.code, response.operation_revision
        )
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{UserInputAnswer, UserInputResolution};
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn confirmation_answers_are_typed_plan_decisions() {
        let resolution = UserInputResolution {
            answers: HashMap::from([(
                AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID.to_string(),
                UserInputAnswer {
                    answers: vec!["Revise".to_string(), "Add rollback.".to_string()],
                },
            )]),
        };
        assert_eq!(
            confirmation_decision(&resolution).unwrap(),
            AgentSessionPlanConfirmationDecision::RequestRevision {
                feedback: "Add rollback.".to_string(),
            }
        );
    }

    #[test]
    fn cloned_handles_share_one_agent_session_kernel() {
        let handle = AgentSessionPlanHandle::new(AgentSessionPlanState::default()).unwrap();
        let peer = handle.clone();
        let (response, changed) = handle
            .mutate(|machine| {
                let response = machine.submit(AgentSessionPlanSubmitCommand {
                    expected_revision: 0,
                    plan: "# Approved baseline\n\nImplement the agreed change.".to_string(),
                    interaction_id: "interaction-1".to_string(),
                    operation_id: "operation-1".to_string(),
                    argument_hash: "hash-1".to_string(),
                    submitted_at: 10,
                });
                let accepted = response.accepted;
                (response, accepted)
            })
            .unwrap();

        assert!(response.accepted);
        assert!(changed.is_some());
        assert_eq!(
            peer.state().state,
            pl_protocol::AgentSessionPlanPhase::AwaitingConfirmation
        );
    }

    #[test]
    fn separately_created_handles_isolate_agent_sessions() {
        let first = AgentSessionPlanHandle::new(AgentSessionPlanState::default()).unwrap();
        let second = AgentSessionPlanHandle::new(AgentSessionPlanState::default()).unwrap();
        first
            .mutate(|machine| {
                let response = machine.submit(AgentSessionPlanSubmitCommand {
                    expected_revision: 0,
                    plan: "# First session only".to_string(),
                    interaction_id: "interaction-1".to_string(),
                    operation_id: "operation-1".to_string(),
                    argument_hash: "hash-1".to_string(),
                    submitted_at: 10,
                });
                let accepted = response.accepted;
                (response, accepted)
            })
            .unwrap();

        assert_eq!(
            first.state().state,
            pl_protocol::AgentSessionPlanPhase::AwaitingConfirmation
        );
        assert_eq!(
            second.state().state,
            pl_protocol::AgentSessionPlanPhase::Drafting
        );
    }
}
