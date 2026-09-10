//! Dynamic workflow tools own decoding and rendering; core sees only versioned extension mutations.
use super::machine::{
    EmptyInput, RestartInput, TransitionInput, WorkflowMachine, WorkflowOperation,
};
use crate::{mode::RegisteredThreadMode, thread_assembler::ThreadAssemblyError};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::extensions::ExtensionMutation,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pl_protocol::WorkflowSessionState;
use std::sync::Arc;

/// Product-owned extension namespace; it is not a core schema or a database table.
pub const WORKFLOW_EXTENSION: &str = "studio.workflow";
const FORMAT: &str = "pl.studio.workflow";

#[derive(Debug, thiserror::Error)]
enum WorkflowPayloadError {
    #[error("workflow state is not initialized")]
    Missing,
    #[error("unsupported workflow payload {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("workflow graph differs from the saved run; an explicit product upgrade is required")]
    GraphMismatch,
}

/// Encodes a product state without exposing its structure to core.
///
/// # Errors
/// Returns invalid state size or encoding errors.
pub fn encode_workflow_state(state: &WorkflowSessionState) -> Result<OpaquePayload, ToolError> {
    crate::mode::state::validate_session_state_size(state).map_err(ToolError::new)?;
    OpaquePayload::new(
        FORMAT,
        1,
        serde_json::to_string(state).map_err(ToolError::new)?,
    )
    .map_err(ToolError::new)
}

/// Decodes an explicitly supported product format; unknown versions are never defaulted.
///
/// # Errors
/// Rejects unknown formats, invalid state JSON and excessive state size.
pub fn decode_workflow_state(payload: &OpaquePayload) -> Result<WorkflowSessionState, ToolError> {
    if payload.format() != FORMAT || payload.version() != 1 {
        return Err(ToolError::new(WorkflowPayloadError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        }));
    }
    let state = serde_json::from_str(payload.content()).map_err(ToolError::new)?;
    crate::mode::state::validate_session_state_size(&state).map_err(ToolError::new)?;
    Ok(state)
}

#[derive(Debug, Clone, Copy)]
enum WorkflowOperationKind {
    Current,
    Next,
    Graph,
    History,
    Transition,
    Restart,
}
impl WorkflowOperationKind {
    fn declaration(self) -> pl_protocol::ToolSpec {
        let (name, description, schema) = match self {
            Self::Current => (
                "workflow_current",
                "Read the current workflow run, state and CAS revision.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::Next => (
                "workflow_next",
                "Read direct next workflow transitions and their guards.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::Graph => (
                "workflow_graph",
                "Read the frozen workflow graph used by this Thread's tools.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::History => (
                "workflow_history",
                "Read saved workflow transitions and archive receipts.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::Transition => (
                "workflow_transition",
                "Complete the current workflow state and cross one direct edge using run, revision and state CAS. Call this tool alone.",
                schemars::schema_for!(TransitionInput).to_value(),
            ),
            Self::Restart => (
                "workflow_restart",
                "Archive the current run and begin a new lineage using run, revision and state CAS. Call this tool alone.",
                schemars::schema_for!(RestartInput).to_value(),
            ),
        };
        pl_protocol::ToolSpec::function(name, description, schema)
    }
    fn id(self) -> &'static str {
        match self {
            Self::Current => "workflow_current",
            Self::Next => "workflow_next",
            Self::Graph => "workflow_graph",
            Self::History => "workflow_history",
            Self::Transition => "workflow_transition",
            Self::Restart => "workflow_restart",
        }
    }
}

#[derive(Debug)]
struct ThreadWorkflowTool {
    mode: Arc<RegisteredThreadMode>,
    kind: WorkflowOperationKind,
}
impl Tool for ThreadWorkflowTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(WorkflowPayloadError::Unsupported {
                format: input.format().into(),
                version: input.version(),
            }));
        }
        let saved = context
            .extensions
            .get(WORKFLOW_EXTENSION)
            .ok_or_else(|| ToolError::new(WorkflowPayloadError::Missing))?;
        let previous = decode_workflow_state(&saved.payload)?;
        let mut machine = WorkflowMachine::new(self.mode.clone(), previous.clone());
        if let Some(run) = &previous.current_run
            && (run.mode_id != self.mode.descriptor().id
                || run.graph_hash != machine.graph().graph_hash())
        {
            return Err(ToolError::new(WorkflowPayloadError::GraphMismatch));
        }
        let identity = WorkflowOperation {
            turn_id: context.turn_id,
            call_id: context.call_id,
        };
        let hash = pl_core::context::content_hash(input.content().as_bytes());
        let response = match self.kind {
            WorkflowOperationKind::Current
            | WorkflowOperationKind::Next
            | WorkflowOperationKind::Graph
            | WorkflowOperationKind::History => {
                let _: EmptyInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                match self.kind {
                    WorkflowOperationKind::Current => machine.current_snapshot(&previous),
                    WorkflowOperationKind::Next => machine.next_snapshot(&previous),
                    WorkflowOperationKind::Graph => machine.graph_snapshot(&previous),
                    WorkflowOperationKind::History => machine.history_snapshot(&previous),
                    WorkflowOperationKind::Transition | WorkflowOperationKind::Restart => {
                        return Err(ToolError::new(WorkflowPayloadError::GraphMismatch));
                    }
                }
            }
            WorkflowOperationKind::Transition => serde_json::to_value(
                machine
                    .apply_transition(
                        serde_json::from_str(input.content()).map_err(ToolError::new)?,
                        &identity,
                        hash,
                    )
                    .map_err(ToolError::new)?,
            )
            .map_err(ToolError::new)?,
            WorkflowOperationKind::Restart => serde_json::to_value(
                machine
                    .apply_restart(
                        serde_json::from_str(input.content()).map_err(ToolError::new)?,
                        &identity,
                        hash,
                    )
                    .map_err(ToolError::new)?,
            )
            .map_err(ToolError::new)?,
        };
        let text = serde_json::to_string(&response).map_err(ToolError::new)?;
        let mut output = ToolOutput::new(
            OpaquePayload::new("pl.studio.workflow-result", 1, text.clone())
                .map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(text),
            }],
        );
        let next = machine.state();
        if next != previous {
            output = output.with_extension_mutations(vec![ExtensionMutation::Put {
                id: WORKFLOW_EXTENSION.into(),
                expected_revision: Some(saved.revision),
                payload: encode_workflow_state(&next)?,
            }]);
        }
        Ok(output)
    }
}

