use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;

use crate::time::unix_seconds;
use crate::turn::ToolEffect;
use pl_protocol::{
    InteractionRequest, InteractionResolution, InteractionScope, PureError, UserInputRequest,
    UserInputResolution, UserInputResponse, UserQuestion, UserQuestionOption,
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::truncation::OutputTruncation;
use super::{StaticTool, ToolCallContext, ToolDirective, ToolPolicy, ToolResult};
use crate::turn::UserInputMode;

#[derive(Debug, Default)]
pub struct AskUserTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskUserInput {
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

impl StaticTool for AskUserTool {
    type Input = AskUserInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("request_user_input"),
            "Ask the user for missing information while the current turn is running. \
             Supports multiple structured questions with optional choices and free-form answers.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::Read)
    }

    fn execute(
        &self,
        args: AskUserInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let questions = args
                .questions
                .into_iter()
                .map(UserQuestion::from)
                .collect::<Vec<_>>();
            execute_user_input("request_user_input", questions, context, None).await
        }
    }
}

pub(super) async fn execute_user_input(
    tool_name: &str,
    questions: Vec<UserQuestion>,
    context: ToolCallContext,
    pending_output: Option<String>,
) -> Result<ToolResult, PureError> {
    validate_questions(tool_name, &questions)?;
    let request_id =
        namespaced_request_id(&context.identity().turn_id, &context.identity().item_id);
    let request = UserInputRequest {
        request_id,
        tool_id: context.identity().item_id.clone(),
        questions,
    };
    let interaction = user_input_interaction(&context.identity().turn_id, &request, &context);
    let (response, runtime_events) = match context.approval().user_input_mode() {
        UserInputMode::AwaitResponse => {
            let Some(callback) = context.approval().interaction_callback() else {
                return Err(PureError::ToolExecutionFailed {
                    tool: tool_name.to_string(),
                    error: "interaction runtime is not configured".to_string(),
                });
            };
            let resolution = match context.cancellation_token() {
                Some(token) => {
                    tokio::select! {
                        resolution = callback(interaction.clone()) => resolution,
                        _ = token.cancelled() => InteractionResolution::UserInput(UserInputResolution {
                            answers: Default::default(),
                        }),
                    }
                }
                None => callback(interaction.clone()).await,
            };
            let response = match resolution {
                InteractionResolution::UserInput(value) => UserInputResponse {
                    answers: value.answers,
                },
                InteractionResolution::ToolApproval(_) => UserInputResponse::default(),
            };
            (response, Vec::new())
        }
        UserInputMode::EmitAndEndTurn => (
            UserInputResponse::default(),
            vec![
                ToolDirective::InteractionRequested {
                    interaction: Box::new(interaction),
                },
                ToolDirective::EndTurn {
                    final_content: None,
                },
            ],
        ),
    };
    let response_description =
        serde_json::to_string(&response).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("failed to serialize response: {error}"),
        })?;
    let description = if runtime_events.is_empty() {
        response_description
    } else {
        pending_output.unwrap_or(response_description)
    };
    Ok(ToolResult::from_runtime_text(
        description,
        OutputTruncation::empty(),
        PathBuf::new(),
        Some(0),
        false,
        runtime_events,
    ))
}

