use pl_protocol::{
    AgentSessionPlanAvailableTransition, AgentSessionPlanDocument,
    AgentSessionPlanMutationResponse, AgentSessionPlanOperation, AgentSessionPlanOperationReceipt,
    AgentSessionPlanPhase, AgentSessionPlanResultCode, AgentSessionPlanSnapshot,
    AgentSessionPlanState, AgentSessionPlanTransitionActor, AgentSessionPlanTransitionError,
    AgentSessionPlanTransitionRecord,
};

use crate::canonical_content_hash;

const MAX_PLAN_BYTES: usize = 64 * 1024;
const MAX_FEEDBACK_BYTES: usize = 8 * 1024;
const MAX_REASON_BYTES: usize = 2 * 1024;
const MAX_HISTORY: usize = 32;
const MAX_RECEIPTS: usize = 32;

/// 固定 Plan 状态机接受的完整计划提交命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionPlanSubmitCommand {
    pub expected_revision: u64,
    pub plan: String,
    pub interaction_id: String,
    pub operation_id: String,
    pub argument_hash: String,
    pub submitted_at: i64,
}

/// 固定 Plan 状态机接受的用户确认决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionPlanConfirmationDecision {
    Approve,
    RequestRevision { feedback: String },
}

/// 用户 Interaction resolution 对 Plan 的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionPlanResolveCommand {
    pub expected_revision: u64,
    pub interaction_id: String,
    pub operation_id: String,
    pub argument_hash: String,
    pub decision: AgentSessionPlanConfirmationDecision,
    pub resolved_at: i64,
}

/// 显式开始新 Plan lifecycle 的命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionPlanRestartCommand {
    pub expected_revision: u64,
    pub reason: String,
    pub operation_id: String,
    pub argument_hash: String,
    pub restarted_at: i64,
}

/// 损坏的持久化 Plan 状态。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid AgentSession Plan state: {message}")]
pub struct AgentSessionPlanError {
    message: String,
}

impl AgentSessionPlanError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Plan 领域的唯一固定状态机。
///
/// 工具和宿主只能提交 typed command；合法 source/target、CAS、幂等和错误提示都由
/// 此对象生成，不读取或解释外部状态图。
#[derive(Debug, Clone, Default)]
pub struct AgentSessionPlanMachine {
    state: AgentSessionPlanState,
}

impl AgentSessionPlanMachine {
    /// 从持久化快照恢复并验证状态机。
    ///
    /// # Errors
    ///
    /// 快照违反状态、内容 hash 或大小不变量时返回错误。
    pub fn new(state: AgentSessionPlanState) -> Result<Self, AgentSessionPlanError> {
        validate_state(&state)?;
        Ok(Self { state })
    }

    pub fn state(&self) -> &AgentSessionPlanState {
        &self.state
    }

    pub fn into_state(self) -> AgentSessionPlanState {
        self.state
    }

    pub fn snapshot(&self) -> AgentSessionPlanSnapshot {
        AgentSessionPlanSnapshot {
            revision: self.state.revision,
            state: self.state.state,
            document: self.state.document.clone(),
            pending_interaction_id: self.state.pending_interaction_id.clone(),
            last_revision_feedback: self.state.last_revision_feedback.clone(),
            updated_at: self.state.updated_at,
            allowed_transitions: available_transitions(self.state.state),
        }
    }

    pub fn available_transitions(&self) -> Vec<AgentSessionPlanAvailableTransition> {
        available_transitions(self.state.state)
    }

