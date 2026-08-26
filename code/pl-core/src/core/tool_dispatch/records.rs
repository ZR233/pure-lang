use std::path::PathBuf;

use pl_protocol::PureError;
use pl_trace::{
    AgentEvent, TraceEventKind, TracePartAction, TracePartCompletion, TraceToolActivePhase,
    TraceToolFailure, TraceToolFailureKind, TraceToolOutput,
};

use crate::tool::model_visible_tool_output;
use crate::tool::{ToolDirective, ToolResult};

use super::display::display_result_for_tool;
use super::unix_seconds;
use super::{ToolExecutionError, ToolExecutionOutcome, ToolExecutionRecord};

#[derive(Debug, Clone)]
pub(super) struct ToolOutputEnvelope {
    pub(super) model_visible_text: String,
    pub(super) display_text: String,
    pub(super) full_output_file: Option<PathBuf>,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) runtime_events: Vec<ToolDirective>,
}

pub(super) fn emit_tool_snapshot(
    recorder: &mut crate::trace::TraceRecorder,
    item: &mut pl_trace::TracePart,
    phase: TraceToolActivePhase,
) {
    let now = unix_seconds();
    if let Err(error) = item.apply(item.command(now, TracePartAction::EnterToolPhase { phase })) {
        tracing::error!(%error, "failed to advance active tool trace state");
        return;
    }
    recorder.update_item_snapshot(item.clone());
}

pub(super) fn finalize_tool_item(
    recorder: &mut crate::trace::TraceRecorder,
    mut item: pl_trace::TracePart,
    record: &ToolExecutionRecord,
) {
    if let Some(revision) = record
        .runtime_events
        .iter()
        .filter_map(|event| match event {
            ToolDirective::ToolResultRevision { revision } => Some(*revision),
            _ => None,
        })
        .max()
        && let Err(error) = item.synchronize_open_revision(revision, unix_seconds())
    {
        tracing::error!(%error, "failed to join streamed tool-result revision");
        return;
    }
    let output = TraceToolOutput::new(record.display_result.clone()).with_details(
        record.exit_code,
        output_artifacts(&record.runtime_events),
        audit_metadata(&record.runtime_events),
        output_metrics(&record.runtime_events),
    );
    let action = match record.outcome {
        ToolExecutionOutcome::Succeeded => {
            TracePartAction::Complete(TracePartCompletion::Tool { output })
        }
        ToolExecutionOutcome::Failed(kind) => TracePartAction::FailTool {
            failure: TraceToolFailure::new(kind, record.display_result.clone()),
            output: Some(output),
        },
        ToolExecutionOutcome::Denied => TracePartAction::DenyTool {
            reason: record.display_result.clone(),
        },
        ToolExecutionOutcome::Cancelled => TracePartAction::Cancel {
            reason: record.display_result.clone(),
        },
    };
    let now = unix_seconds();
    if let Err(error) = item.apply(item.command(now, action)) {
        tracing::error!(%error, "failed to terminalize tool trace item");
        return;
    }
    match record.outcome {
        ToolExecutionOutcome::Succeeded => recorder.complete_item(item),
        ToolExecutionOutcome::Failed(_)
        | ToolExecutionOutcome::Denied
        | ToolExecutionOutcome::Cancelled => recorder.fail_item(item),
    }
    if record.outcome != ToolExecutionOutcome::Succeeded {
        recorder.terminalize_plan_item_for_tool(
            &record.id,
            record.outcome == ToolExecutionOutcome::Cancelled,
            &record.display_result,
        );
    }
    if record.outcome == ToolExecutionOutcome::Succeeded {
        for event in &record.runtime_events {
            match event {
                ToolDirective::InteractionRequested { interaction } => {
                    recorder.broadcast(AgentEvent::InteractionChanged {
                        event: pl_protocol::InteractionChangedEvent {
                            interaction: interaction.as_ref().clone(),
                        },
                    });
                }
                ToolDirective::SkillActivated { activation } => {
                    recorder.record_trace_only(TraceEventKind::SkillActivated {
                        activation: activation.clone(),
                    });
                    recorder.broadcast(AgentEvent::SkillActivated {
                        activation: activation.clone(),
                    });
                }
                ToolDirective::PlanCompleted { .. } => {}
                ToolDirective::ToolResultRevision { .. } => {}
                ToolDirective::OutputArtifacts { .. } => {}
                ToolDirective::AuditMetadata { .. } => {}
                ToolDirective::ExecutionFailed => {}
                ToolDirective::CacheHit { .. } => {}
                ToolDirective::OutputMetrics { .. } => {}
                ToolDirective::OutputBudget { .. } => {}
                ToolDirective::EndTurn { .. } => {}
            }
        }
    }
}

pub(super) async fn ready_tool_execution_record(
    tool_call: pl_model::ToolCall,
    error: ToolExecutionError,
    outcome: ToolExecutionOutcome,
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
            outcome,
        )),
        ToolExecutionError::Fatal(message) => Err(ToolExecutionError::Fatal(message)),
    }
}

