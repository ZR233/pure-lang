use std::sync::Arc;

use pl_protocol::{PureError, WorkflowSessionState, WorkflowTransitionRecord};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::mode::RegisteredThreadMode;
use crate::mode::new_run;
use crate::mode::state::{
    archive_run, commit_operation_receipt, lifecycle_for_state, operation_receipt, trim_history,
};

pub(super) const MAX_REASON_BYTES: usize = 2 * 1024;
pub(super) const MAX_SUMMARY_BYTES: usize = 8 * 1024;
pub(super) const MAX_EVIDENCE_ITEMS: usize = 16;
pub(super) const MAX_EVIDENCE_ITEM_BYTES: usize = 2 * 1024;

#[derive(Debug)]
pub(super) struct WorkflowOperation {
    pub turn_id: String,
    pub call_id: String,
}

#[derive(Debug, Clone)]
pub(super) struct WorkflowMachine {
    state: WorkflowSessionState,
    pub mode: Arc<RegisteredThreadMode>,
}

impl WorkflowMachine {
    pub fn new(mode: Arc<RegisteredThreadMode>, state: WorkflowSessionState) -> Self {
        Self { mode, state }
    }

    pub fn state(&self) -> WorkflowSessionState {
        self.state.clone()
    }

    pub fn graph(&self) -> &crate::mode::CompiledWorkflowDefinition {
        self.mode
            .workflow()
            .expect("workflow tools require a registered graph")
    }

    pub fn current_snapshot(&self, state: &WorkflowSessionState) -> serde_json::Value {
        let Some(run) = &state.current_run else {
            return serde_json::json!({
                "modeId": self.mode.descriptor().id,
                "revision": state.revision,
                "currentRun": null,
            });
        };
        let current = self.graph().state(&run.current_state_id);
        serde_json::json!({
            "modeId": run.mode_id,
            "runId": run.run_id,
            "lineageId": run.lineage_id,
            "revision": state.revision,
            "graphRevision": run.graph_revision,
            "graphHash": run.graph_hash,
            "lifecycle": run.lifecycle,
            "currentStateId": run.current_state_id,
            "currentState": current,
            "updatedAt": run.updated_at,
        })
    }

    pub fn next_snapshot(&self, state: &WorkflowSessionState) -> serde_json::Value {
        let Some(run) = &state.current_run else {
            return serde_json::json!({
                "modeId": self.mode.descriptor().id,
                "revision": state.revision,
                "currentRun": null,
                "transitions": [],
            });
        };
        serde_json::json!({
            "modeId": run.mode_id,
            "runId": run.run_id,
            "revision": state.revision,
            "currentStateId": run.current_state_id,
            "transitions": self.graph().outgoing(&run.current_state_id),
        })
    }

    pub fn graph_snapshot(&self, state: &WorkflowSessionState) -> serde_json::Value {
        serde_json::json!({
            "modeId": self.mode.descriptor().id,
            "revision": state.revision,
            "graphRevision": self.mode.graph_revision(),
            "graphHash": self.graph().graph_hash(),
            "definition": self.graph().definition(),
        })
    }

    pub fn history_snapshot(&self, state: &WorkflowSessionState) -> serde_json::Value {
        state.current_run.as_ref().map_or_else(
            || serde_json::json!({ "revision": state.revision, "currentRun": null }),
            |run| {
                serde_json::json!({
                    "modeId": run.mode_id,
                    "runId": run.run_id,
                    "revision": state.revision,
                    "history": run.history_tail,
                    "archivedTransitionCount": run.archived_transition_count,
                    "archivedTransitionDigest": run.archived_transition_digest,
                    "archivedRuns": state.archived_runs,
                    "archivedRunCount": state.archived_run_count,
                    "archivedRunDigest": state.archived_run_digest,
                })
            },
        )
    }

