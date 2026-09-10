use pl_protocol::{AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID, UserQuestion, UserQuestionOption};
use schemars::JsonSchema;
use serde::Deserialize;
pub const TOOL_PLAN_SUBMIT: &str = "plan_submit";
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSubmitInput {
    /// CAS revision returned by plan_current.
    pub(super) expected_revision: u64,
    /// Complete replacement Plan beginning with a level-one Markdown heading.
    pub(super) plan: String,
}

pub(super) fn confirmation_question(plan: String) -> UserQuestion {
    UserQuestion {
        id: AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID.to_string(),
        header: "Plan".to_string(),
        question: plan,
        is_other: true,
        is_secret: false,
        options: Some(vec![
            UserQuestionOption {
                label: "Approve".to_string(),
                description: "Approve this exact Plan and allow the task to proceed.".to_string(),
            },
            UserQuestionOption {
                label: "Revise".to_string(),
                description: "Request a revised Plan and optionally describe the changes."
                    .to_string(),
            },
        ]),
    }
}
