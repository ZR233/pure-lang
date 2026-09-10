//! User-input tool schema and model-facing result projection.

use pl_protocol::{UserQuestion, UserQuestionOption};
use schemars::JsonSchema;
use serde::Deserialize;

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

/// Registers ask-user for the protocol-independent Thread interaction lifecycle.
///
/// # Errors
/// Propagates invalid registration identity.
pub fn registration(
    declaration: pl_core::context::OpaquePayload,
) -> Result<pl_core::tool::opaque::Registration, pl_core::tool::opaque::RegistryError> {
    Ok(pl_core::tool::opaque::Registration::new(
        "request_user_input".into(),
        declaration,
        AskUserTool,
    )?
    .with_interactions())
}

impl pl_core::tool::opaque::Tool for AskUserTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        _: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let input: AskUserInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        let mut ids = std::collections::BTreeSet::new();
        if input.questions.is_empty()
            || input.questions.iter().any(|question| {
                question.id.trim().is_empty()
                    || question.question.trim().is_empty()
                    || !ids.insert(&question.id)
            })
        {
            return Err(pl_core::tool::opaque::ToolError::new(crate::tool_error(
                "request_user_input",
                "questions require unique nonempty IDs and question text",
            )));
        }
        let questions = input
            .questions
            .into_iter()
            .map(UserQuestion::from)
            .collect::<Vec<_>>();
        let encoded =
            serde_json::to_string(&questions).map_err(pl_core::tool::opaque::ToolError::new)?;
        let payload = pl_core::context::OpaquePayload::new("pl.tool.user-input", 1, encoded)
            .map_err(pl_core::tool::opaque::ToolError::new)?;
        Ok(pl_core::tool::ToolOutput::new(
            payload.clone(),
            vec![pl_core::context::ContextContent::Text {
                text: std::sync::Arc::from("Waiting for the user's response."),
            }],
        )
        .with_interaction(payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{input, thread_context};
    use pl_core::tool::{ToolControl, opaque::Tool};
    use pretty_assertions::assert_eq;
    #[tokio::test]
    async fn questions_preserve_secret_metadata_and_request_framework_interaction() {
        let questions = serde_json::json!([{"id":"token","header":"Token","question":"Provide token","isSecret":true,"isOther":true}]);
        let output = AskUserTool
            .execute(
                input(serde_json::json!({"questions":questions})),
                thread_context(),
            )
            .await
            .unwrap();
        assert_eq!(output.control(), ToolControl::AwaitInteraction);
        let request: serde_json::Value =
            serde_json::from_str(output.interaction().unwrap().content()).unwrap();
        assert_eq!(request, questions);
    }
    #[tokio::test]
    async fn rejects_empty_or_duplicate_question_ids_before_creating_interaction() {
        for questions in [
            serde_json::json!([]),
            serde_json::json!([{"id":"a","header":"A","question":"First"},{"id":"a","header":"B","question":"Second"}]),
        ] {
            let error = AskUserTool
                .execute(
                    input(serde_json::json!({"questions":questions})),
                    thread_context(),
                )
                .await
                .unwrap_err();
            assert!(!error.to_string().is_empty());
        }
    }
}