    pub fn submit(
        &mut self,
        command: AgentSessionPlanSubmitCommand,
    ) -> AgentSessionPlanMutationResponse {
        if let Some(response) = self.idempotent_response(
            AgentSessionPlanOperation::Submit,
            &command.operation_id,
            &command.argument_hash,
        ) {
            return response;
        }
        if command.expected_revision != self.state.revision {
            return self.rejected(
                AgentSessionPlanOperation::Submit,
                AgentSessionPlanResultCode::StaleRevision,
                format!(
                    "plan revision mismatch: expected {}, current revision is {}",
                    command.expected_revision, self.state.revision
                ),
                "expectedRevision must equal the canonical Plan revision",
                vec![
                    "Call plan_current to read the canonical revision and state.".to_string(),
                    "Retry plan_submit with that expectedRevision and the complete Markdown plan."
                        .to_string(),
                ],
            );
        }
        if !matches!(
            self.state.state,
            AgentSessionPlanPhase::Drafting | AgentSessionPlanPhase::RevisionRequested
        ) {
            return self.invalid_state(AgentSessionPlanOperation::Submit);
        }
        if let Err(message) = validate_plan(&command.plan) {
            return self.rejected(
                AgentSessionPlanOperation::Submit,
                AgentSessionPlanResultCode::InvalidState,
                message.clone(),
                message,
                vec![
                    "Provide a complete plan beginning with a level-one Markdown heading."
                        .to_string(),
                ],
            );
        }
        if command.interaction_id.trim().is_empty() {
            return self.rejected(
                AgentSessionPlanOperation::Submit,
                AgentSessionPlanResultCode::InvalidInteraction,
                "plan_submit requires a non-empty canonical Interaction ID",
                "interactionId must be generated from the tool call identity",
                vec!["Retry plan_submit as a new Solo tool call.".to_string()],
            );
        }

        let source = self.state.state;
        let next_revision = self.state.revision.saturating_add(1);
        let document_version = self
            .state
            .document
            .as_ref()
            .map_or(1, |document| document.version.saturating_add(1));
        self.state.document = Some(AgentSessionPlanDocument {
            version: document_version,
            content_hash: canonical_content_hash(command.plan.as_bytes()),
            markdown: command.plan,
        });
        self.state.state = AgentSessionPlanPhase::AwaitingConfirmation;
        self.state.pending_interaction_id = Some(command.interaction_id);
        self.state.last_revision_feedback = None;
        self.record_transition(AgentSessionPlanTransitionRecord {
            revision: next_revision,
            operation: AgentSessionPlanOperation::Submit,
            source_state: source,
            target_state: AgentSessionPlanPhase::AwaitingConfirmation,
            operation_id: command.operation_id.clone(),
            reason: format!("Submitted Plan document version {document_version} for confirmation."),
            transitioned_at: command.submitted_at,
        });
        self.commit_receipt(command.operation_id, command.argument_hash, next_revision);
        self.finish_mutation(next_revision, command.submitted_at);
        self.accepted(
            AgentSessionPlanOperation::Submit,
            AgentSessionPlanResultCode::Submitted,
        )
    }

    pub fn resolve(
        &mut self,
        command: AgentSessionPlanResolveCommand,
    ) -> AgentSessionPlanMutationResponse {
        let (operation, target, reason, feedback) = match command.decision {
            AgentSessionPlanConfirmationDecision::Approve => (
                AgentSessionPlanOperation::Approve,
                AgentSessionPlanPhase::Approved,
                "User approved the current Plan.".to_string(),
                None,
            ),
            AgentSessionPlanConfirmationDecision::RequestRevision { feedback } => {
                if feedback.len() > MAX_FEEDBACK_BYTES {
                    return self.rejected(
                        AgentSessionPlanOperation::RequestRevision,
                        AgentSessionPlanResultCode::InvalidResolution,
                        format!("revision feedback exceeds the {MAX_FEEDBACK_BYTES}-byte limit"),
                        "revision feedback must remain within the documented size limit",
                        vec![
                            "Submit a shorter revision request for the pending Interaction."
                                .to_string(),
                        ],
                    );
                }
                let feedback = feedback.trim().to_string();
                let reason = if feedback.is_empty() {
                    "User requested a revised Plan.".to_string()
                } else {
                    format!("User requested a revised Plan: {feedback}")
                };
                (
                    AgentSessionPlanOperation::RequestRevision,
                    AgentSessionPlanPhase::RevisionRequested,
                    reason,
                    Some(feedback),
                )
            }
        };
        if let Some(response) =
            self.idempotent_response(operation, &command.operation_id, &command.argument_hash)
        {
            return response;
        }
        if command.expected_revision != self.state.revision {
            return self.rejected(
                operation,
                AgentSessionPlanResultCode::StaleRevision,
                format!(
                    "plan revision mismatch while resolving Interaction: expected {}, current revision is {}",
                    command.expected_revision, self.state.revision
                ),
                "the Interaction must resolve the exact awaiting Plan revision",
                vec!["Reload the canonical pending Interaction and Plan state before retrying."
                    .to_string()],
            );
        }
        if self.state.state != AgentSessionPlanPhase::AwaitingConfirmation {
            return self.invalid_state(operation);
        }
        if self.state.pending_interaction_id.as_deref() != Some(&command.interaction_id) {
            return self.rejected(
                operation,
                AgentSessionPlanResultCode::InvalidInteraction,
                format!(
                    "Plan is waiting for Interaction `{}`, not `{}`",
                    self.state
                        .pending_interaction_id
                        .as_deref()
                        .unwrap_or("<missing>"),
                    command.interaction_id
                ),
                "interactionId must equal the Plan pendingInteractionId",
                vec![
                    "Reload the canonical pending Interaction and answer that Interaction."
                        .to_string(),
                ],
            );
        }

        let next_revision = self.state.revision.saturating_add(1);
        self.state.state = target;
        self.state.pending_interaction_id = None;
        self.state.last_revision_feedback = feedback;
        self.record_transition(AgentSessionPlanTransitionRecord {
            revision: next_revision,
            operation,
            source_state: AgentSessionPlanPhase::AwaitingConfirmation,
            target_state: target,
            operation_id: command.operation_id.clone(),
            reason,
            transitioned_at: command.resolved_at,
        });
        self.commit_receipt(command.operation_id, command.argument_hash, next_revision);
        self.finish_mutation(next_revision, command.resolved_at);
        self.accepted(
            operation,
            match operation {
                AgentSessionPlanOperation::Approve => AgentSessionPlanResultCode::Approved,
                AgentSessionPlanOperation::RequestRevision => {
                    AgentSessionPlanResultCode::RevisionRequested
                }
                AgentSessionPlanOperation::Submit | AgentSessionPlanOperation::Restart => {
                    unreachable!("confirmation resolution has a user operation")
                }
            },
        )
    }

