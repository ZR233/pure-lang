use pretty_assertions::assert_eq;

use super::ToolExecutionRecord;
use super::display::redact_user_input_display_result;
use super::progress_messages::{tool_start_progress_message, tool_terminal_progress_message};
use super::records::finalize_tool_item;
use pl_protocol::ToolCallKind;
use pl_trace::TracePartStatus;

fn completed_record(name: &str) -> ToolExecutionRecord {
    ToolExecutionRecord {
        id: "item-1".to_string(),
        call_id: None,
        name: name.to_string(),
        kind: ToolCallKind::Function,
        result: String::new(),
        display_result: String::new(),
        arguments: "{}".to_string(),
        status: TracePartStatus::Completed,
        exit_code: None,
        timed_out: false,
        revision: None,
        runtime_events: Vec::new(),
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
        tool_start_progress_message("wait_agent"),
        "正在等待子代理。"
    );
    assert_eq!(
        tool_terminal_progress_message(&completed_record("wait_agent")),
        "子代理等待已结束。"
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
fn finalize_tool_item_carries_output_artifacts() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut recorder = crate::TraceRecorder::new("session".to_string(), event_tx, 0);
    let item = recorder.tool_item(
        "turn-1",
        "turn-1-call-1",
        "container_exec".to_string(),
        "{}".to_string(),
        Some("call-1".to_string()),
        None,
    );
    let mut record = completed_record("container_exec");
    record
        .runtime_events
        .push(crate::tool::ToolRuntimeEvent::OutputArtifacts {
            artifacts: vec![serde_json::json!({
                "id": "artifact-1",
                "stream": "stdout",
            })],
        });
    record
        .runtime_events
        .push(crate::tool::ToolRuntimeEvent::OutputMetrics {
            raw_bytes: 20_000,
            model_visible_bytes: 12_000,
            artifact_bytes: 8_000,
            result_hash: "result-hash".to_string(),
        });
    record
        .runtime_events
        .push(crate::tool::ToolRuntimeEvent::CacheHit {
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
            | pl_trace::TraceEventKind::PlanLifecycleChanged { .. }
            | pl_trace::TraceEventKind::InteractionChanged { .. }
            | pl_trace::TraceEventKind::SkillActivated { .. }
            | pl_trace::TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("completed tool item");

    assert_eq!(
        completed.tool.as_ref().unwrap().output_artifacts,
        vec![serde_json::json!({
            "id": "artifact-1",
            "stream": "stdout",
        })]
    );
    assert_eq!(
        completed.tool.as_ref().unwrap().output_metrics,
        Some(pl_trace::TraceToolOutputMetrics {
            raw_bytes: 20_000,
            model_visible_bytes: 12_000,
            artifact_bytes: 8_000,
            result_hash: "result-hash".to_string(),
            cache_hit: true,
        })
    );
}
