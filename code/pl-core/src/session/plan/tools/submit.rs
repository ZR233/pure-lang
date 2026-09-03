use std::future::Future;

use pl_protocol::{
    AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID, AgentSessionPlanConfirmationPurpose,
    InteractionContinuationPreset, InteractionPurpose, PureError, UserQuestion, UserQuestionOption,
};
use schemars::JsonSchema;
use serde::Deserialize;

use super::common::{AgentSessionPlanToolBinding, AgentSessionPlanToolRuntime, operation_id};
use crate::session::plan::{
    AgentSessionPlanResolveCommand, AgentSessionPlanSubmitCommand, confirmation_decision,
    validate_plan,
};
use crate::{
    StaticTool, StaticToolDefinition, ToolBatchPolicy, ToolCallContext, ToolEffect, ToolName,
    ToolPolicy, ToolResult, TurnWorkingSetHandle, build_user_input_interaction,
    execute_user_input_interaction,
};

pub const TOOL_PLAN_SUBMIT: &str = "plan_submit";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSubmitInput {
    /// CAS revision returned by plan_current.
    expected_revision: u64,
    /// Complete replacement Plan beginning with a level-one Markdown heading.
    plan: String,
}

#[derive(Debug, Clone)]
pub struct PlanSubmitTool(AgentSessionPlanToolRuntime);

impl PlanSubmitTool {
    pub(crate) fn new(
        working_set: TurnWorkingSetHandle,
        binding: AgentSessionPlanToolBinding,
    ) -> Self {
        Self(AgentSessionPlanToolRuntime::new(working_set, binding))
    }
}

impl StaticTool for PlanSubmitTool {
    type Input = PlanSubmitInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin(TOOL_PLAN_SUBMIT),
            "Submit a complete Markdown Plan through the fixed Plan state machine for user approval or revision. This is the only tool for asking the user to approve implementation of a complete Plan; do not first ask whether to implement, proceed, or approve through request_user_input or final text. Requires plan revision CAS, and this must be the only tool call in the response.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default()
            .with_effect(ToolEffect::AgentControl)
            .with_batch_policy(ToolBatchPolicy::Solo)
    }

    fn execute(
        &self,
        args: PlanSubmitInput,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            validate_plan(&args.plan).map_err(|error| PureError::ToolExecutionFailed {
                tool: TOOL_PLAN_SUBMIT.to_string(),
                error,
            })?;

            let previous = self.0.working_state();
            let mut interaction = build_user_input_interaction(
                TOOL_PLAN_SUBMIT,
                vec![confirmation_question(args.plan.clone())],
                &context,
                InteractionPurpose::General,
            )?;
            interaction.continuation = Some(InteractionContinuationPreset::question(
                AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID,
                self.0.submitted_plan_presentation(),
            ));
            let operation_id = operation_id(context.identity());
            let argument_hash = crate::canonical_json_hash(&serde_json::json!({
                "expectedRevision": args.expected_revision,
                "plan": args.plan,
                "interactionId": interaction.interaction_id,
                "continuation": interaction.continuation,
            }));
            let plan_hash = crate::canonical_content_hash(args.plan.as_bytes());
            interaction.scope.purpose = InteractionPurpose::AgentSessionPlanConfirmation(
                AgentSessionPlanConfirmationPurpose {
                    expected_revision: args.expected_revision,
                    operation_id: operation_id.clone(),
                    argument_hash: argument_hash.clone(),
                    plan_hash,
                },
            );

            let submitted = self.0.mutate(|machine| {
                let response = machine.submit(AgentSessionPlanSubmitCommand {
                    expected_revision: args.expected_revision,
                    plan: args.plan,
                    interaction_id: interaction.interaction_id.clone(),
                    operation_id,
                    argument_hash,
                    submitted_at: interaction.created_at,
                });
                let accepted = response.accepted;
                (response, accepted)
            })?;
            if !submitted.accepted {
                return ToolResult::json(submitted);
            }

            let pending_output = serde_json::to_string(&submitted).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: TOOL_PLAN_SUBMIT.to_string(),
                    error: format!("failed to serialize Plan submission: {error}"),
                }
            })?;
            let execution = match execute_user_input_interaction(
                TOOL_PLAN_SUBMIT,
                interaction.clone(),
                context,
                Some(pending_output),
            )
            .await
            {
                Ok(execution) => execution,
                Err(error) => {
                    self.0.restore(previous.clone())?;
                    return Err(error);
                }
            };
            let Some(resolution) = execution.resolution else {
                return Ok(execution.result);
            };

            let decision = match confirmation_decision(&resolution) {
                Ok(decision) => decision,
                Err(error) => {
                    self.0.restore(previous)?;
                    return Err(PureError::ToolExecutionFailed {
                        tool: TOOL_PLAN_SUBMIT.to_string(),
                        error,
                    });
                }
            };
            let resolution_hash =
                crate::canonical_json_hash(&serde_json::to_value(&resolution).map_err(
                    |error| PureError::ToolExecutionFailed {
                        tool: TOOL_PLAN_SUBMIT.to_string(),
                        error: format!("failed to hash Plan confirmation: {error}"),
                    },
                )?);
            let resolved = self.0.mutate(|machine| {
                let response = machine.resolve(AgentSessionPlanResolveCommand {
                    expected_revision: submitted.snapshot.revision,
                    interaction_id: interaction.interaction_id.clone(),
                    operation_id: format!("plan-resolution:{}", interaction.interaction_id),
                    argument_hash: resolution_hash,
                    decision,
                    resolved_at: crate::time::unix_seconds(),
                });
                let accepted = response.accepted;
                (response, accepted)
            })?;
            ToolResult::json(resolved)
        }
    }
}

