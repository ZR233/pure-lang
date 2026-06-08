use std::collections::HashSet;
use std::path::PathBuf;

use pl_protocol::{AgentEvent, PureError, UserInputRequest, UserInputResponse, UserQuestion};
use serde::Deserialize;

use super::truncation::OutputTruncation;
use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

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
            let _ = context.event_tx.send(AgentEvent::UserInputRequested {
                request_id: request.request_id.clone(),
                tool_id: request.tool_id.clone(),
                questions: request.questions.clone(),
            });
            let Some(callback) = context.options.user_input_callback.clone() else {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: "user input callback is not configured".to_string(),
                });
            };
            let response = match context.options.cancellation_token.clone() {
                Some(token) => {
                    tokio::select! {
                        response = callback(request.clone()) => response,
                        _ = token.cancelled() => UserInputResponse::default(),
                    }
                }
                None => callback(request.clone()).await,
            };
            let _ = context.event_tx.send(AgentEvent::UserInputAnswered {
                request_id: request.request_id,
            });
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use pl_protocol::{UserInputAnswer, UserQuestionOption};
    use pretty_assertions::assert_eq;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::tool::WorkspaceAccess;
    use crate::{AgentControl, TurnOptions};

    fn context(options: TurnOptions) -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options,
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: AgentControl::default(),
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
        }
    }

    #[tokio::test]
    async fn request_user_input_returns_answers_from_callback() {
        let seen_request = Arc::new(Mutex::new(None));
        let seen_request_for_callback = seen_request.clone();
        let callback: crate::UserInputCallback = Arc::new(move |request| {
            let seen_request = seen_request_for_callback.clone();
            Box::pin(async move {
                *seen_request.lock().unwrap() = Some(request);
                UserInputResponse {
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
                context(TurnOptions::default().with_user_input_callback(callback)),
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
        let request = seen_request.lock().unwrap().clone().unwrap();
        assert_eq!(request.request_id, "session-1-call-1");
        assert_eq!(
            request.questions,
            vec![UserQuestion {
                id: "mode".to_string(),
                header: "Mode".to_string(),
                question: "Which mode?".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![UserQuestionOption {
                    label: "Fast".to_string(),
                    description: "Use the fast path.".to_string(),
                }]),
            }]
        );
    }

    #[tokio::test]
    async fn request_user_input_returns_empty_answers_when_cancelled() {
        let callback: crate::UserInputCallback = Arc::new(|_request| {
            Box::pin(async {
                std::future::pending::<()>().await;
                UserInputResponse::default()
            })
        });
        let token = CancellationToken::new();
        token.cancel();
        let output = AskUserTool
            .execute(
                tool_input(),
                context(
                    TurnOptions::default()
                        .with_user_input_callback(callback)
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
