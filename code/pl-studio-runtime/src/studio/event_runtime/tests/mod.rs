use crate::{StudioPartStatus, StudioPartType, StudioTextChannel};
use pl_trace::{
    AgentEvent, TraceDelta, TracePart, TracePartDeltaEvent, TracePartKind, TracePartSource,
    TracePartStatus, TraceTextChannel, TraceToolPart,
};

use super::*;
use crate::StudioMode;

mod delta;
mod live_events;
mod message_lifecycle;
mod part_projection;

fn streaming_text_part(turn_id: &str, item_id: &str) -> TracePart {
    TracePart::text(
        turn_id,
        item_id,
        0,
        TraceTextChannel::Final,
        "",
        TracePartStatus::Streaming,
        100,
    )
}

fn thinking_part(turn_id: &str, item_id: &str, sequence: u64, text: &str) -> TracePart {
    TracePart {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: sequence,
        revision: 0,
        kind: TracePartKind::Thinking,
        status: TracePartStatus::Completed,
        created_at: 100,
        updated_at: 100,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: vec![pl_trace::TraceThinkingChunk {
            chunk_index: 0,
            content: text.to_string(),
        }],
        tool: None,
        agent: None,
        inference: None,
        usage: None,
    }
}

fn tool_part(turn_id: &str, item_id: &str, sequence: u64) -> TracePart {
    TracePart {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: sequence,
        revision: 0,
        kind: TracePartKind::Tool,
        status: TracePartStatus::Completed,
        created_at: 100,
        updated_at: 100,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: Vec::new(),
        tool: Some(TraceToolPart {
            tool_call_id: item_id.to_string(),
            call_id: Some(format!("{item_id}-call")),
            provider_item_id: Some(item_id.to_string()),
            name: "bash".to_string(),
            arguments: "{}".to_string(),
            result: Some("ok".to_string()),
            exit_code: Some(0),
            timed_out: false,
            output_artifacts: Vec::new(),
            output_metrics: None,
            working_directory: None,
            denial_reason: None,
        }),
        agent: None,
        inference: None,
        usage: None,
    }
}

fn text_delta_event(
    turn_id: &str,
    item_id: &str,
    revision: u64,
    delta: &str,
) -> TracePartDeltaEvent {
    TracePartDeltaEvent {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: 0,
        revision,
        kind: TracePartKind::Text,
        status: TracePartStatus::Streaming,
        created_at: 100,
        updated_at: 100,
        delta: TraceDelta::Text {
            text_channel: TraceTextChannel::Final,
            delta: delta.to_string(),
        },
    }
}

fn commentary_delta_event(
    turn_id: &str,
    item_id: &str,
    revision: u64,
    delta: &str,
) -> TracePartDeltaEvent {
    TracePartDeltaEvent {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: 0,
        revision,
        kind: TracePartKind::Text,
        status: TracePartStatus::Streaming,
        created_at: 100,
        updated_at: 100,
        delta: TraceDelta::Text {
            text_channel: TraceTextChannel::Commentary,
            delta: delta.to_string(),
        },
    }
}