fn confirmation_question(plan: String) -> UserQuestion {
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::TurnOptions;
    use crate::tool::{
        StaticToolTestExt, ToolApprovalContext, ToolDirective, ToolInput, WorkspaceAccess,
    };

    fn context() -> ToolCallContext {
        context_with_call("call-1")
    }

    fn context_with_call(call_id: &str) -> ToolCallContext {
        let options = TurnOptions::default().with_user_input_end_turn();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let approval =
            ToolApprovalContext::new(options.permission_mode, WorkspaceAccess::WorkspaceOnly)
                .with_interaction(options.interaction_callback, options.user_input_mode);
        ToolCallContext::new(
            crate::ToolCallIdentity {
                call_id: call_id.to_string(),
                item_id: call_id.to_string(),
                agent_id: "agent-1".to_string(),
                agent_path: Some("/root".to_string()),
                agent_role: "root".to_string(),
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                ..crate::ToolCallIdentity::default()
            },
            event_tx,
        )
        .with_approval(approval)
    }

    fn input(revision: u64) -> ToolInput {
        ToolInput {
            arguments: serde_json::json!({
                "expectedRevision": revision,
                "plan": "# Plan\n\nImplement and verify."
            }),
        }
    }

    #[tokio::test]
    async fn submit_creates_typed_plan_purpose_and_updates_machine() {
        let working_set = TurnWorkingSetHandle::default();
        let output = PlanSubmitTool::new(
            working_set.clone(),
            AgentSessionPlanToolBinding::new(crate::AgentSessionPlanOptions::default()),
        )
        .execute_raw(input(0), context())
        .await
        .expect("submit succeeds");
        let ToolDirective::InteractionRequested { interaction } = &output.runtime_events[0] else {
            panic!("plan_submit must request a generic UserInput Interaction");
        };
        assert!(matches!(
            interaction.scope.purpose,
            InteractionPurpose::AgentSessionPlanConfirmation(_)
        ));
        assert_eq!(
            interaction.continuation,
            Some(InteractionContinuationPreset::question(
                AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID,
                pl_protocol::MessagePresentation::Hidden,
            ))
        );
        assert_eq!(
            working_set.plan().unwrap().state,
            pl_protocol::AgentSessionPlanPhase::AwaitingConfirmation
        );
        assert!(matches!(
            output.runtime_events.get(1),
            Some(ToolDirective::EndTurn { .. })
        ));
    }

    #[tokio::test]
    async fn repeated_submit_returns_complete_state_rejection() {
        let working_set = TurnWorkingSetHandle::default();
        let tool = PlanSubmitTool::new(
            working_set,
            AgentSessionPlanToolBinding::new(crate::AgentSessionPlanOptions::default()),
        );
        tool.execute_raw(input(0), context()).await.unwrap();
        let output = tool
            .execute_raw(input(1), context_with_call("call-2"))
            .await
            .unwrap();
        let response = serde_json::from_str::<pl_protocol::AgentSessionPlanMutationResponse>(
            &output.canonical_output(),
        )
        .unwrap();

        assert!(!response.accepted);
        assert_eq!(
            response.code,
            pl_protocol::AgentSessionPlanResultCode::InvalidState
        );
        assert_eq!(response.error.unwrap().allowed_transitions.len(), 2);
        assert!(
            !output
                .runtime_events
                .iter()
                .any(|event| matches!(event, ToolDirective::InteractionRequested { .. }))
        );
    }

    #[tokio::test]
    async fn submit_rejects_stale_revision_without_interaction() {
        let output = PlanSubmitTool::new(
            TurnWorkingSetHandle::default(),
            AgentSessionPlanToolBinding::new(crate::AgentSessionPlanOptions::default()),
        )
        .execute_raw(input(9), context())
        .await
        .unwrap();
        let response = serde_json::from_str::<pl_protocol::AgentSessionPlanMutationResponse>(
            &output.canonical_output(),
        )
        .unwrap();
        assert_eq!(
            response.code,
            pl_protocol::AgentSessionPlanResultCode::StaleRevision
        );
        assert!(
            response
                .error
                .unwrap()
                .message
                .contains("current revision is 0")
        );
        assert!(
            !output
                .runtime_events
                .iter()
                .any(|event| matches!(event, ToolDirective::InteractionRequested { .. }))
        );
    }

    #[tokio::test]
    async fn registration_preset_controls_plan_message_presentation() {
        let options = crate::AgentSessionPlanOptions::default()
            .with_submitted_plan_presentation(pl_protocol::MessagePresentation::Visible);
        let output = PlanSubmitTool::new(
            TurnWorkingSetHandle::default(),
            AgentSessionPlanToolBinding::new(options),
        )
        .execute_raw(input(0), context())
        .await
        .unwrap();
        let ToolDirective::InteractionRequested { interaction } = &output.runtime_events[0] else {
            panic!("plan_submit must request confirmation");
        };
        assert_eq!(
            interaction.continuation,
            Some(InteractionContinuationPreset::question(
                AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID,
                pl_protocol::MessagePresentation::Visible,
            ))
        );
    }
}