/// Creates Thread-owned workflow operations from a frozen product Mode.
///
/// # Errors
/// Returns model declaration or registration failures.
pub fn workflow_registrations(
    mode: Arc<RegisteredThreadMode>,
) -> Result<Vec<Registration>, ThreadAssemblyError> {
    if mode.workflow().is_none() {
        return Ok(Vec::new());
    }
    [
        WorkflowOperationKind::Current,
        WorkflowOperationKind::Next,
        WorkflowOperationKind::Graph,
        WorkflowOperationKind::History,
        WorkflowOperationKind::Transition,
        WorkflowOperationKind::Restart,
    ]
    .into_iter()
    .map(|kind| {
        let declaration = pl_model::runtime::thread_tool_declaration(&kind.declaration())?;
        let registration = Registration::new(
            kind.id().into(),
            declaration,
            ThreadWorkflowTool {
                mode: mode.clone(),
                kind,
            },
        )?;
        Ok(match kind {
            WorkflowOperationKind::Transition | WorkflowOperationKind::Restart => {
                registration.with_extension_updates()
            }
            WorkflowOperationKind::Current
            | WorkflowOperationKind::Next
            | WorkflowOperationKind::Graph
            | WorkflowOperationKind::History => registration,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::thread::extensions::ExtensionRecord;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeMap;

    fn fixture() -> (Arc<RegisteredThreadMode>, WorkflowSessionState, CallContext) {
        let manager = crate::mode::ThreadModeManager::default();
        let mode = crate::mode::test_support::registered(
            &manager,
            "workflow-test",
            "role",
            crate::mode::test_support::graph(""),
        );
        let state = crate::mode::reconcile_workflow_for_turn(None, &mode, "seed", 1)
            .unwrap()
            .unwrap();
        let context = CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "transition".into(),
            cancellation: Default::default(),
            catalog: Vec::new().into(),
            extension_sequence: 7,
            extensions: Arc::new(BTreeMap::from([(
                WORKFLOW_EXTENSION.into(),
                ExtensionRecord {
                    revision: 7,
                    payload: encode_workflow_state(&state).unwrap(),
                },
            )])),
        };
        (mode, state, context)
    }

    fn input(state: &WorkflowSessionState, revision: u64) -> OpaquePayload {
        let run = state.current_run.as_ref().unwrap();
        OpaquePayload::new("application/json", 1, serde_json::json!({
            "expectedRunId":run.run_id, "expectedRevision":revision, "expectedStateId":run.current_state_id,
            "targetStateId":"done", "completion":{"reason":"criteria satisfied", "summary":"verified", "evidence":["check passed"]},
        }).to_string()).unwrap()
    }

    #[tokio::test]
    async fn workflow_transition_proposes_cas_without_mutating_the_frozen_input_state() {
        let (mode, state, context) = fixture();
        let original = context.extensions.clone();
        let tool = ThreadWorkflowTool {
            mode,
            kind: WorkflowOperationKind::Transition,
        };
        let output = tool
            .execute(input(&state, state.revision), context.clone())
            .await
            .unwrap();
        let [
            ExtensionMutation::Put {
                id,
                expected_revision,
                payload,
            },
        ] = output.extension_mutations()
        else {
            panic!("expected one workflow CAS")
        };
        assert_eq!(id, WORKFLOW_EXTENSION);
        assert_eq!(*expected_revision, Some(7));
        assert_eq!(
            decode_workflow_state(payload)
                .unwrap()
                .current_run
                .unwrap()
                .current_state_id,
            "done"
        );
        assert_eq!(context.extensions, original);
        let rejected = tool
            .execute(input(&state, state.revision + 1), context)
            .await
            .unwrap();
        assert!(rejected.extension_mutations().is_empty());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(rejected.payload().content()).unwrap()["accepted"],
            false
        );
    }

    #[tokio::test]
    async fn unknown_workflow_format_remains_intact_and_blocks_execution() {
        let (mode, _, mut context) = fixture();
        let unknown = OpaquePayload::new("future.workflow", 99, "uninterpreted state\n").unwrap();
        context.extensions = Arc::new(BTreeMap::from([(
            WORKFLOW_EXTENSION.into(),
            ExtensionRecord {
                revision: 7,
                payload: unknown.clone(),
            },
        )]));
        let tool = ThreadWorkflowTool {
            mode,
            kind: WorkflowOperationKind::Current,
        };
        assert!(
            tool.execute(
                OpaquePayload::new("application/json", 1, "{}").unwrap(),
                context.clone()
            )
            .await
            .is_err()
        );
        assert_eq!(context.extensions[WORKFLOW_EXTENSION].payload, unknown);
    }
}
