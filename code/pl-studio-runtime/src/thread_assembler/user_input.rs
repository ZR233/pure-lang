//! Ordinary question projection and atomic user-answer continuation.
use super::StudioThreadAssembler;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{
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
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum UserInteractionError {
    #[error(transparent)]
    Key(#[from] super::InteractionKeyError),
    #[error("unsupported user interaction payload {format} version {version}")]
    Unsupported { format: String, version: u32 },
    #[error("user interaction payload is invalid")]
    Decode(#[from] serde_json::Error),
    #[error("user interaction identity or revision does not match")]
    Identity,
    #[error("user answer validation failed")]
    Projection(#[from] pl_protocol::InteractionTransitionError),
    #[error("user answer could not be frozen")]
    Payload(#[from] pl_core::context::PayloadError),
    #[error("Thread rejected user answer")]
    Thread(#[from] pl_core::thread::ThreadError),
}
fn decode<T: serde::de::DeserializeOwned>(
    payload: &OpaquePayload,
    format: &str,
) -> Result<T, UserInteractionError> {
    if payload.format() != format || payload.version() != 1 {
        return Err(UserInteractionError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        });
    }
    Ok(serde_json::from_str(payload.content())?)
}

/// Projects the original tool questions and saved user response into the existing product DTO.
///
/// # Errors
/// Rejects unknown encodings, invalid responses and contradictory record revisions.
pub fn project_user_input(
    thread_id: &str,
    record: &InteractionRecord,
) -> Result<InteractionRequest, UserInteractionError> {
    let questions: Vec<pl_protocol::UserQuestion> =
        decode(&record.request.payload, "pl.tool.user-input")?;
    let wire_id = super::interaction_key::encode(thread_id, &record.request.id);
    let mut projected = InteractionRequest::user_input(
        wire_id.clone(),
        InteractionScope {
            thread_id: thread_id.into(),
            turn_id: record.request.turn_id.clone(),
            item_id: None,
            tool_id: None,
            agent_path: Some(thread_id.into()),
            purpose: Default::default(),
        },
        questions,
        record.created_at,
    )
    .with_continuation(pl_protocol::InteractionContinuationPreset::resolution(
        pl_protocol::MessagePresentation::Hidden,
    ));
    projected.revision = 1;
    let command = match &record.state {
        InteractionState::Pending => None,
        InteractionState::Resolved(response) => {
            let response: UserInputResolution = decode(&response.payload, "pl.studio.user-answer")?;
            Some(InteractionCommand::ResolveUserInput(ResolveUserInput {
                interaction_id: wire_id.clone(),
                expected_revision: 1,
                operation_id: format!("resolve:{wire_id}"),
                resolved_at: record.updated_at,
                answers: response.answers,
            }))
        }
        InteractionState::Cancelled => {
            Some(InteractionCommand::Cancel(pl_protocol::CancelInteraction {
                interaction_id: wire_id.clone(),
                expected_revision: 1,
                operation_id: format!("cancel:{wire_id}"),
                reason: "User input was cancelled.".into(),
                cancelled_at: record.updated_at,
            }))
        }
    };
    if let Some(command) = command {
        let decision = projected.decide(command)?;
        projected.apply(decision, record.updated_at);
    }
    if projected.revision != record.revision {
        return Err(UserInteractionError::Identity);
    }
    Ok(projected)
}

fn prepare_resolution(
    thread_id: &str,
    record: &InteractionRecord,
    resolution: ResolveUserInput,
) -> Result<CoreResolution, UserInteractionError> {
    if record.request.id != resolution.interaction_id
        || record.revision != resolution.expected_revision
        || !matches!(record.state, InteractionState::Pending)
    {
        return Err(UserInteractionError::Identity);
    }
    let projected = project_user_input(thread_id, record)?;
    let mut validation = resolution.clone();
    validation.interaction_id = projected.interaction_id.clone();
    projected.decide(InteractionCommand::ResolveUserInput(validation))?;
    let answers = resolution
        .answers
        .iter()
        .map(|(id, answer)| (id.as_str(), answer))
        .collect::<BTreeMap<_, _>>();
    let encoded = serde_json::to_string(&serde_json::json!({"answers":answers}))?;
    let payload = OpaquePayload::new("pl.studio.user-answer", 1, encoded.clone())?;
    let continuation_metadata = OpaquePayload::new(
        "pl.studio.interaction-continuation",
        1,
        serde_json::to_string(
            &serde_json::json!({"interactionId":record.request.id,"presentation":pl_protocol::MessagePresentation::Hidden}),
        )?,
    )?;
    Ok(CoreResolution {
        id: record.request.id.clone(),
        expected_revision: record.revision,
        mutations: Vec::new(),
        response: InteractionResponse {
            payload,
            context: vec![ContextContent::Text {
                text: "User response received.".into(),
            }],
        },
        continuation: Some(ThreadInput {
            id: format!("interaction:{}:continuation", record.request.id),
            payload: continuation_metadata,
            context: vec![ContextContent::Text {
                text: encoded.into(),
            }],
        }),
    })
}

impl StudioThreadAssembler {
    /// Validates an answer, commits it with its continuation, then resumes this Thread.
    ///
    /// # Errors
    /// Rejects unknown owners, stale questions, invalid answers and failed core admission.
    pub async fn resolve_user_input(
        &self,
        mut resolution: ResolveUserInput,
    ) -> Result<InteractionRequest, UserInteractionError> {
        let (thread_id, local_id) = super::decode_interaction_key(&resolution.interaction_id)?;
        let thread_id = thread_id.to_owned();
        resolution.interaction_id = local_id.to_owned();
        let (thread, execution) = {
            self.0
                .state()
                .entries
                .get(&thread_id)
                .map(|entry| (entry.thread.clone(), entry.execution))
        }
        .ok_or(UserInteractionError::Identity)?;
        let snapshot = thread.snapshot();
        let current = snapshot
            .interactions
            .get(&resolution.interaction_id)
            .ok_or(UserInteractionError::Identity)?;
        if let InteractionState::Resolved(saved) = &current.state {
            let answer: UserInputResolution = decode(&saved.payload, "pl.studio.user-answer")?;
            if resolution.expected_revision.checked_add(1) == Some(current.revision)
                && answer.answers == resolution.answers
            {
                return project_user_input(&thread_id, current);
            }
            return Err(UserInteractionError::Identity);
        }
        let committed = thread
            .resolve_interaction(prepare_resolution(&thread_id, current, resolution)?)
            .await?;
        thread.resume_inputs(execution).await?;
        project_user_input(&thread_id, &committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn record() -> InteractionRecord {
        let questions = ["alpha", "beta"]
            .into_iter()
            .map(|id| pl_protocol::UserQuestion {
                id: id.into(),
                header: id.into(),
                question: format!("Question {id}?"),
                is_other: true,
                is_secret: id == "beta",
                options: None,
            })
            .collect::<Vec<_>>();
        InteractionRecord {
            created_at: 10,
            updated_at: 10,
            continuation_id: None,
            request: pl_core::thread::interactions::InteractionRequest {
                id: "question".into(),
                turn_id: "turn".into(),
                payload: OpaquePayload::new(
                    "pl.tool.user-input",
                    1,
                    serde_json::to_string(&questions).unwrap(),
                )
                .unwrap(),
            },
            revision: 1,
            state: InteractionState::Pending,
            extension_mutations: Vec::new(),
        }
    }

    #[test]
    fn user_answer_keeps_exact_text_and_uses_deterministic_hidden_continuation() {
        let current = record();
        let answer = |reverse: bool| {
            let entries = if reverse {
                vec![("beta", "  私有回答\r\n"), ("alpha", "first")]
            } else {
                vec![("alpha", "first"), ("beta", "  私有回答\r\n")]
            };
            ResolveUserInput {
                interaction_id: "question".into(),
                expected_revision: 1,
                operation_id: "resolve".into(),
                resolved_at: 12,
                answers: entries
                    .into_iter()
                    .map(|(id, value)| {
                        (
                            id.into(),
                            pl_protocol::UserInputAnswer {
                                answers: vec![value.into()],
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>(),
            }
        };
        let first = prepare_resolution("thread", &current, answer(false)).unwrap();
        let second = prepare_resolution("thread", &current, answer(true)).unwrap();
        assert_eq!(first.response, second.response);
        let continuation = first.continuation.unwrap();
        assert_eq!(Some(continuation.clone()), second.continuation);
        let [ContextContent::Text { text }] = continuation.context.as_slice() else {
            panic!("expected the exact answer text")
        };
        let decoded: UserInputResolution = serde_json::from_str(text).unwrap();
        assert_eq!(decoded.answers["beta"].answers, ["  私有回答\r\n"]);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(continuation.payload.content()).unwrap()["presentation"],
            "hidden"
        );
        assert!(first.mutations.is_empty());
        let projected = project_user_input("thread", &current).unwrap();
        assert_eq!(
            super::super::decode_interaction_key(&projected.interaction_id).unwrap(),
            ("thread", "question")
        );
    }
}
