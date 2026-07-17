use super::records::session_dto;
use crate::api::studio::types::{
    BridgeInteractionChangedDto, BridgeInteractionPayloadDto, BridgeUserQuestionDto,
    BridgeUserQuestionOptionDto, ResolveInteractionResponse,
};
use pl_protocol::{InteractionPayload, InteractionRequest};
pub(crate) fn interaction_request_bridge_dto(
    interaction: InteractionRequest,
) -> BridgeInteractionChangedDto {
    bridge_interaction_changed(pl_protocol::InteractionChangedEvent { interaction })
}

pub(crate) fn resolve_interaction_response(
    response: pl_studio_runtime::StudioResolveInteractionResponse,
) -> ResolveInteractionResponse {
    ResolveInteractionResponse {
        session_id: response.session_id,
        interaction: bridge_interaction_changed(pl_protocol::InteractionChangedEvent {
            interaction: response.interaction,
        }),
        sessions: response.sessions.into_iter().map(session_dto).collect(),
    }
}

pub(crate) fn bridge_interaction_changed(
    event: pl_protocol::InteractionChangedEvent,
) -> BridgeInteractionChangedDto {
    let interaction = event.interaction;
    BridgeInteractionChangedDto {
        interaction_id: interaction.interaction_id,
        kind: interaction.kind.as_str().to_string(),
        status: interaction.status.as_str().to_string(),
        session_id: interaction.scope.session_id,
        turn_id: interaction.scope.turn_id,
        item_id: interaction.scope.item_id,
        tool_id: interaction.scope.tool_id,
        agent_path: interaction.scope.agent_path,
        payload: bridge_interaction_payload(interaction.payload),
        created_at: interaction.created_at,
        updated_at: interaction.updated_at,
        resolved_at: interaction.resolved_at,
    }
}

pub(crate) fn bridge_interaction_payload(
    payload: InteractionPayload,
) -> BridgeInteractionPayloadDto {
    match payload {
        InteractionPayload::UserInput { questions } => BridgeInteractionPayloadDto::UserInput {
            questions: questions
                .into_iter()
                .map(|question| BridgeUserQuestionDto {
                    id: question.id,
                    header: question.header,
                    question: question.question,
                    is_other: question.is_other,
                    is_secret: question.is_secret,
                    options: question.options.map(|options| {
                        options
                            .into_iter()
                            .map(|option| BridgeUserQuestionOptionDto {
                                label: option.label,
                                description: option.description,
                            })
                            .collect()
                    }),
                })
                .collect(),
        },
        InteractionPayload::ToolApproval {
            name,
            arguments,
            working_directory,
            parent_agent_id,
        } => BridgeInteractionPayloadDto::ToolApproval {
            name,
            arguments_json: arguments.to_string(),
            working_directory,
            parent_agent_id,
        },
        InteractionPayload::PlanConfirmation { plan_id, content } => {
            BridgeInteractionPayloadDto::PlanConfirmation { plan_id, content }
        }
    }
}
