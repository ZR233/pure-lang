use std::path::PathBuf;

use pl_protocol::PureError;
use pl_trace::{AgentEvent, TraceEventKind, TracePartStatus};

use crate::tool::model_visible_tool_output;
use crate::tool::{ToolOutput, ToolRuntimeEvent};

use super::display::display_result_for_tool;
use super::unix_seconds;
use super::{ToolExecutionError, ToolExecutionRecord};

#[derive(Debug, Clone)]
pub(super) struct ToolOutputEnvelope {
    pub(super) model_visible_text: String,
    pub(super) display_text: String,
    pub(super) full_output_file: Option<PathBuf>,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) runtime_events: Vec<ToolRuntimeEvent>,
}

pub(super) fn emit_tool_snapshot(
    recorder: &mut crate::trace::TraceRecorder,
    item: &mut pl_trace::TracePart,
    status: TracePartStatus,
) {
    item.status = status;
    item.updated_at = unix_seconds();
    recorder.update_item_snapshot(item.clone());
}

pub(super) fn finalize_tool_item(
    recorder: &mut crate::trace::TraceRecorder,
    mut item: pl_trace::TracePart,
    record: &ToolExecutionRecord,
) {
    item.status = record.status;
    item.updated_at = unix_seconds();
    if let Some(tool) = &mut item.tool {
        tool.result = Some(record.display_result.clone());
        tool.exit_code = record.exit_code;
        tool.timed_out = record.timed_out;
        tool.output_artifacts = output_artifacts(&record.runtime_events);
        tool.output_metrics = output_metrics(&record.runtime_events);
    }
    if let Some(revision) = record.revision {
        item.revision = item.revision.max(revision);
    }
    let status = item.status;
    match status {
        TracePartStatus::Failed
        | TracePartStatus::Denied
        | TracePartStatus::Interrupted
        | TracePartStatus::BudgetLimited => {
            recorder.fail_item(item, record.display_result.clone());
        }
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Running
        | TracePartStatus::Completed => recorder.complete_item(item),
    }
    if status == TracePartStatus::Completed {
        for event in &record.runtime_events {
            match event {
                ToolRuntimeEvent::SkillActivated { activation } => {
                    recorder.record_trace_only(TraceEventKind::SkillActivated {
                        activation: activation.clone(),
                    });
                    recorder.broadcast(AgentEvent::SkillActivated {
                        activation: activation.clone(),
                    });
                }
                ToolRuntimeEvent::ToolResultRevision { .. } => {}
                ToolRuntimeEvent::OutputArtifacts { .. } => {}
                ToolRuntimeEvent::CacheHit { .. } => {}
                ToolRuntimeEvent::OutputMetrics { .. } => {}
                ToolRuntimeEvent::EndTurn => {}
            }
        }
    }
}

pub(super) async fn ready_tool_execution_record(
    tool_call: pl_model::ToolCall,
    error: ToolExecutionError,
    status: TracePartStatus,
    exit_code: Option<i32>,
    timed_out: bool,
) -> Result<ToolExecutionRecord, ToolExecutionError> {
    match error {
        ToolExecutionError::RespondToModel(message) => Ok(tool_execution_record_from_envelope(
            tool_call.clone(),
            tool_call.name.clone(),
            ToolOutputEnvelope {
                model_visible_text: message.clone(),
                display_text: message,
                full_output_file: None,
                exit_code,
                timed_out,
                runtime_events: Vec::new(),
            },
            status,
        )),
        ToolExecutionError::Fatal(message) => Err(ToolExecutionError::Fatal(message)),
    }
}

pub(super) fn tool_execution_record(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    result: std::result::Result<ToolOutput, PureError>,
) -> Result<ToolExecutionRecord, ToolExecutionError> {
    let (envelope, status) = match result {
        Ok(output) => {
            let model_visible_text = model_visible_tool_output(&output.description);
            let mut runtime_events = output.runtime_events;
            if !runtime_events
                .iter()
                .any(|event| matches!(event, ToolRuntimeEvent::OutputMetrics { .. }))
            {
                runtime_events.push(ToolRuntimeEvent::OutputMetrics {
                    raw_bytes: output.description.len() as u64,
                    model_visible_bytes: model_visible_text.len() as u64,
                    artifact_bytes: 0,
                    result_hash: crate::canonical_content_hash(output.description.as_bytes()),
                });
            }
            (
                ToolOutputEnvelope {
                    model_visible_text,
                    display_text: output.description,
                    full_output_file: (!output.output_file.as_os_str().is_empty())
                        .then_some(output.output_file),
                    exit_code: output.exit_code,
                    timed_out: output.timed_out,
                    runtime_events,
                },
                TracePartStatus::Completed,
            )
        }
        Err(error) => {
            return Err(ToolExecutionError::RespondToModel(format!(
                "Tool execution error: {error}"
            )));
        }
    };
    Ok(tool_execution_record_from_envelope(
        tool_call, tool_name, envelope, status,
    ))
}