    pub fn restart(
        &mut self,
        command: AgentSessionPlanRestartCommand,
    ) -> AgentSessionPlanMutationResponse {
        if let Some(response) = self.idempotent_response(
            AgentSessionPlanOperation::Restart,
            &command.operation_id,
            &command.argument_hash,
        ) {
            return response;
        }
        if command.expected_revision != self.state.revision {
            return self.rejected(
                AgentSessionPlanOperation::Restart,
                AgentSessionPlanResultCode::StaleRevision,
                format!(
                    "plan revision mismatch: expected {}, current revision is {}",
                    command.expected_revision, self.state.revision
                ),
                "expectedRevision must equal the canonical Plan revision",
                vec![
                    "Call plan_current to read the canonical revision and state.".to_string(),
                    "Retry plan_restart with that expectedRevision and a non-empty reason."
                        .to_string(),
                ],
            );
        }
        if !matches!(
            self.state.state,
            AgentSessionPlanPhase::Approved | AgentSessionPlanPhase::RevisionRequested
        ) {
            return self.invalid_state(AgentSessionPlanOperation::Restart);
        }
        let reason = command.reason.trim();
        if reason.is_empty() || reason.len() > MAX_REASON_BYTES {
            return self.rejected(
                AgentSessionPlanOperation::Restart,
                AgentSessionPlanResultCode::InvalidState,
                format!("restart reason must be non-empty and at most {MAX_REASON_BYTES} bytes"),
                "reason must identify why a new Plan lifecycle is required",
                vec!["Retry plan_restart with a concise non-empty reason.".to_string()],
            );
        }

        let source = self.state.state;
        let next_revision = self.state.revision.saturating_add(1);
        self.state.state = AgentSessionPlanPhase::Drafting;
        self.state.document = None;
        self.state.pending_interaction_id = None;
        self.state.last_revision_feedback = None;
        self.record_transition(AgentSessionPlanTransitionRecord {
            revision: next_revision,
            operation: AgentSessionPlanOperation::Restart,
            source_state: source,
            target_state: AgentSessionPlanPhase::Drafting,
            operation_id: command.operation_id.clone(),
            reason: reason.to_string(),
            transitioned_at: command.restarted_at,
        });
        self.commit_receipt(command.operation_id, command.argument_hash, next_revision);
        self.finish_mutation(next_revision, command.restarted_at);
        self.accepted(
            AgentSessionPlanOperation::Restart,
            AgentSessionPlanResultCode::Restarted,
        )
    }

    fn accepted(
        &self,
        operation: AgentSessionPlanOperation,
        code: AgentSessionPlanResultCode,
    ) -> AgentSessionPlanMutationResponse {
        AgentSessionPlanMutationResponse {
            accepted: true,
            code,
            operation,
            operation_revision: self.state.revision,
            snapshot: self.snapshot(),
            error: None,
        }
    }