    pub fn apply_transition(
        &mut self,
        input: TransitionInput,
        identity: &WorkflowOperation,
        argument_hash: String,
    ) -> Result<WorkflowMutationResponse, PureError> {
        let mut state = self.state.clone();
        let operation_id = operation_id(identity);
        if let Some(response) = idempotent_response(self, &state, &operation_id, &argument_hash) {
            return Ok(response);
        }
        if let Some(response) = validate_cas(self, &state, &input.cas()) {
            return Ok(response);
        }
        if let Some(message) = validate_completion(&input.completion) {
            return Ok(self.rejected("invalidCompletion", &state, message));
        }
        let run = state
            .current_run
            .as_ref()
            .expect("validated CAS requires a run");
        let Some(transition) = self
            .graph()
            .transition(&run.current_state_id, &input.target_state_id)
        else {
            return Ok(self.rejected(
                "transitionNotAllowed",
                &state,
                "Choose a target returned by workflow_next.",
            ));
        };
        let target_id = transition.target_state_id.clone();
        let target_kind = self
            .graph()
            .state(&target_id)
            .expect("compiled transition target exists")
            .kind;
        let next_revision = state.revision.saturating_add(1);
        let now = crate::studio::unix_seconds();
        let run = state
            .current_run
            .as_mut()
            .expect("validated CAS requires a run");
        let source_state_id = run.current_state_id.clone();
        run.current_state_id = target_id.clone();
        run.lifecycle = lifecycle_for_state(target_kind);
        run.updated_at = now;
        run.history_tail.push(WorkflowTransitionRecord {
            revision: next_revision,
            source_state_id,
            target_state_id: target_id,
            reason: input.completion.reason.trim().to_string(),
            summary: input.completion.summary.trim().to_string(),
            evidence: input
                .completion
                .evidence
                .into_iter()
                .map(|item| item.trim().to_string())
                .collect(),
            turn_id: identity.turn_id.clone(),
            call_id: identity.call_id.clone(),
            transitioned_at: now,
        });
        trim_history(run)?;
        commit_operation_receipt(&mut state, operation_id, argument_hash, next_revision);
        crate::mode::state::validate_session_state_size(&state)?;
        self.state = state.clone();
        Ok(self.accepted("transitioned", &state))
    }

    pub fn apply_restart(
        &mut self,
        input: RestartInput,
        identity: &WorkflowOperation,
        argument_hash: String,
    ) -> Result<WorkflowMutationResponse, PureError> {
        let mut state = self.state.clone();
        let operation_id = operation_id(identity);
        if let Some(response) = idempotent_response(self, &state, &operation_id, &argument_hash) {
            return Ok(response);
        }
        if let Some(response) = validate_cas(self, &state, &input.cas()) {
            return Ok(response);
        }
        if input.reason.trim().is_empty() || input.reason.len() > MAX_REASON_BYTES {
            return Ok(self.rejected(
                "invalidReason",
                &state,
                "reason must be non-empty and within the documented limit",
            ));
        }
        let old = state
            .current_run
            .take()
            .expect("validated CAS requires a run");
        let next_revision = state.revision.saturating_add(1);
        let now = crate::studio::unix_seconds();
        archive_run(&mut state, old, "restarted", input.reason.trim(), now)?;
        let seed = format!("{operation_id}:{argument_hash}:{next_revision}");
        state.current_run = Some(new_run(&self.mode, self.graph(), None, &seed, now));
        commit_operation_receipt(&mut state, operation_id, argument_hash, next_revision);
        crate::mode::state::validate_session_state_size(&state)?;
        self.state = state.clone();
        Ok(self.accepted("restarted", &state))
    }

    fn accepted(
        &self,
        code: &'static str,
        state: &WorkflowSessionState,
    ) -> WorkflowMutationResponse {
        WorkflowMutationResponse {
            accepted: true,
            code,
            operation_revision: state.revision,
            snapshot: self.current_snapshot(state),
            recovery_action: None,
        }
    }

