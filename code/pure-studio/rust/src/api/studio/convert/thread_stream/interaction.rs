use anyhow::Result;
use pl_protocol::{
    CancelledInteractionState, ExpiredInteractionState, InteractionContent, InteractionRequest,
    ToolApprovalResolution, ToolApprovalState, UserInputState, UserQuestion,
};

use crate::api::studio::types::*;

pub(crate) fn interaction(value: InteractionRequest) -> Result<BridgeInteractionRequest> {
    Ok(BridgeInteractionRequest {
        interaction_id: value.interaction_id,
        scope: BridgeInteractionScope {
            thread_id: value.scope.thread_id,
            turn_id: value.scope.turn_id,
            item_id: value.scope.item_id,
            tool_id: value.scope.tool_id,
            agent_path: value.scope.agent_path,
        },
        revision: value.revision,
        content: content(value.content)?,
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}

fn content(value: InteractionContent) -> Result<BridgeInteractionContent> {
    Ok(match value {
        InteractionContent::UserInput(value) => BridgeInteractionContent::UserInput {
            questions: value.questions().iter().cloned().map(question).collect(),
            state: user_input_state(value.state()),
        },
        InteractionContent::ToolApproval(value) => {
            let request = value.request();
            BridgeInteractionContent::ToolApproval {
                name: request.name.clone(),
                arguments_json: serde_json::to_string(&request.arguments)?,
                working_directory: request.working_directory.clone(),
                parent_agent_id: request.parent_agent_id.clone(),
                state: tool_approval_state(value.state()),
            }
        }
    })
}

fn user_input_state(value: &UserInputState) -> BridgeUserInputInteractionState {
    match value {
        UserInputState::Pending(state) => BridgeUserInputInteractionState::Pending {
            operation_id: state.operation_id().to_owned(),
        },
        UserInputState::Resolved(state) => BridgeUserInputInteractionState::Resolved {
            operation_id: state.operation_id().to_owned(),
            resolved_at: state.resolved_at(),
            answers: sorted_answers(state.answers()),
        },
        UserInputState::Cancelled(state) => BridgeUserInputInteractionState::Cancelled {
            operation_id: state.operation_id().to_owned(),
            cancelled_at: state.cancelled_at(),
            reason: state.reason().to_owned(),
        },
        UserInputState::Expired(state) => expired_user_input(state),
    }
}

fn expired_user_input(state: &ExpiredInteractionState) -> BridgeUserInputInteractionState {
    BridgeUserInputInteractionState::Expired {
        operation_id: state.operation_id().to_owned(),
        expired_at: state.expired_at(),
    }
}

fn tool_approval_state(value: &ToolApprovalState) -> BridgeToolApprovalInteractionState {
    match value {
        ToolApprovalState::Pending(state) => BridgeToolApprovalInteractionState::Pending {
            operation_id: state.operation_id().to_owned(),
        },
        ToolApprovalState::Resolved(state) => BridgeToolApprovalInteractionState::Resolved {
            operation_id: state.operation_id().to_owned(),
            resolved_at: state.resolved_at(),
            decision: tool_approval_resolution(state.decision()),
            reason: state.reason().map(str::to_owned),
        },
        ToolApprovalState::Cancelled(state) => cancelled_tool_approval(state),
        ToolApprovalState::Expired(state) => BridgeToolApprovalInteractionState::Expired {
            operation_id: state.operation_id().to_owned(),
            expired_at: state.expired_at(),
        },
    }
}

fn cancelled_tool_approval(
    state: &CancelledInteractionState,
) -> BridgeToolApprovalInteractionState {
    BridgeToolApprovalInteractionState::Cancelled {
        operation_id: state.operation_id().to_owned(),
        cancelled_at: state.cancelled_at(),
        reason: state.reason().to_owned(),
    }
}

fn sorted_answers(
    value: &std::collections::HashMap<String, pl_protocol::UserInputAnswer>,
) -> Vec<BridgeUserInputAnswer> {
    let mut answers = value
        .iter()
        .map(|(question_id, answer)| BridgeUserInputAnswer {
            question_id: question_id.clone(),
            answers: answer.answers.clone(),
        })
        .collect::<Vec<_>>();
    answers.sort_by(|left, right| left.question_id.cmp(&right.question_id));
    answers
}

fn tool_approval_resolution(value: ToolApprovalResolution) -> BridgeToolApprovalResolution {
    match value {
        ToolApprovalResolution::Approved => BridgeToolApprovalResolution::Approved,
        ToolApprovalResolution::Denied => BridgeToolApprovalResolution::Denied,
    }
}

fn question(value: UserQuestion) -> BridgeUserQuestion {
    BridgeUserQuestion {
        id: value.id,
        header: value.header,
        question: value.question,
        is_other: value.is_other,
        is_secret: value.is_secret,
        options: value.options.map(|options| {
            options
                .into_iter()
                .map(|option| BridgeUserQuestionOption {
                    label: option.label,
                    description: option.description,
                })
                .collect()
        }),
    }
}
