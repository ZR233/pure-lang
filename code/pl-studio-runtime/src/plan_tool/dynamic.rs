//! Plan tool frontends over core's opaque extension and interaction transactions.
use super::{
    common::EmptyInput,
    restart::PlanRestartInput,
    submit::{PlanSubmitInput, confirmation_question},
};
use crate::plan_tool::state::{
    AgentSessionPlanMachine, AgentSessionPlanOptions, AgentSessionPlanRestartCommand,
    AgentSessionPlanSubmitCommand,
};
use crate::thread_assembler::ThreadAssemblyError;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::extensions::ExtensionMutation,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pl_protocol::AgentSessionPlanState;
use std::sync::Arc;

pub const PLAN_EXTENSION: &str = "studio.plan";
const FORMAT: &str = "pl.studio.plan";

#[derive(Debug, thiserror::Error)]
enum PlanPayloadError {
    #[error("plan state is not initialized")]
    Missing,
    #[error("unsupported plan payload {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("invalid plan: {0}")]
    InvalidPlan(String),
}

/// Encodes the complete product state for a generic Thread extension.
///
/// # Errors
/// Rejects invalid or excessive plan state and encoding errors.
pub fn encode_plan_state(state: &AgentSessionPlanState) -> Result<OpaquePayload, ToolError> {
    crate::plan_tool::state::validate_session_state_size(state).map_err(ToolError::new)?;
    OpaquePayload::new(
        FORMAT,
        1,
        serde_json::to_string(state).map_err(ToolError::new)?,
    )
    .map_err(ToolError::new)
}

/// Decodes only an explicitly supported plan format; core history never calls this function.
///
/// # Errors
/// Rejects unknown payload formats, corrupt plan state and excessive content.
pub fn decode_plan_state(payload: &OpaquePayload) -> Result<AgentSessionPlanState, ToolError> {
    if payload.format() != FORMAT || payload.version() != 1 {
        return Err(ToolError::new(PlanPayloadError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        }));
    }
    let state = serde_json::from_str(payload.content()).map_err(ToolError::new)?;
    crate::plan_tool::state::validate_session_state_size(&state).map_err(ToolError::new)?;
    Ok(state)
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlanConfirmationPrompt {
    pub question: pl_protocol::UserQuestion,
    pub expected_plan_revision: u64,
    pub plan_hash: String,
    pub created_at: i64,
    pub presentation: pl_protocol::MessagePresentation,
}

#[derive(Debug, Clone, Copy)]
enum PlanOperation {
    Current,
    Next,
    History,
    Submit,
    Restart,
}
impl PlanOperation {
    fn name(self) -> &'static str {
        match self {
            Self::Current => "plan_current",
            Self::Next => "plan_next",
            Self::History => "plan_history",
            Self::Submit => "plan_submit",
            Self::Restart => "plan_restart",
        }
    }
    fn declaration(self) -> pl_protocol::ToolSpec {
        let (description, schema) = match self {
            Self::Current => (
                "Read the current Plan, state and CAS revision.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::Next => (
                "Read permitted operations in the current fixed Plan state.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::History => (
                "Read saved Plan transitions and archive receipts.",
                schemars::schema_for!(EmptyInput).to_value(),
            ),
            Self::Submit => (
                "Submit a complete Markdown Plan for user approval or revision. This is the only tool for asking the user to approve implementation of a complete Plan; do not first ask whether to implement through request_user_input or final text. Requires revision CAS; call this tool alone.",
                schemars::schema_for!(PlanSubmitInput).to_value(),
            ),
            Self::Restart => (
                "Restart an approved or revision-requested Plan using revision CAS and a reason. Call this tool alone.",
                schemars::schema_for!(PlanRestartInput).to_value(),
            ),
        };
        pl_protocol::ToolSpec::function(self.name(), description, schema)
    }
}

