use pretty_assertions::assert_eq;

use super::display::redact_user_input_display_result;
use super::progress_messages::{tool_start_progress_message, tool_terminal_progress_message};
use super::records::finalize_tool_item;
use super::{
    ToolExecutionError, ToolExecutionOutcome, ToolExecutionRecord, notify_tool_completion,
};
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
        runtime_events: Vec::new(),
        execution_millis: 0,
    }
}

#[test]
fn redacts_secret_user_input_answers_for_display() {
    let arguments = serde_json::json!({
        "questions": [
            { "id": "api_key", "header": "Key", "question": "API key?", "isSecret": true },
            { "id": "mode", "header": "Mode", "question": "Mode?", "isSecret": false }
        ]
    });
    let result = serde_json::json!({
        "answers": {
            "api_key": { "answers": ["sk-secret"] },
            "mode": { "answers": ["Fast"] }
        }
    })
    .to_string();

    let display = redact_user_input_display_result(&arguments, &result);

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&display).unwrap(),
        serde_json::json!({
            "answers": {
                "api_key": { "answers": ["[redacted]"] },
                "mode": { "answers": ["Fast"] }
            }
        })
    );
}

#[test]
fn progress_messages_describe_plan_and_subagent_lifecycle() {
    assert_eq!(tool_start_progress_message("plan_exit"), "正在提交计划。");
    assert_eq!(
        tool_terminal_progress_message(&completed_record("plan_exit")),
        "计划已生成，等待确认。"
    );
    assert_eq!(
        tool_start_progress_message("spawn_agent"),
        "正在创建子代理。"
    );
    assert_eq!(
        tool_terminal_progress_message(&completed_record("spawn_agent")),
        "子代理已创建。"
    );
    assert_eq!(
        tool_start_progress_message("read_file"),
        "正在执行工具 `read_file`。"
    );
    assert_eq!(
        tool_terminal_progress_message(&completed_record("read_file")),
        "工具 `read_file` 已完成。"
    );
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
    let options = crate::TurnOptions::default().with_tool_completion_callback(std::sync::Arc::new(
        move |completion| {
            let callback_observed = callback_observed.clone();
            Box::pin(async move {
                callback_observed.lock().unwrap().push(completion);
                Ok(())
            })
        },
    ));
    let mut record = completed_record("exec");
    record.result = "generated file".to_string();
    record.exit_code = Some(0);

    notify_tool_completion(&options, &record).await.unwrap();

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
    let options =
        crate::TurnOptions::default().with_tool_completion_callback(std::sync::Arc::new(|_| {
            Box::pin(async { anyhow::bail!("fingerprint failed") })
        }));

    let error = notify_tool_completion(&options, &completed_record("apply_patch"))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        ToolExecutionError::Fatal(
            "host post-tool observation failed after apply_patch: fingerprint failed".to_string()
        )
    );
}
