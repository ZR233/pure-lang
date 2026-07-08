use std::collections::HashSet;
use std::path::PathBuf;

use pl_protocol::{
    InteractionKind, InteractionPayload, InteractionRequest, InteractionResolution,
    InteractionScope, InteractionStatus, PureError, UserInputRequest, UserInputResponse,
    UserQuestion,
};
use serde::Deserialize;

use super::truncation::OutputTruncation;
use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent};
use crate::turn::UserInputMode;

#[derive(Debug, Default)]
pub struct AskUserTool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AskUserInput {
    questions: Vec<UserQuestion>,
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
        serde_json::json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Stable snake_case id used as the answer map key."
                            },
                            "header": {
                                "type": "string",
                                "description": "Short label for the question."
                            },
                            "question": {
                                "type": "string",
                                "description": "Question shown to the user."
                            },
                            "isOther": {
                                "type": "boolean",
                                "description": "Whether a free-form custom answer should be accepted."
                            },
                            "isSecret": {
                                "type": "boolean",
                                "description": "Whether the answer is sensitive and should be hidden in UI logs."
                            },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    },
                                    "required": ["label", "description"],
                                    "additionalProperties": false
                                }
                            }
                        },
                        "required": ["id", "header", "question"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["questions"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: AskUserInput = serde_json::from_value(input.arguments).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("invalid input: {error}"),
                }
            })?;
            validate_questions(&args.questions)?;
            let request_id = namespaced_request_id(&input.session_id, &input.tool_id);
            let request = UserInputRequest {
                request_id,
                tool_id: input.tool_id,
                questions: args.questions,
            };
            let Some(callback) = context.options.interaction_callback.clone() else {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: "interaction runtime is not configured".to_string(),
                });
            };
            let interaction = user_input_interaction(&input.session_id, &request, &context);
            let (response, runtime_events) = match context.options.user_input_mode {
                UserInputMode::AwaitResponse => {
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
                UserInputMode::EmitAndEndTurn => {
                    tokio::spawn(async move {
                        let _ = callback(interaction).await;
                    });
                    (
                        UserInputResponse::default(),
                        vec![ToolRuntimeEvent::EndTurn],
                    )
                }
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
        })
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
            session_id: String::new(),
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
    use crate::tool::WorkspaceAccess;
    use crate::{AgentSupervisor, TurnOptions};

    fn context(options: TurnOptions) -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options,
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: AgentSupervisor::default(),
            agent_tool_registrar: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::CoreSession::new()),
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
            Box::pin(async move {
                *seen_interaction.lock().unwrap() = Some(interaction);
                InteractionResolution::UserInput {
                    answers: HashMap::from([(
                        "mode".to_string(),
                        UserInputAnswer {
                            answers: vec!["Fast".to_string()],
                        },
                    )]),
                }
            })
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
                session_id: String::new(),
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
    }

    #[tokio::test]
    async fn request_user_input_returns_empty_answers_when_cancelled() {
        let callback: crate::InteractionCallback = Arc::new(|_interaction| {
            Box::pin(async {
                std::future::pending::<()>().await;
                InteractionResolution::UserInput {
                    answers: Default::default(),
                }
            })
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
        let callback: crate::InteractionCallback = Arc::new(|_interaction| {
            Box::pin(async {
                std::future::pending::<()>().await;
                InteractionResolution::UserInput {
                    answers: Default::default(),
                }
            })
        });
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            AskUserTool.execute(
                tool_input(),
                context(
                    TurnOptions::default()
                        .with_interaction_callback(callback)
                        .with_user_input_end_turn(),
                ),
            ),
        )
        .await
        .expect("tool should not wait for user resolution")
        .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.description).unwrap(),
            UserInputResponse::default()
        );
        assert_eq!(
            output.runtime_events,
            vec![crate::tool::ToolRuntimeEvent::EndTurn]
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
