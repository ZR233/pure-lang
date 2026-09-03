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
    pub(super) model_attachments: Vec<pl_protocol::ThreadAttachment>,
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
        record
            .model_attachments
            .iter()
            .map(trace_attachment)
            .collect(),
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
                ToolDirective::ToolResultRevision { .. } => {}
                ToolDirective::OutputArtifacts { .. } => {}
                ToolDirective::RevealTools { .. } => {}
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
    tool_call: pl_model::completion::ToolCall,
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
                model_attachments: Vec::new(),
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
    tool_call: pl_model::completion::ToolCall,
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
            let model_attachments = output.model_attachments;
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
                    model_attachments,
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
    tool_call: pl_model::completion::ToolCall,
    tool_name: String,
    envelope: ToolOutputEnvelope,
    outcome: ToolExecutionOutcome,
) -> ToolExecutionRecord {
    let ToolOutputEnvelope {
        model_visible_text,
        display_text,
        model_attachments,
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
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::RevealTools { .. }
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
        model_attachments,
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
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::RevealTools { .. }
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
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::RevealTools { .. }
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
        | ToolDirective::ToolResultRevision { .. }
        | ToolDirective::OutputArtifacts { .. }
        | ToolDirective::RevealTools { .. }
        | ToolDirective::AuditMetadata { .. }
        | ToolDirective::ExecutionFailed
        | ToolDirective::CacheHit { .. }
        | ToolDirective::OutputBudget { .. }
        | ToolDirective::EndTurn { .. } => None,
    })
}

pub(super) fn interrupted_tool_execution_record(
    tool_call: pl_model::completion::ToolCall,
) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: "Tool execution interrupted".to_string(),
            display_text: "Tool execution interrupted".to_string(),
            model_attachments: Vec::new(),
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        ToolExecutionOutcome::Cancelled,
    )
}

pub(super) fn respond_to_model_tool_execution_record(
    tool_call: pl_model::completion::ToolCall,
    message: String,
) -> ToolExecutionRecord {
    tool_execution_record_from_envelope(
        tool_call.clone(),
        tool_call.name.clone(),
        ToolOutputEnvelope {
            model_visible_text: message.clone(),
            display_text: message,
            model_attachments: Vec::new(),
            full_output_file: None,
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        },
        ToolExecutionOutcome::Failed(TraceToolFailureKind::Execution),
    )
}