pub(super) fn tool_execution_record(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    result: std::result::Result<ToolResult, PureError>,
) -> Result<ToolExecutionRecord, ToolExecutionError> {
    let (envelope, outcome) = match result {
        Ok(output) => {
            let timed_out = output.timed_out;
            let canonical_output = output.canonical_output();
            let execution_failed = output
                .runtime_events
                .iter()
                .any(|event| matches!(event, ToolDirective::ExecutionFailed));
            let budget_bytes = output.runtime_events.iter().find_map(|event| match event {
                ToolDirective::OutputBudget { max_bytes } => Some(*max_bytes),
                _ => None,
            });
            let model_visible_text = match budget_bytes {
                Some(max_bytes) => crate::tool::model_visible_tool_output_with_budget(
                    output.model_output(),
                    max_bytes / crate::tool::TOKEN_ESTIMATE_BYTES,
                    max_bytes,
                ),
                None => model_visible_tool_output(output.model_output()),
            };
            let mut runtime_events = output.runtime_events;
            if !runtime_events
                .iter()
                .any(|event| matches!(event, ToolDirective::OutputMetrics { .. }))
            {
                runtime_events.push(ToolDirective::OutputMetrics {
                    raw_bytes: canonical_output.len() as u64,
                    model_visible_bytes: model_visible_text.len() as u64,
                    artifact_bytes: 0,
                    result_hash: crate::canonical_content_hash(canonical_output.as_bytes()),
                });
            }
            (
                ToolOutputEnvelope {
                    model_visible_text,
                    display_text: canonical_output,
                    full_output_file: (!output.output_file.as_os_str().is_empty())
                        .then_some(output.output_file),
                    exit_code: output.exit_code,
                    timed_out,
                    runtime_events,
                },
                if timed_out {
                    ToolExecutionOutcome::Failed(TraceToolFailureKind::TimedOut)
                } else if execution_failed {
                    ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution)
                } else {
                    ToolExecutionOutcome::Succeeded
                },
            )
        }
        Err(error) => {
            return Err(ToolExecutionError::RespondToModel(format!(
                "Tool execution error: {error}"
            )));
        }
    };
    Ok(tool_execution_record_from_envelope(
        tool_call, tool_name, envelope, outcome,
    ))
}

fn tool_execution_record_from_envelope(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    envelope: ToolOutputEnvelope,
    outcome: ToolExecutionOutcome,
) -> ToolExecutionRecord {
    let ToolOutputEnvelope {
        model_visible_text,
        display_text,
        full_output_file: _full_output_file,
        exit_code,
        timed_out,
        runtime_events,
    } = envelope;
    let (raw_bytes, model_visible_bytes, artifact_bytes) = runtime_events
        .iter()
        .find_map(|event| match event {
            ToolDirective::OutputMetrics {
                raw_bytes,
                model_visible_bytes,
                artifact_bytes,
                result_hash: _,
            } => Some((*raw_bytes, *model_visible_bytes, *artifact_bytes)),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::PlanCompleted { .. }
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::AuditMetadata { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::EndTurn { .. } => None,
        })
        .unwrap_or((
            model_visible_text.len() as u64,
            model_visible_text.len() as u64,
            0,
        ));
    let cache_hit = runtime_events
        .iter()
        .any(|event| matches!(event, ToolDirective::CacheHit { .. }));
    tracing::trace!(
        target: "pl_core::tool_metrics",
        tool = %tool_name,
        raw_bytes,
        model_visible_bytes,
        artifact_bytes,
        cache_hit,
        "tool output projected"
    );
    let display_result = display_result_for_tool(&tool_call, &tool_name, &display_text, outcome);
    ToolExecutionRecord {
        id: tool_call.id.clone(),
        call_id: tool_call.call_id.clone(),
        name: tool_name,
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result: model_visible_text,
        display_result,
        outcome,
        exit_code,
        timed_out,
        runtime_events,
        execution_millis: 0,
    }
}

fn output_artifacts(runtime_events: &[ToolDirective]) -> Vec<serde_json::Value> {
    runtime_events
        .iter()
        .filter_map(|event| match event {
            ToolDirective::OutputArtifacts { artifacts } => Some(artifacts.as_slice()),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::PlanCompleted { .. }
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::AuditMetadata { .. }
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputMetrics { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::EndTurn { .. } => None,
        })
        .flatten()
        .cloned()
        .collect()
}

fn audit_metadata(runtime_events: &[ToolDirective]) -> Vec<serde_json::Value> {
    runtime_events
        .iter()
        .filter_map(|event| match event {
            ToolDirective::AuditMetadata { metadata } => Some(metadata.clone()),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::PlanCompleted { .. }
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputMetrics { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::EndTurn { .. } => None,
        })
        .collect()
}

fn output_metrics(runtime_events: &[ToolDirective]) -> Option<pl_trace::TraceToolOutputMetrics> {
    let cache_hit = runtime_events
        .iter()
        .any(|event| matches!(event, ToolDirective::CacheHit { .. }));
    runtime_events.iter().find_map(|event| match event {
        ToolDirective::OutputMetrics {
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
        ToolDirective::InteractionRequested { .. }
        | ToolDirective::SkillActivated { .. }
        | ToolDirective::PlanCompleted { .. }
        | ToolDirective::ToolResultRevision { .. }
        | ToolDirective::OutputArtifacts { .. }
        | ToolDirective::AuditMetadata { .. }
        | ToolDirective::ExecutionFailed
        | ToolDirective::CacheHit { .. }
        | ToolDirective::OutputBudget { .. }
        | ToolDirective::EndTurn { .. } => None,
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
        ToolExecutionOutcome::Cancelled,
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
        ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution),
    )
}
