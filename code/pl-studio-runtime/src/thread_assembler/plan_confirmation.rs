//! Product Plan confirmation over generic core interaction and extension commits.
use super::StudioThreadAssembler;
use crate::plan_tool::{
    PLAN_EXTENSION, PlanConfirmationPrompt, decode_plan_state, encode_plan_state,
};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{
        extensions::{ExtensionMutation, ExtensionRecord},
        input::ThreadInput,
        interactions::{
            InteractionRecord, InteractionResolution as CoreResolution, InteractionResponse,
            InteractionState,
        },
    },
};
use pl_protocol::{
    InteractionCommand, InteractionRequest, InteractionScope, ResolveUserInput, UserInputResolution,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum PlanInteractionError {
    #[error(transparent)]
    Key(#[from] super::InteractionKeyError),
    #[error("unsupported Plan interaction payload {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("Plan interaction payload is invalid")]
    Decode(#[from] serde_json::Error),
    #[error("Plan confirmation does not match its pending Plan or interaction revision")]
    Identity,
    #[error("invalid Plan confirmation answer: {0}")]
    Answer(String),
    #[error("Plan confirmation was rejected: {0:?}")]
    Rejected(pl_protocol::AgentSessionPlanResultCode),
    #[error("Plan interaction transition failed")]
    Projection(#[from] pl_protocol::InteractionTransitionError),
    #[error("Plan state could not be decoded or frozen")]
    State(#[from] pl_core::tool::opaque::ToolError),
    #[error("Plan state transition failed")]
    Machine(#[from] crate::plan_tool::state::AgentSessionPlanError),
    #[error("Plan context projection failed")]
    Context(#[from] crate::PureError),
    #[error("Plan continuation payload could not be frozen")]
    Payload(#[from] pl_core::context::PayloadError),
    #[error("Thread rejected Plan resolution")]
    Thread(#[from] pl_core::thread::ThreadError),
}

fn decode<T: serde::de::DeserializeOwned>(
    payload: &OpaquePayload,
    format: &str,
) -> Result<T, PlanInteractionError> {
    if payload.format() != format || payload.version() != 1 {
        return Err(PlanInteractionError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        });
    }
    Ok(serde_json::from_str(payload.content())?)
}

fn prompt(record: &InteractionRecord) -> Result<PlanConfirmationPrompt, PlanInteractionError> {
    let prompt: PlanConfirmationPrompt =
        decode(&record.request.payload, "pl.studio.plan-confirmation")?;
    if prompt.expected_plan_revision == 0
        || prompt.question.id != pl_protocol::AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID
        || pl_core::context::content_hash(prompt.question.question.as_bytes()) != prompt.plan_hash
    {
        return Err(PlanInteractionError::Identity);
    }
    Ok(prompt)
}

/// Converts saved question and answer facts into Studio DTOs without rerendering the Plan.
///
/// # Errors
/// Rejects unknown encodings, changed Plan text, contradictory revisions and invalid answers.
pub fn project_plan_confirmation(
    thread_id: &str,
    record: &InteractionRecord,
) -> Result<InteractionRequest, PlanInteractionError> {
    let prompt = prompt(record)?;
    let wire_id = super::interaction_key::encode(thread_id, &record.request.id);
    let mut projected = InteractionRequest::user_input(
        wire_id.clone(),
        InteractionScope {
            thread_id: thread_id.into(),
            turn_id: record.request.turn_id.clone(),
            item_id: None,
            tool_id: None,
            agent_path: Some(thread_id.into()),
            purpose: pl_protocol::InteractionPurpose::AgentSessionPlanConfirmation(
                pl_protocol::AgentSessionPlanConfirmationPurpose {
                    expected_revision: prompt.expected_plan_revision - 1,
                    operation_id: record.request.id.clone(),
                    argument_hash: pl_core::context::content_hash(
                        record.request.payload.content().as_bytes(),
                    ),
                    plan_hash: prompt.plan_hash,
                },
            ),
        },
        vec![prompt.question],
        record.created_at,
    )
    .with_continuation(pl_protocol::InteractionContinuationPreset::question(
        pl_protocol::AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID,
        prompt.presentation,
    ));
    projected.revision = 1;
    let command = match &record.state {
        InteractionState::Pending => None,
        InteractionState::Resolved(response) => {
            let answer: UserInputResolution = decode(&response.payload, "pl.studio.plan-answer")?;
            Some(InteractionCommand::ResolveUserInput(ResolveUserInput {
                interaction_id: wire_id.clone(),
                expected_revision: 1,
                operation_id: format!("resolve:{}", record.request.id),
                resolved_at: record.updated_at,
                answers: answer.answers,
            }))
        }
        InteractionState::Cancelled => {
            Some(InteractionCommand::Cancel(pl_protocol::CancelInteraction {
                interaction_id: wire_id.clone(),
                expected_revision: 1,
                operation_id: format!("cancel:{}", record.request.id),
                reason: "Plan confirmation was cancelled.".into(),
                cancelled_at: record.updated_at,
            }))
        }
    };
    if let Some(command) = command {
        let decision = projected.decide(command)?;
        projected.apply(decision, record.updated_at);
    }
    if projected.revision != record.revision {
        return Err(PlanInteractionError::Identity);
    }
    Ok(projected)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderedAnswer<'a> {
    answers: BTreeMap<&'a str, &'a pl_protocol::UserInputAnswer>,
}
fn answer_payload(resolution: &ResolveUserInput) -> Result<OpaquePayload, PlanInteractionError> {
    let answer = OrderedAnswer {
        answers: resolution
            .answers
            .iter()
            .map(|(id, answer)| (id.as_str(), answer))
            .collect(),
    };
    Ok(OpaquePayload::new(
        "pl.studio.plan-answer",
        1,
        serde_json::to_string(&answer)?,
    )?)
}

fn prepare_resolution(
    thread_id: &str,
    current: &InteractionRecord,
    saved_plan: &ExtensionRecord,
    resolution: &ResolveUserInput,
) -> Result<CoreResolution, PlanInteractionError> {
    if current.request.id != resolution.interaction_id
        || current.revision != resolution.expected_revision
        || !matches!(current.state, InteractionState::Pending)
    {
        return Err(PlanInteractionError::Identity);
    }
    let projected = project_plan_confirmation(thread_id, current)?;
    let mut projected_resolution = resolution.clone();
    projected_resolution.interaction_id = projected.interaction_id.clone();
    projected.decide(InteractionCommand::ResolveUserInput(projected_resolution))?;
    let prompt = prompt(current)?;
    let state = decode_plan_state(&saved_plan.payload)?;
    if state.revision != prompt.expected_plan_revision
        || state.pending_interaction_id.as_deref() != Some(current.request.id.as_str())
        || !state.document.as_ref().is_some_and(|document| {
            document.content_hash == prompt.plan_hash
                && document.markdown == prompt.question.question
        })
    {
        return Err(PlanInteractionError::Identity);
    }
    let answer = UserInputResolution {
        answers: resolution.answers.clone(),
    };
    let decision = crate::plan_tool::state::confirmation_decision(&answer)
        .map_err(PlanInteractionError::Answer)?;
    let payload = answer_payload(resolution)?;
    let mut machine = crate::plan_tool::state::AgentSessionPlanMachine::new(state)?;
    let result = machine.resolve(crate::plan_tool::state::AgentSessionPlanResolveCommand {
        expected_revision: prompt.expected_plan_revision,
        interaction_id: current.request.id.clone(),
        operation_id: format!("resolve:{}", current.request.id),
        argument_hash: pl_core::context::content_hash(payload.content().as_bytes()),
        decision,
        resolved_at: crate::studio::unix_seconds(),
    });
    if !result.accepted {
        return Err(PlanInteractionError::Rejected(result.code));
    }
    let projection = crate::plan_tool::plan_model_context_section(machine.state())?;
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Continuation<'a> {
        interaction_id: &'a str,
        presentation: pl_protocol::MessagePresentation,
    }
    let continuation_payload = OpaquePayload::new(
        "pl.studio.interaction-continuation",
        1,
        serde_json::to_string(&Continuation {
            interaction_id: &current.request.id,
            presentation: prompt.presentation,
        })?,
    )?;
    Ok(CoreResolution {
        id: current.request.id.clone(),
        expected_revision: current.revision,
        response: InteractionResponse {
            context: vec![
                ContextContent::Text {
                    text: Arc::from(format!("Plan confirmation: {}", payload.content())),
                },
                ContextContent::Text {
                    text: projection.content.into(),
                },
            ],
            payload,
        },
        mutations: vec![ExtensionMutation::Put {
            id: PLAN_EXTENSION.into(),
            expected_revision: Some(saved_plan.revision),
            payload: encode_plan_state(machine.state())?,
        }],
        continuation: Some(ThreadInput {
            id: format!("interaction:{}:continuation", current.request.id),
            payload: continuation_payload,
            context: vec![ContextContent::Text {
                text: prompt.question.question.into(),
            }],
        }),
    })
}

impl StudioThreadAssembler {
    /// Commits a Plan answer, its product state and the exact follow-up input, then resumes execution.
    ///
    /// # Errors
    /// Rejects missing owners, stale or incompatible Plan state and invalid confirmation answers.
    pub async fn resolve_plan_confirmation(
        &self,
        thread_id: &str,
        mut resolution: ResolveUserInput,
    ) -> Result<InteractionRequest, PlanInteractionError> {
        let (owner, local_id) = super::decode_interaction_key(&resolution.interaction_id)?;
        if owner != thread_id {
            return Err(PlanInteractionError::Identity);
        }
        resolution.interaction_id = local_id.to_owned();
        let (thread, execution) = {
            self.0
                .state()
                .entries
                .get(thread_id)
                .map(|entry| (entry.thread.clone(), entry.execution))
        }
        .ok_or(PlanInteractionError::Identity)?;
        let snapshot = thread.snapshot();
        let current = snapshot
            .interactions
            .get(&resolution.interaction_id)
            .ok_or(PlanInteractionError::Identity)?;
        if let InteractionState::Resolved(existing) = &current.state {
            if resolution.expected_revision.checked_add(1) == Some(current.revision)
                && existing.payload == answer_payload(&resolution)?
            {
                return project_plan_confirmation(thread_id, current);
            }
            return Err(PlanInteractionError::Identity);
        }
        let saved_plan = snapshot
            .extensions
            .get(PLAN_EXTENSION)
            .ok_or(PlanInteractionError::Identity)?;
        let prepared = prepare_resolution(thread_id, current, saved_plan, &resolution)?;
        let committed = thread.resolve_interaction(prepared).await?;
        thread.resume_inputs(execution).await?;
        project_plan_confirmation(thread_id, &committed)
    }
}