    fn invalid_state(
        &self,
        operation: AgentSessionPlanOperation,
    ) -> AgentSessionPlanMutationResponse {
        let pending = self
            .state
            .pending_interaction_id
            .as_deref()
            .map(|value| format!(" Pending Interaction: `{value}`."))
            .unwrap_or_default();
        self.rejected(
            operation,
            AgentSessionPlanResultCode::InvalidState,
            format!(
                "Plan operation `{}` is not allowed in state `{}` at revision {}.{pending}",
                operation.as_str(),
                self.state.state.as_str(),
                self.state.revision
            ),
            "the attempted operation is not one of the fixed transitions from the current state",
            recovery_actions(self.state.state),
        )
    }

    fn rejected(
        &self,
        operation: AgentSessionPlanOperation,
        code: AgentSessionPlanResultCode,
        message: impl Into<String>,
        failed_condition: impl Into<String>,
        recovery_actions: Vec<String>,
    ) -> AgentSessionPlanMutationResponse {
        let snapshot = self.snapshot();
        let error = AgentSessionPlanTransitionError {
            code,
            message: message.into(),
            attempted_operation: operation,
            current_state: self.state.state,
            current_revision: self.state.revision,
            allowed_transitions: snapshot.allowed_transitions.clone(),
            failed_condition: failed_condition.into(),
            recovery_actions,
        };
        AgentSessionPlanMutationResponse {
            accepted: false,
            code,
            operation,
            operation_revision: self.state.revision,
            snapshot,
            error: Some(error),
        }
    }

    fn idempotent_response(
        &self,
        operation: AgentSessionPlanOperation,
        operation_id: &str,
        argument_hash: &str,
    ) -> Option<AgentSessionPlanMutationResponse> {
        let receipt = self
            .state
            .operation_receipts
            .iter()
            .find(|receipt| receipt.operation_id == operation_id)?;
        if receipt.argument_hash == argument_hash {
            return Some(AgentSessionPlanMutationResponse {
                accepted: true,
                code: AgentSessionPlanResultCode::AlreadyApplied,
                operation,
                operation_revision: receipt.operation_revision,
                snapshot: self.snapshot(),
                error: None,
            });
        }
        Some(self.rejected(
            operation,
            AgentSessionPlanResultCode::OperationIdentityConflict,
            format!(
                "Plan operation identity `{operation_id}` was already used with different arguments"
            ),
            "one operation identity can bind to exactly one canonical argument hash",
            vec!["Retry as a new tool call so the runtime supplies a new operation identity."
                .to_string()],
        ))
    }

    fn record_transition(&mut self, transition: AgentSessionPlanTransitionRecord) {
        self.state.history_tail.push(transition);
        while self.state.history_tail.len() > MAX_HISTORY {
            let archived = self.state.history_tail.remove(0);
            let encoded = serde_json::to_vec(&archived).unwrap_or_default();
            let mut digest_input = self.state.archived_transition_digest.as_bytes().to_vec();
            digest_input.extend(encoded);
            self.state.archived_transition_digest = canonical_content_hash(&digest_input);
            self.state.archived_transition_count =
                self.state.archived_transition_count.saturating_add(1);
        }
    }

    fn commit_receipt(&mut self, operation_id: String, argument_hash: String, revision: u64) {
        self.state
            .operation_receipts
            .push(AgentSessionPlanOperationReceipt {
                operation_id,
                argument_hash,
                operation_revision: revision,
            });
        if self.state.operation_receipts.len() > MAX_RECEIPTS {
            let remove = self.state.operation_receipts.len() - MAX_RECEIPTS;
            self.state.operation_receipts.drain(..remove);
        }
    }

    fn finish_mutation(&mut self, revision: u64, updated_at: i64) {
        self.state.revision = revision;
        self.state.updated_at = updated_at;
        debug_assert!(validate_state(&self.state).is_ok());
    }
}

