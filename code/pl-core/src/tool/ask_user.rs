use std::collections::HashSet;
use std::path::PathBuf;

use crate::turn::ToolEffect;
use futures::FutureExt;
use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionResolution,
    InteractionScope, InteractionStatus, PureError, UserInputRequest, UserInputResponse,
    UserQuestion, UserQuestionOption,
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::truncation::OutputTruncation;
use super::{
    BoxFuture, FunctionToolDefinition, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent,
    deserialize_tool_input,
};
use crate::turn::UserInputMode;

#[derive(Debug, Default)]
pub struct AskUserTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskUserInput {
    /// Structured questions shown to the user.
    #[schemars(length(min = 1))]
    questions: Vec<UserQuestionInput>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserQuestionInput {
    /// Stable snake_case id used as the answer map key.
    id: String,
    /// Short label for the question.
    header: String,
    /// Question shown to the user.
    question: String,
    /// Whether a free-form custom answer should be accepted.
    #[serde(default)]
    is_other: bool,
    /// Whether the answer is sensitive and should be hidden in UI logs.
    #[serde(default)]
    is_secret: bool,
    /// Optional predefined choices.
    options: Option<Vec<UserQuestionOptionInput>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UserQuestionOptionInput {
    /// Choice label shown to the user.
    label: String,
    /// Short explanation of the choice.
    description: String,
}

impl From<UserQuestionInput> for UserQuestion {
    fn from(question: UserQuestionInput) -> Self {
        Self {
            id: question.id,
            header: question.header,
            question: question.question,
            is_other: question.is_other,
            is_secret: question.is_secret,
            options: question.options.map(|options| {
                options
                    .into_iter()
                    .map(|option| UserQuestionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect()
            }),
        }
    }
}

impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "request_user_input"
    }

    fn description(&self) -> &str {
        "Ask the user for missing information while the current turn is running. \
         Supports multiple structured questions with optional choices and free-form answers."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<AskUserInput>::new(self.name(), self.description()).input_schema()
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        async move {
            let args = deserialize_tool_input::<AskUserInput>(self.name(), input.arguments)?;
            let questions = args
                .questions
                .into_iter()
                .map(UserQuestion::from)
                .collect::<Vec<_>>();
            validate_questions(&questions)?;
            let request_id = namespaced_request_id(&input.session_id, &input.tool_id);
            let request = UserInputRequest {
                request_id,
                tool_id: input.tool_id,
                questions,
            };
            let interaction = user_input_interaction(&input.session_id, &request, &context);
            let (response, runtime_events) = match context.options.user_input_mode {
                UserInputMode::AwaitResponse => {
                    let Some(callback) = context.options.interaction_callback.clone() else {
                        return Err(PureError::ToolExecutionFailed {
                            tool: self.name().to_string(),
                            error: "interaction runtime is not configured".to_string(),
                        });
                    };
                    let resolution = match context.options.cancellation_token.clone() {
                        Some(token) => {
                            tokio::select! {
                                resolution = callback(interaction.clone()) => resolution,
                                _ = token.cancelled() => InteractionResolution::UserInput {
                                    answers: Default::default(),
                                },
                            }
                        }
                        None => callback(interaction.clone()).await,
                    };
                    let response = match resolution {
                        InteractionResolution::UserInput { answers } => {
                            UserInputResponse { answers }
                        }
                        InteractionResolution::ToolApproval { .. }
                        | InteractionResolution::PlanConfirmation { .. } => {
                            UserInputResponse::default()
                        }
                    };
                    (response, Vec::new())
                }
                UserInputMode::EmitAndEndTurn => (
                    UserInputResponse::default(),
                    vec![
                        ToolRuntimeEvent::InteractionRequested {
                            interaction: Box::new(interaction),
                        },
                        ToolRuntimeEvent::EndTurn,
                    ],
                ),
            };
            let description = serde_json::to_string(&response).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("failed to serialize response: {error}"),
                }
            })?;
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events,
            })
        }
        .boxed()
    }
}

fn validate_questions(questions: &[UserQuestion]) -> Result<(), PureError> {
    if questions.is_empty() {
        return Err(PureError::ToolExecutionFailed {
            tool: "request_user_input".to_string(),
            error: "questions must not be empty".to_string(),
        });
    }
    let mut ids = HashSet::new();
    for question in questions {
        let id = question.id.trim();
        if id.is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: "request_user_input".to_string(),
                error: "question id must not be empty".to_string(),
            });
        }
        if !ids.insert(id.to_string()) {
            return Err(PureError::ToolExecutionFailed {
                tool: "request_user_input".to_string(),
                error: format!("duplicate question id: {id}"),
            });
        }
        if question.question.trim().is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: "request_user_input".to_string(),
                error: format!("question text must not be empty for id: {id}"),
            });
        }
    }
    Ok(())
}

fn namespaced_request_id(session_id: &str, tool_id: &str) -> String {
    if tool_id.starts_with(session_id) {
        tool_id.to_string()
    } else {
        format!("{session_id}-{tool_id}")
    }
}