    fn rejected(
        &self,
        code: &'static str,
        state: &WorkflowSessionState,
        recovery_action: impl Into<String>,
    ) -> WorkflowMutationResponse {
        WorkflowMutationResponse {
            accepted: false,
            code,
            operation_revision: state.revision,
            snapshot: self.current_snapshot(state),
            recovery_action: Some(recovery_action.into()),
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransitionInput {
    /// Current run returned by `workflow_current`.
    pub expected_run_id: String,
    /// Current workflow revision returned by `workflow_current`.
    pub expected_revision: u64,
    /// Current state returned by `workflow_current`.
    pub expected_state_id: String,
    /// One direct successor returned by `workflow_next`.
    pub target_state_id: String,
    /// Completion declaration for the current state and selected edge.
    pub completion: CompletionInput,
}

impl TransitionInput {
    fn cas(&self) -> WorkflowCas<'_> {
        WorkflowCas {
            expected_run_id: &self.expected_run_id,
            expected_revision: self.expected_revision,
            expected_state_id: &self.expected_state_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionInput {
    /// Why the current state's criteria and selected edge guard are satisfied.
    pub reason: String,
    /// Concise result produced while completing the current state.
    pub summary: String,
    /// Concrete checks, receipts, or artifacts supporting the declaration.
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestartInput {
    pub expected_run_id: String,
    pub expected_revision: u64,
    pub expected_state_id: String,
    pub reason: String,
}

impl RestartInput {
    fn cas(&self) -> WorkflowCas<'_> {
        WorkflowCas {
            expected_run_id: &self.expected_run_id,
            expected_revision: self.expected_revision,
            expected_state_id: &self.expected_state_id,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkflowMutationResponse {
    pub accepted: bool,
    pub code: &'static str,
    pub operation_revision: u64,
    pub snapshot: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_action: Option<String>,
}

struct WorkflowCas<'a> {
    expected_run_id: &'a str,
    expected_revision: u64,
    expected_state_id: &'a str,
}

fn validate_cas(
    runtime: &WorkflowMachine,
    state: &WorkflowSessionState,
    cas: &WorkflowCas<'_>,
) -> Option<WorkflowMutationResponse> {
    let Some(run) = &state.current_run else {
        return Some(runtime.rejected(
            "workflowNotStarted",
            state,
            "Wait for the root Turn bootstrap before mutating workflow state.",
        ));
    };
    let mismatch = if run.run_id != cas.expected_run_id {
        Some("runMismatch")
    } else if state.revision != cas.expected_revision {
        Some("staleRevision")
    } else if run.current_state_id != cas.expected_state_id {
        Some("stateMismatch")
    } else if run.mode_id != runtime.mode.descriptor().id
        || run.graph_hash != runtime.graph().graph_hash()
    {
        Some("modeSnapshotMismatch")
    } else {
        None
    };
    mismatch.map(|code| {
        runtime.rejected(
            code,
            state,
            "Call workflow_current and retry with its canonical run, revision, and state.",
        )
    })
}

fn validate_completion(completion: &CompletionInput) -> Option<&'static str> {
    if completion.reason.trim().is_empty() || completion.reason.len() > MAX_REASON_BYTES {
        return Some("reason must be non-empty and within the documented limit");
    }
    if completion.summary.trim().is_empty() || completion.summary.len() > MAX_SUMMARY_BYTES {
        return Some("completion summary must be non-empty and within the documented limit");
    }
    if completion.evidence.len() > MAX_EVIDENCE_ITEMS
        || completion
            .evidence
            .iter()
            .any(|item| item.trim().is_empty() || item.len() > MAX_EVIDENCE_ITEM_BYTES)
    {
        return Some("completion evidence is empty, too large, or contains too many items");
    }
    None
}

fn idempotent_response(
    runtime: &WorkflowMachine,
    state: &WorkflowSessionState,
    operation_id: &str,
    argument_hash: &str,
) -> Option<WorkflowMutationResponse> {
    operation_receipt(state, operation_id).map(|receipt| {
        if receipt.argument_hash == argument_hash {
            runtime.accepted("alreadyApplied", state)
        } else {
            runtime.rejected(
                "operationIdentityConflict",
                state,
                "Retry with a new tool call identity.",
            )
        }
    })
}

fn operation_id(identity: &WorkflowOperation) -> String {
    format!("{}/{}", identity.turn_id, identity.call_id)
}
