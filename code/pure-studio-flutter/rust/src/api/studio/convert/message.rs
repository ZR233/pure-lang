use crate::api::studio::types::{
    BridgeStudioAgentPartDto, BridgeStudioMessageDto, BridgeStudioPartDeltaDto,
    BridgeStudioPartDto, BridgeStudioPlanPartDto, BridgeStudioToolPartDto, BridgeStudioTurnDto,
};
use pl_studio_runtime::{
    StudioMessage, StudioPart, StudioPartDelta, StudioPartDeltaField, StudioTurn,
};
pub(crate) fn bridge_turn(turn: StudioTurn) -> BridgeStudioTurnDto {
    BridgeStudioTurnDto {
        turn_id: turn.turn_id,
        session_id: turn.session_id,
        status: turn.status.as_str().to_string(),
        reason: turn.reason,
        updated_at: turn.updated_at,
    }
}

pub(crate) fn bridge_message(message: StudioMessage) -> BridgeStudioMessageDto {
    BridgeStudioMessageDto {
        message_id: message.message_id,
        session_id: message.session_id,
        turn_id: message.turn_id,
        role: message.role.as_str().to_string(),
        status: message.status.as_str().to_string(),
        created_at: message.created_at,
        updated_at: message.updated_at,
        completed_at: message.completed_at,
        error: message.error,
    }
}

pub(crate) fn bridge_part(part: StudioPart) -> BridgeStudioPartDto {
    BridgeStudioPartDto {
        part_id: part.part_id,
        message_id: part.message_id,
        session_id: part.session_id,
        turn_id: part.turn_id,
        part_type: part.part_type.as_str().to_string(),
        order: part.order,
        revision: part.revision,
        status: part.status.as_str().to_string(),
        created_at: part.created_at,
        updated_at: part.updated_at,
        completed_at: part.completed_at,
        error: part.error,
        text_channel: part
            .text_channel
            .map(|channel| channel.as_str().to_string()),
        activity_group_id: part.activity_group_id,
        text: part.text,
        tool: part.tool.map(|tool| BridgeStudioToolPartDto {
            tool_call_id: tool.tool_call_id,
            call_id: tool.call_id,
            provider_item_id: tool.provider_item_id,
            name: tool.name,
            arguments: tool.arguments,
            result: tool.result,
            exit_code: tool.exit_code,
            timed_out: tool.timed_out,
            working_directory: tool.working_directory,
            denial_reason: tool.denial_reason,
        }),
        agent: part.agent.map(|agent| BridgeStudioAgentPartDto {
            id: agent.id,
            path: agent.path,
            parent_path: agent.parent_path,
            role: agent.role,
            task: agent.task,
            status: agent.status.as_str().to_string(),
            summary: agent.summary,
            depth: agent.depth,
            error: agent.error,
            reason: agent.reason,
        }),
        plan: part.plan.map(|plan| BridgeStudioPlanPartDto {
            content: plan.content,
        }),
        synthetic: part.synthetic,
        ignored: part.ignored,
    }
}

pub(crate) fn bridge_part_delta(delta: StudioPartDelta) -> BridgeStudioPartDeltaDto {
    BridgeStudioPartDeltaDto {
        part_id: delta.part_id,
        revision: delta.revision,
        field: bridge_part_delta_field(delta.field),
        delta: delta.delta,
        chunk_index: delta.chunk_index,
    }
}

pub(crate) fn bridge_part_delta_field(field: StudioPartDeltaField) -> String {
    match field {
        StudioPartDeltaField::Text => "text".to_string(),
        StudioPartDeltaField::ReasoningSummary => "reasoning.summary".to_string(),
        StudioPartDeltaField::PlanContent => "planContent".to_string(),
        StudioPartDeltaField::ToolArguments => "tool.arguments".to_string(),
        StudioPartDeltaField::ToolResult => "tool.result".to_string(),
    }
}