fn user_input_interaction(
    turn_id: &str,
    request: &UserInputRequest,
    context: &ToolContext,
) -> InteractionRequest {
    let now = unix_seconds();
    InteractionRequest {
        interaction_id: request.request_id.clone(),
        kind: InteractionKind::UserInput,
        status: InteractionStatus::Pending,
        scope: InteractionScope {
            thread_id: String::new(),
            turn_id: turn_id.to_string(),
            item_id: Some(request.tool_id.clone()),
            tool_id: Some(request.tool_id.clone()),
            agent_path: context
                .active_subagent
                .as_ref()
                .and_then(|subagent| subagent.agent_path.clone()),
        },
        payload: InteractionPayload::UserInput {
            questions: request.questions.clone(),
        },
        created_at: now,
        updated_at: now,
        resolved_at: None,
        resolution: None,
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use pl_protocol::{UserInputAnswer, UserQuestionOption};
    use pretty_assertions::assert_eq;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::TurnOptions;
    use crate::tool::WorkspaceAccess;

    fn context(options: TurnOptions) -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options,
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        }
    }

    fn tool_input() -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({
                "questions": [{
                    "id": "mode",
                    "header": "Mode",
                    "question": "Which mode?",
                    "options": [{
                        "label": "Fast",
                        "description": "Use the fast path."
                    }]
                }]
            }),
            session_id: "session-1".to_string(),
            tool_id: "call-1".to_string(),
            revision_base: 0,
        }
    }

    #[tokio::test]
    async fn request_user_input_returns_answers_from_interaction_callback() {
        let seen_interaction = Arc::new(Mutex::new(None));
        let seen_interaction_for_callback = seen_interaction.clone();
        let callback: crate::InteractionCallback = Arc::new(move |interaction| {
            let seen_interaction = seen_interaction_for_callback.clone();
            async move {
                *seen_interaction.lock().unwrap() = Some(interaction);
                InteractionResolution::UserInput {
                    answers: HashMap::from([(
                        "mode".to_string(),
                        UserInputAnswer {
                            answers: vec!["Fast".to_string()],
                        },
                    )]),
                }
            }
            .boxed()
        });
        let output = AskUserTool
            .execute(
                tool_input(),
                context(TurnOptions::default().with_interaction_callback(callback)),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.description).unwrap(),
            UserInputResponse {
                answers: HashMap::from([(
                    "mode".to_string(),
                    UserInputAnswer {
                        answers: vec!["Fast".to_string()],
                    },
                )]),
            }
        );
        let interaction = seen_interaction.lock().unwrap().clone().unwrap();
        assert_eq!(interaction.interaction_id, "session-1-call-1");
        assert_eq!(interaction.kind, InteractionKind::UserInput);
        assert_eq!(interaction.status, InteractionStatus::Pending);
        assert_eq!(
            interaction.scope,
            InteractionScope {
                thread_id: String::new(),
                turn_id: "session-1".to_string(),
                item_id: Some("call-1".to_string()),
                tool_id: Some("call-1".to_string()),
                agent_path: None,
            }
        );
        assert_eq!(
            interaction.payload,
            InteractionPayload::UserInput {
                questions: vec![UserQuestion {
                    id: "mode".to_string(),
                    header: "Mode".to_string(),
                    question: "Which mode?".to_string(),
                    is_other: false,
                    is_secret: false,
                    options: Some(vec![UserQuestionOption {
                        label: "Fast".to_string(),
                        description: "Use the fast path.".to_string(),
                    }]),
                }],
            }
        );
        assert!(output.runtime_events.is_empty());
    }

    #[tokio::test]
    async fn request_user_input_returns_empty_answers_when_cancelled() {
        let callback: crate::InteractionCallback = Arc::new(|_interaction| {
            async {
                std::future::pending::<()>().await;
                InteractionResolution::UserInput {
                    answers: Default::default(),
                }
            }
            .boxed()
        });
        let token = CancellationToken::new();
        token.cancel();
        let output = AskUserTool
            .execute(
                tool_input(),
                context(
                    TurnOptions::default()
                        .with_interaction_callback(callback)
                        .with_cancellation(token),
                ),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.description).unwrap(),
            UserInputResponse::default()
        );
    }

    #[tokio::test]
    async fn request_user_input_can_end_current_turn_after_request() {
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            AskUserTool.execute(
                tool_input(),
                context(TurnOptions::default().with_user_input_end_turn()),
            ),
        )
        .await
        .expect("tool should not wait for user resolution")
        .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.description).unwrap(),
            UserInputResponse::default()
        );
        assert_eq!(output.runtime_events.len(), 2);
        let crate::tool::ToolRuntimeEvent::InteractionRequested { interaction } =
            &output.runtime_events[0]
        else {
            panic!("first runtime event must persist the interaction");
        };
        assert_eq!(interaction.interaction_id, "session-1-call-1");
        assert_eq!(interaction.kind, InteractionKind::UserInput);
        assert_eq!(
            output.runtime_events[1],
            crate::tool::ToolRuntimeEvent::EndTurn
        );
    }

    #[test]
    fn rejects_duplicate_question_ids() {
        let questions = vec![
            UserQuestion {
                id: "mode".to_string(),
                header: "Mode".to_string(),
                question: "Which mode?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            },
            UserQuestion {
                id: "mode".to_string(),
                header: "Mode".to_string(),
                question: "Which mode?".to_string(),
                is_other: false,
                is_secret: false,
                options: None,
            },
        ];

        assert!(validate_questions(&questions).is_err());
    }
}