pub fn available_transitions(
    state: AgentSessionPlanPhase,
) -> Vec<AgentSessionPlanAvailableTransition> {
    match state {
        AgentSessionPlanPhase::Drafting => vec![transition(
            AgentSessionPlanOperation::Submit,
            AgentSessionPlanPhase::AwaitingConfirmation,
            AgentSessionPlanTransitionActor::Agent,
            "A complete plan begins with a level-one Markdown heading.",
            "Call plan_submit with expectedRevision and the complete plan.",
        )],
        AgentSessionPlanPhase::AwaitingConfirmation => vec![
            transition(
                AgentSessionPlanOperation::Approve,
                AgentSessionPlanPhase::Approved,
                AgentSessionPlanTransitionActor::User,
                "The user approves the exact pending plan Interaction.",
                "Wait for the pending user response; do not call another Plan mutation.",
            ),
            transition(
                AgentSessionPlanOperation::RequestRevision,
                AgentSessionPlanPhase::RevisionRequested,
                AgentSessionPlanTransitionActor::User,
                "The user selects Revise or supplies revision feedback.",
                "Wait for the pending user response; then read plan_current before resubmitting.",
            ),
        ],
        AgentSessionPlanPhase::RevisionRequested => vec![
            transition(
                AgentSessionPlanOperation::Submit,
                AgentSessionPlanPhase::AwaitingConfirmation,
                AgentSessionPlanTransitionActor::Agent,
                "The requested changes have been incorporated into a complete replacement plan.",
                "Call plan_submit with the current expectedRevision and revised plan.",
            ),
            transition(
                AgentSessionPlanOperation::Restart,
                AgentSessionPlanPhase::Drafting,
                AgentSessionPlanTransitionActor::Agent,
                "The old document should be discarded before drafting a replacement.",
                "Call plan_restart with the current expectedRevision and a non-empty reason.",
            ),
        ],
        AgentSessionPlanPhase::Approved => vec![transition(
            AgentSessionPlanOperation::Restart,
            AgentSessionPlanPhase::Drafting,
            AgentSessionPlanTransitionActor::Agent,
            "A material requirement change requires a new Plan lifecycle.",
            "Call plan_restart with the current expectedRevision and a non-empty reason.",
        )],
    }
}

fn transition(
    operation: AgentSessionPlanOperation,
    target_state: AgentSessionPlanPhase,
    actor: AgentSessionPlanTransitionActor,
    condition: &str,
    action: &str,
) -> AgentSessionPlanAvailableTransition {
    AgentSessionPlanAvailableTransition {
        operation,
        target_state,
        actor,
        condition: condition.to_string(),
        action: action.to_string(),
    }
}

fn recovery_actions(state: AgentSessionPlanPhase) -> Vec<String> {
    available_transitions(state)
        .into_iter()
        .map(|transition| transition.action)
        .collect()
}

pub fn validate_plan(plan: &str) -> Result<(), String> {
    let trimmed = plan.trim();
    if trimmed.len() > MAX_PLAN_BYTES {
        return Err(format!("plan exceeds the {MAX_PLAN_BYTES}-byte limit"));
    }
    let mut characters = trimmed.chars();
    if characters.next() != Some('#')
        || !characters.next().is_some_and(char::is_whitespace)
        || !characters.any(|character| !character.is_whitespace())
    {
        return Err("plan must start with a level-one Markdown heading".to_string());
    }
    Ok(())
}