#[derive(Debug)]
struct ThreadPlanTool {
    kind: PlanOperation,
    options: AgentSessionPlanOptions,
}
impl Tool for ThreadPlanTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        if input.format() != "application/json" || input.version() != 1 {
            return Err(ToolError::new(PlanPayloadError::Unsupported {
                format: input.format().into(),
                version: input.version(),
            }));
        }
        let saved = context
            .extensions
            .get(PLAN_EXTENSION)
            .ok_or_else(|| ToolError::new(PlanPayloadError::Missing))?;
        let previous = decode_plan_state(&saved.payload)?;
        let mut machine = AgentSessionPlanMachine::new(previous.clone()).map_err(ToolError::new)?;
        let operation_id = format!(
            "{}/{}/{}",
            context.thread_id, context.turn_id, context.call_id
        );
        let argument_hash = pl_core::context::content_hash(input.content().as_bytes());
        let now = crate::studio::unix_seconds();
        let mut interaction = None;
        let result = match self.kind {
            PlanOperation::Current => {
                let _: EmptyInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                serde_json::to_value(machine.snapshot()).map_err(ToolError::new)?
            }
            PlanOperation::Next => {
                let _: EmptyInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                serde_json::json!({"revision": machine.state().revision, "state": machine.state().state, "transitions": machine.available_transitions()})
            }
            PlanOperation::History => {
                let _: EmptyInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                serde_json::json!({"revision":previous.revision,"state":previous.state,"history":previous.history_tail,"archivedTransitionCount":previous.archived_transition_count,"archivedTransitionDigest":previous.archived_transition_digest})
            }
            PlanOperation::Submit => {
                let args: PlanSubmitInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                crate::plan_tool::state::validate_plan(&args.plan)
                    .map_err(|error| ToolError::new(PlanPayloadError::InvalidPlan(error)))?;
                let question = confirmation_question(args.plan.clone());
                let plan_hash = pl_core::context::content_hash(args.plan.as_bytes());
                let result = machine.submit(AgentSessionPlanSubmitCommand {
                    expected_revision: args.expected_revision,
                    plan: args.plan,
                    interaction_id: context.interaction_id(),
                    operation_id,
                    argument_hash,
                    submitted_at: now,
                });
                if result.accepted && machine.state() != &previous {
                    let prompt = PlanConfirmationPrompt {
                        question,
                        expected_plan_revision: machine.state().revision,
                        plan_hash,
                        created_at: now,
                        presentation: self.options.submitted_plan_presentation(),
                    };
                    interaction = Some(
                        OpaquePayload::new(
                            "pl.studio.plan-confirmation",
                            1,
                            serde_json::to_string(&prompt).map_err(ToolError::new)?,
                        )
                        .map_err(ToolError::new)?,
                    );
                }
                serde_json::to_value(result).map_err(ToolError::new)?
            }
            PlanOperation::Restart => {
                let args: PlanRestartInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                serde_json::to_value(machine.restart(AgentSessionPlanRestartCommand {
                    expected_revision: args.expected_revision,
                    reason: args.reason,
                    operation_id,
                    argument_hash,
                    restarted_at: now,
                }))
                .map_err(ToolError::new)?
            }
        };
        let text = serde_json::to_string(&result).map_err(ToolError::new)?;
        let mut output = ToolOutput::new(
            OpaquePayload::new("pl.studio.plan-result", 1, text.clone()).map_err(ToolError::new)?,
            vec![ContextContent::Text {
                text: Arc::from(text),
            }],
        );
        if machine.state() != &previous {
            output = output.with_extension_mutations(vec![ExtensionMutation::Put {
                id: PLAN_EXTENSION.into(),
                expected_revision: Some(saved.revision),
                payload: encode_plan_state(machine.state())?,
            }]);
        }
        if let Some(prompt) = interaction {
            output = output.with_interaction(prompt);
        }
        Ok(output)
    }
}

/// Registers independent Plan tools without a mutable product session handle.
///
/// # Errors
/// Returns model declaration or core registration failures.
pub fn plan_registrations(
    options: AgentSessionPlanOptions,
) -> Result<Vec<Registration>, ThreadAssemblyError> {
    [
        PlanOperation::Current,
        PlanOperation::Next,
        PlanOperation::History,
        PlanOperation::Submit,
        PlanOperation::Restart,
    ]
    .into_iter()
    .map(|kind| {
        let declaration = pl_model::runtime::thread_tool_declaration(&kind.declaration())?;
        let registration = Registration::new(
            kind.name().into(),
            declaration,
            ThreadPlanTool { kind, options },
        )?;
        Ok(match kind {
            PlanOperation::Submit => registration.with_extension_updates().with_interactions(),
            PlanOperation::Restart => registration.with_extension_updates(),
            PlanOperation::Current | PlanOperation::Next | PlanOperation::History => registration,
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

    fn context() -> CallContext {
        CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "submit".into(),
            cancellation: Default::default(),
            catalog: Vec::new().into(),
            extension_sequence: 3,
            extensions: Arc::new(BTreeMap::from([(
                PLAN_EXTENSION.into(),
                ExtensionRecord {
                    revision: 3,
                    payload: encode_plan_state(&Default::default()).unwrap(),
                },
            )])),
        }
    }

    #[tokio::test]
    async fn plan_submission_returns_one_atomic_state_change_and_matching_question() {
        let tool = ThreadPlanTool {
            kind: PlanOperation::Submit,
            options: Default::default(),
        };
        let context = context();
        let id = context.interaction_id();
        let original = context.extensions.clone();
        let markdown = "# Plan\n\nImplement and verify.\n";
        let input = OpaquePayload::new(
            "application/json",
            1,
            serde_json::json!({"expectedRevision":0,"plan":markdown}).to_string(),
        )
        .unwrap();
        let result = tool.execute(input, context.clone()).await.unwrap();
        assert_eq!(
            result.control(),
            pl_core::tool::ToolControl::AwaitInteraction
        );
        let [
            ExtensionMutation::Put {
                id: extension,
                expected_revision,
                payload,
            },
        ] = result.extension_mutations()
        else {
            panic!("expected one plan CAS")
        };
        assert_eq!(extension, PLAN_EXTENSION);
        assert_eq!(*expected_revision, Some(3));
        let state = decode_plan_state(payload).unwrap();
        assert_eq!(state.pending_interaction_id.as_deref(), Some(id.as_str()));
        let prompt: PlanConfirmationPrompt =
            serde_json::from_str(result.interaction().unwrap().content()).unwrap();
        assert_eq!(prompt.expected_plan_revision, state.revision);
        assert_eq!(prompt.question.question, markdown);
        assert_eq!(context.extensions, original);
    }

    #[tokio::test]
    async fn stale_plan_submission_neither_changes_state_nor_requests_confirmation() {
        let tool = ThreadPlanTool {
            kind: PlanOperation::Submit,
            options: Default::default(),
        };
        let result = tool
            .execute(
                OpaquePayload::new(
                    "application/json",
                    1,
                    r##"{"expectedRevision":10,"plan":"# Plan\n\nWork."}"##,
                )
                .unwrap(),
                context(),
            )
            .await
            .unwrap();
        assert!(result.extension_mutations().is_empty());
        assert!(result.interaction().is_none());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(result.payload().content()).unwrap()["accepted"],
            false
        );
    }
}
