use std::future::Future;

use pl_protocol::{PureError, UserQuestion, UserQuestionOption};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ask_user::execute_user_input;
use super::{StaticTool, ToolBatchPolicy, ToolCallContext, ToolPolicy, ToolResult};
use crate::turn::ToolEffect;

#[derive(Debug, Default)]
pub struct SubmitPlanTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitPlanInput {
    /// The complete final plan in Markdown.
    plan: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SubmitPlanResult {
    status: String,
    message: String,
}

impl StaticTool for SubmitPlanTool {
    type Input = SubmitPlanInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("submit_plan"),
            "Submit the final Markdown plan for user confirmation. Use only after the plan is \
             complete; this tool does not execute the plan. Use request_user_input instead when \
             information is missing or clarification is needed.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::Read)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        args: SubmitPlanInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            if !has_markdown_level_one_heading(args.plan.trim()) {
                return Err(PureError::ToolExecutionFailed {
                    tool: "submit_plan".to_string(),
                    error: "plan must start with a level-one Markdown heading".to_string(),
                });
            }
            let submitted = serde_json::to_string(&SubmitPlanResult {
                status: "submitted".to_string(),
                message: "Plan submitted for user confirmation. Return a brief final acknowledgement and stop."
                    .to_string(),
            })
            .map_err(|error| PureError::ToolExecutionFailed {
                tool: "submit_plan".to_string(),
                error: format!("failed to serialize submission result: {error}"),
            })?;
            execute_user_input(
                "submit_plan",
                vec![UserQuestion {
                    id: "plan_confirmation".to_string(),
                    header: "Plan".to_string(),
                    question: args.plan,
                    is_other: true,
                    is_secret: false,
                    options: Some(vec![
                        UserQuestionOption {
                            label: "Approve".to_string(),
                            description: "Proceed with this plan.".to_string(),
                        },
                        UserQuestionOption {
                            label: "Revise".to_string(),
                            description:
                                "Return to planning and incorporate the requested changes."
                                    .to_string(),
                        },
                    ]),
                }],
                context,
                Some(submitted),
            )
            .await
        }
    }
}

fn has_markdown_level_one_heading(plan: &str) -> bool {
    let mut characters = plan.chars();
    characters.next() == Some('#')
        && characters.next().is_some_and(char::is_whitespace)
        && characters.any(|character| !character.is_whitespace())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::TurnOptions;
    use crate::tool::{
        StaticToolTestExt, ToolApprovalContext, ToolDirective, ToolInput, WorkspaceAccess,
    };

    fn context() -> ToolCallContext {
        let options = TurnOptions::default().with_user_input_end_turn();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let approval =
            ToolApprovalContext::new(options.permission_mode, WorkspaceAccess::WorkspaceOnly)
                .with_interaction(options.interaction_callback, options.user_input_mode);
        ToolCallContext::test(event_tx).with_approval(approval)
    }

    #[tokio::test]
    async fn reuses_the_markdown_submission_contract_with_generic_interaction() {
        let plan = "  # Plan\n\n1. Inspect\n2. Implement\n3. Verify\n  ";
        let output = SubmitPlanTool
            .execute_raw(
                ToolInput {
                    arguments: serde_json::json!({"plan": plan}),
                },
                context(),
            )
            .await
            .unwrap();
        let ToolDirective::InteractionRequested { interaction } = &output.runtime_events[0] else {
            panic!("submit_plan must persist an interaction");
        };
        let pl_protocol::InteractionContent::UserInput(user_input) = &interaction.content else {
            panic!("submit_plan must emit generic user input");
        };

        assert_eq!(
            (
                user_input.questions().to_vec(),
                serde_json::from_str::<SubmitPlanResult>(&output.canonical_output()).unwrap(),
                output.runtime_events.get(1),
            ),
            (
                vec![UserQuestion {
                    id: "plan_confirmation".to_string(),
                    header: "Plan".to_string(),
                    question: plan.to_string(),
                    is_other: true,
                    is_secret: false,
                    options: Some(vec![
                        UserQuestionOption {
                            label: "Approve".to_string(),
                            description: "Proceed with this plan.".to_string(),
                        },
                        UserQuestionOption {
                            label: "Revise".to_string(),
                            description: "Return to planning and incorporate the requested changes."
                                .to_string(),
                        },
                    ]),
                }],
                SubmitPlanResult {
                    status: "submitted".to_string(),
                    message: "Plan submitted for user confirmation. Return a brief final acknowledgement and stop."
                        .to_string(),
                },
                Some(&ToolDirective::EndTurn {
                    final_content: None,
                }),
            )
        );

        for arguments in [
            serde_json::json!({"plan": "1. Missing heading"}),
            serde_json::json!({"content": "# Legacy"}),
        ] {
            assert!(
                SubmitPlanTool
                    .execute_raw(ToolInput { arguments }, context())
                    .await
                    .is_err()
            );
        }
    }
}