fn validate_questions(tool_name: &str, questions: &[UserQuestion]) -> Result<(), PureError> {
    if questions.is_empty() {
        return Err(PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: "questions must not be empty".to_string(),
        });
    }
    let mut ids = HashSet::new();
    for question in questions {
        let id = question.id.trim();
        if id.is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: tool_name.to_string(),
                error: "question id must not be empty".to_string(),
            });
        }
        if !ids.insert(id.to_string()) {
            return Err(PureError::ToolExecutionFailed {
                tool: tool_name.to_string(),
                error: format!("duplicate question id: {id}"),
            });
        }
        if question.question.trim().is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: tool_name.to_string(),
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
    context: &ToolCallContext,
) -> InteractionRequest {
    let now = unix_seconds();
    InteractionRequest::user_input(
        request.request_id.clone(),
        InteractionScope {
            thread_id: String::new(),
            turn_id: turn_id.to_string(),
            item_id: Some(request.tool_id.clone()),
            tool_id: Some(request.tool_id.clone()),
            agent_path: context.identity().agent_path.clone(),
        },
        request.questions.clone(),
        now,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use futures::FutureExt;
    use pl_protocol::{UserInputAnswer, UserQuestionOption};
    use pretty_assertions::assert_eq;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::TurnOptions;
    use crate::tool::{StaticToolTestExt, ToolApprovalContext, ToolInput, WorkspaceAccess};

    fn context(options: TurnOptions) -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let approval =
            ToolApprovalContext::new(options.permission_mode, WorkspaceAccess::WorkspaceOnly)
                .with_interaction(options.interaction_callback, options.user_input_mode);
        ToolCallContext::test(event_tx)
            .with_cancellation(options.cancellation_token)
            .with_approval(approval)
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
                InteractionResolution::UserInput(UserInputResolution {
                    answers: HashMap::from([(
                        "mode".to_string(),
                        UserInputAnswer {
                            answers: vec!["Fast".to_string()],
                        },
                    )]),
                })
            }
            .boxed()
        });
        let output = AskUserTool
            .execute_raw(
                tool_input(),
                context(TurnOptions::default().with_interaction_callback(callback)),
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.canonical_output()).unwrap(),
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
        assert_eq!(interaction.interaction_id, "turn-1-call-1");
        assert_eq!(interaction.kind(), pl_protocol::InteractionKind::UserInput);
        assert_eq!(
            interaction.status(),
            pl_protocol::InteractionStatus::Pending
        );
        assert_eq!(
            interaction.scope,
            InteractionScope {
                thread_id: String::new(),
                turn_id: "turn-1".to_string(),
                item_id: Some("call-1".to_string()),
                tool_id: Some("call-1".to_string()),
                agent_path: Some("/root".to_string()),
            }
        );
        let pl_protocol::InteractionContent::UserInput(user_input) = interaction.content else {
            panic!("interaction must be user input");
        };
        assert_eq!(
            user_input.questions(),
            &[UserQuestion {
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
        assert!(output.runtime_events.is_empty());
    }

    #[tokio::test]
    async fn request_user_input_returns_empty_answers_when_cancelled() {
        let callback: crate::InteractionCallback = Arc::new(|_interaction| {
            async {
                std::future::pending::<()>().await;
                InteractionResolution::UserInput(UserInputResolution {
                    answers: Default::default(),
                })
            }
            .boxed()
        });
        let token = CancellationToken::new();
        token.cancel();
        let output = AskUserTool
            .execute_raw(
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
            serde_json::from_str::<UserInputResponse>(&output.canonical_output()).unwrap(),
            UserInputResponse::default()
        );
    }

    #[tokio::test]
    async fn request_user_input_can_end_current_turn_after_request() {
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            AskUserTool.execute_raw(
                tool_input(),
                context(TurnOptions::default().with_user_input_end_turn()),
            ),
        )
        .await
        .expect("tool should not wait for user resolution")
        .unwrap();

        assert_eq!(
            serde_json::from_str::<UserInputResponse>(&output.canonical_output()).unwrap(),
            UserInputResponse::default()
        );
        assert_eq!(output.runtime_events.len(), 2);
        let crate::tool::ToolDirective::InteractionRequested { interaction } =
            &output.runtime_events[0]
        else {
            panic!("first runtime event must persist the interaction");
        };
        assert_eq!(interaction.interaction_id, "turn-1-call-1");
        assert_eq!(interaction.kind(), pl_protocol::InteractionKind::UserInput);
        assert_eq!(
            output.runtime_events[1],
            crate::tool::ToolDirective::EndTurn {
                final_content: None,
            }
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

        assert!(validate_questions("request_user_input", &questions).is_err());
    }
}