fn trace_attachment(attachment: &pl_protocol::ThreadAttachment) -> pl_trace::TraceAttachment {
    pl_trace::TraceAttachment {
        id: attachment.id.clone(),
        modality: match attachment.modality {
            pl_protocol::AttachmentModality::Image => pl_trace::TraceAttachmentModality::Image,
            pl_protocol::AttachmentModality::Video => pl_trace::TraceAttachmentModality::Video,
            pl_protocol::AttachmentModality::File => pl_trace::TraceAttachmentModality::File,
        },
        media_type: attachment.media_type.clone(),
        filename: attachment.filename.clone(),
        width: attachment.width,
        height: attachment.height,
        byte_size: attachment.byte_size,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use pl_protocol::ToolCallKind;
    fn completed_record(name: &str) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: "item-1".to_string(),
            call_id: "call-1".to_string(),
            name: name.to_string(),
            kind: ToolCallKind::Function,
            result: String::new(),
            display_result: String::new(),
            arguments: "{}".to_string(),
            outcome: ToolExecutionOutcome::Succeeded,
            exit_code: None,
            timed_out: false,
            model_attachments: Vec::new(),
            runtime_events: Vec::new(),
            execution_millis: 0,
        }
    }

    #[test]
    fn finalize_tool_item_separates_output_artifacts_and_audit_metadata() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = crate::TraceRecorder::new("session".to_string(), event_tx, 0);
        let item = recorder.tool_item(
            "turn-1",
            "turn-1-call-1",
            "exec".to_string(),
            "{}".to_string(),
            Some("call-1".to_string()),
            None,
        );
        recorder.start_item(item.clone());
        let mut record = completed_record("exec");
        record
            .runtime_events
            .push(crate::tool::ToolDirective::OutputArtifacts {
                artifacts: vec![serde_json::json!({
                    "id": "artifact-1",
                    "stream": "stdout",
                })],
            });
        record
            .runtime_events
            .push(crate::tool::ToolDirective::OutputMetrics {
                raw_bytes: 20_000,
                model_visible_bytes: 12_000,
                artifact_bytes: 8_000,
                result_hash: "result-hash".to_string(),
            });
        record
            .runtime_events
            .push(crate::tool::ToolDirective::AuditMetadata {
                metadata: serde_json::json!({
                    "kind": "mcpCallToolResult",
                    "result": { "structuredContent": { "answer": 42 } },
                }),
            });
        record
            .runtime_events
            .push(crate::tool::ToolDirective::CacheHit {
                reused_from_call_id: "earlier".to_string(),
                result_hash: "hash".to_string(),
                total_bytes: 20_000,
            });

        finalize_tool_item(&mut recorder, item, &record);
        let events = recorder.drain();
        let completed = events
            .iter()
            .find_map(|event| match &event.kind {
                pl_trace::TraceEventKind::TracePartCompleted { item } => Some(item),
                pl_trace::TraceEventKind::TracePartStarted { .. }
                | pl_trace::TraceEventKind::TracePartDelta { .. }
                | pl_trace::TraceEventKind::TracePartFailed { .. }
                | pl_trace::TraceEventKind::InteractionChanged { .. }
                | pl_trace::TraceEventKind::SkillActivated { .. }
                | pl_trace::TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("completed tool item");

        let output = completed
            .tool()
            .and_then(pl_trace::TraceToolPart::terminal_output)
            .expect("completed tool output");
        assert_eq!(
            output.output_artifacts(),
            vec![serde_json::json!({
                "id": "artifact-1",
                "stream": "stdout",
            })]
        );
        assert_eq!(
            output.audit_metadata(),
            vec![serde_json::json!({
                "kind": "mcpCallToolResult",
                "result": { "structuredContent": { "answer": 42 } },
            })]
        );
        assert_eq!(
            output.metrics(),
            Some(&pl_trace::TraceToolOutputMetrics {
                raw_bytes: 20_000,
                model_visible_bytes: 12_000,
                artifact_bytes: 8_000,
                result_hash: "result-hash".to_string(),
                cache_hit: true,
            })
        );
    }

    #[tokio::test]
    async fn completed_tool_callback_receives_the_canonical_terminal_record() {
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let callback_observed = observed.clone();
        let options = crate::TurnOptions::default().with_tool_completion_callback(
            std::sync::Arc::new(move |completion| {
                let callback_observed = callback_observed.clone();
                Box::pin(async move {
                    callback_observed.lock().unwrap().push(completion);
                    Ok(())
                })
            }),
        );
        let mut record = completed_record("exec");
        record.result = "generated file".to_string();
        record.exit_code = Some(0);

        super::super::notify_tool_completion(&options, &record)
            .await
            .unwrap();

        let observations = observed.lock().unwrap();
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].call_id, "call-1");
        assert_eq!(observations[0].name, "exec");
        assert_eq!(observations[0].status, "succeeded");
        assert_eq!(observations[0].result, "generated file");
        assert_eq!(observations[0].exit_code, Some(0));
    }

    #[tokio::test]
    async fn completed_tool_callback_failure_is_a_fatal_host_boundary_error() {
        let options = crate::TurnOptions::default().with_tool_completion_callback(
            std::sync::Arc::new(|_| Box::pin(async { anyhow::bail!("fingerprint failed") })),
        );

        let error =
            super::super::notify_tool_completion(&options, &completed_record("apply_patch"))
                .await
                .unwrap_err();

        assert_eq!(
            error,
            ToolExecutionError::Fatal(
                "host post-tool observation failed after apply_patch: fingerprint failed"
                    .to_string()
            )
        );
    }
}