fn tool_execution_record_from_envelope(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    envelope: ToolOutputEnvelope,
    status: TracePartStatus,
) -> ToolExecutionRecord {
    let ToolOutputEnvelope {
        model_visible_text,
        display_text,
        full_output_file: _full_output_file,
        exit_code,
        timed_out,
        runtime_events,
    } = envelope;
    let revision = runtime_events.iter().find_map(|event| match event {
        ToolRuntimeEvent::ToolResultRevision { revision } => Some(*revision),
        ToolRuntimeEvent::SkillActivated { .. }
        | ToolRuntimeEvent::OutputArtifacts { .. }
        | ToolRuntimeEvent::CacheHit { .. }
        | ToolRuntimeEvent::OutputMetrics { .. }
        | ToolRuntimeEvent::EndTurn => None,
    });
    let (raw_bytes, model_visible_bytes, artifact_bytes) = runtime_events
        .iter()
        .find_map(|event| match event {
            ToolRuntimeEvent::OutputMetrics {
                raw_bytes,
                model_visible_bytes,
                artifact_bytes,
                result_hash: _,
            } => Some((*raw_bytes, *model_visible_bytes, *artifact_bytes)),
            ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::EndTurn => None,
        })
        .unwrap_or((
            model_visible_text.len() as u64,
            model_visible_text.len() as u64,
            0,
        ));
    let cache_hit = runtime_events
        .iter()
        .any(|event| matches!(event, ToolRuntimeEvent::CacheHit { .. }));
    tracing::trace!(
        target: "pl_core::tool_metrics",
        tool = %tool_name,
        raw_bytes,
        model_visible_bytes,
        artifact_bytes,
        cache_hit,
        "tool output projected"
    );
    let display_result = display_result_for_tool(&tool_call, &tool_name, &display_text, status);
    ToolExecutionRecord {
        id: tool_call.id.clone(),
        call_id: tool_call.call_id.clone(),
        name: tool_name,
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result: model_visible_text,
        display_result,
        status,
        exit_code,
        timed_out,
        revision,
        runtime_events,
    }
}

fn output_artifacts(runtime_events: &[ToolRuntimeEvent]) -> Vec<serde_json::Value> {
    runtime_events
        .iter()
        .filter_map(|event| match event {
            ToolRuntimeEvent::OutputArtifacts { artifacts } => Some(artifacts.as_slice()),
            ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::OutputMetrics { .. }
            | ToolRuntimeEvent::EndTurn => None,
        })
        .flatten()
        .cloned()
        .collect()
}

fn output_metrics(runtime_events: &[ToolRuntimeEvent]) -> Option<pl_trace::TraceToolOutputMetrics> {
    let cache_hit = runtime_events
        .iter()
        .any(|event| matches!(event, ToolRuntimeEvent::CacheHit { .. }));
    runtime_events.iter().find_map(|event| match event {
        ToolRuntimeEvent::OutputMetrics {
            raw_bytes,
            model_visible_bytes,
            artifact_bytes,
            result_hash,
        } => Some(pl_trace::TraceToolOutputMetrics {
            raw_bytes: *raw_bytes,
            model_visible_bytes: *model_visible_bytes,
            artifact_bytes: *artifact_bytes,
            result_hash: result_hash.clone(),
            cache_hit,
        }),
        ToolRuntimeEvent::SkillActivated { .. }
        | ToolRuntimeEvent::ToolResultRevision { .. }
        | ToolRuntimeEvent::OutputArtifacts { .. }
        | ToolRuntimeEvent::CacheHit { .. }
        | ToolRuntimeEvent::EndTurn => None,
    })
}

pub(super) fn interrupted_tool_execution_record(
    tool_call: pl_model::ToolCall,
) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: "Tool execution interrupted".to_string(),
            display_text: "Tool execution interrupted".to_string(),
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        TracePartStatus::Interrupted,
    )
}

pub(super) fn respond_to_model_tool_execution_record(
    tool_call: pl_model::ToolCall,
    message: String,
) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: message.clone(),
            display_text: message,
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        TracePartStatus::Failed,
    )
}