pub fn validate_state(state: &AgentSessionPlanState) -> Result<(), AgentSessionPlanError> {
    match state.state {
        AgentSessionPlanPhase::Drafting => {
            if state.pending_interaction_id.is_some() {
                return Err(AgentSessionPlanError::new(
                    "drafting cannot retain a pending Interaction",
                ));
            }
        }
        AgentSessionPlanPhase::AwaitingConfirmation => {
            if state.document.is_none() || state.pending_interaction_id.is_none() {
                return Err(AgentSessionPlanError::new(
                    "awaitingConfirmation requires a document and pending Interaction",
                ));
            }
        }
        AgentSessionPlanPhase::RevisionRequested | AgentSessionPlanPhase::Approved => {
            if state.document.is_none() || state.pending_interaction_id.is_some() {
                return Err(AgentSessionPlanError::new(
                    "resolved Plan states require a document and no pending Interaction",
                ));
            }
        }
    }
    if let Some(document) = &state.document {
        validate_plan(&document.markdown).map_err(AgentSessionPlanError::new)?;
        let actual_hash = canonical_content_hash(document.markdown.as_bytes());
        if actual_hash != document.content_hash {
            return Err(AgentSessionPlanError::new(
                "Plan document content hash does not match its Markdown",
            ));
        }
    }
    if state
        .last_revision_feedback
        .as_ref()
        .is_some_and(|feedback| feedback.len() > MAX_FEEDBACK_BYTES)
    {
        return Err(AgentSessionPlanError::new(
            "revision feedback exceeds its size limit",
        ));
    }
    if state.history_tail.len() > MAX_HISTORY || state.operation_receipts.len() > MAX_RECEIPTS {
        return Err(AgentSessionPlanError::new(
            "Plan history or operation receipts exceed their bounded tail",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn submit(expected_revision: u64, operation_id: &str) -> AgentSessionPlanSubmitCommand {
        AgentSessionPlanSubmitCommand {
            expected_revision,
            plan: "# Plan\n\nImplement and verify.".to_string(),
            interaction_id: format!("interaction-{expected_revision}"),
            operation_id: operation_id.to_string(),
            argument_hash: format!("hash-{operation_id}"),
            submitted_at: 10 + expected_revision as i64,
        }
    }

    #[test]
    fn fixed_machine_supports_revision_and_approval_cycles() {
        let mut machine = AgentSessionPlanMachine::default();
        assert!(machine.submit(submit(0, "submit-1")).accepted);
        assert_eq!(
            machine.state().state,
            AgentSessionPlanPhase::AwaitingConfirmation
        );

        let revised = machine.resolve(AgentSessionPlanResolveCommand {
            expected_revision: 1,
            interaction_id: "interaction-0".to_string(),
            operation_id: "resolve-1".to_string(),
            argument_hash: "resolve-hash-1".to_string(),
            decision: AgentSessionPlanConfirmationDecision::RequestRevision {
                feedback: "Add the integration test.".to_string(),
            },
            resolved_at: 12,
        });
        assert_eq!(revised.code, AgentSessionPlanResultCode::RevisionRequested);
        assert_eq!(
            machine.state().state,
            AgentSessionPlanPhase::RevisionRequested
        );

        assert!(machine.submit(submit(2, "submit-2")).accepted);
        let approved = machine.resolve(AgentSessionPlanResolveCommand {
            expected_revision: 3,
            interaction_id: "interaction-2".to_string(),
            operation_id: "resolve-2".to_string(),
            argument_hash: "resolve-hash-2".to_string(),
            decision: AgentSessionPlanConfirmationDecision::Approve,
            resolved_at: 14,
        });
        assert_eq!(approved.code, AgentSessionPlanResultCode::Approved);
        assert_eq!(machine.state().state, AgentSessionPlanPhase::Approved);
        assert_eq!(machine.state().document.as_ref().unwrap().version, 2);
    }

    #[test]
    fn invalid_state_response_contains_complete_recovery_prompt() {
        let mut machine = AgentSessionPlanMachine::default();
        machine.submit(submit(0, "submit-1"));
        let rejected = machine.submit(submit(1, "submit-2"));

        assert!(!rejected.accepted);
        assert_eq!(rejected.code, AgentSessionPlanResultCode::InvalidState);
        let error = rejected.error.expect("state rejection has details");
        assert_eq!(
            error.current_state,
            AgentSessionPlanPhase::AwaitingConfirmation
        );
        assert_eq!(error.current_revision, 1);
        assert_eq!(error.allowed_transitions.len(), 2);
        assert!(error.message.contains("interaction-0"));
        assert!(
            error
                .recovery_actions
                .iter()
                .all(|action| !action.is_empty())
        );
    }

    #[test]
    fn operation_identity_is_idempotent_and_rejects_argument_conflicts() {
        let mut machine = AgentSessionPlanMachine::default();
        let command = submit(0, "submit-1");
        let first = machine.submit(command.clone());
        let replay = machine.submit(command.clone());
        let mut conflict = command;
        conflict.argument_hash = "different".to_string();
        let conflict = machine.submit(conflict);

        assert_eq!(first.code, AgentSessionPlanResultCode::Submitted);
        assert_eq!(replay.code, AgentSessionPlanResultCode::AlreadyApplied);
        assert_eq!(machine.state().revision, 1);
        assert_eq!(
            conflict.code,
            AgentSessionPlanResultCode::OperationIdentityConflict
        );
        assert!(!conflict.accepted);
    }

    #[test]
    fn restart_is_only_available_after_resolution() {
        let mut machine = AgentSessionPlanMachine::default();
        let rejected = machine.restart(AgentSessionPlanRestartCommand {
            expected_revision: 0,
            reason: "New requirements".to_string(),
            operation_id: "restart-1".to_string(),
            argument_hash: "restart-hash".to_string(),
            restarted_at: 1,
        });
        assert_eq!(rejected.code, AgentSessionPlanResultCode::InvalidState);
        assert_eq!(rejected.snapshot.state, AgentSessionPlanPhase::Drafting);
    }
}
