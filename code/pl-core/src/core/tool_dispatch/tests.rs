use pretty_assertions::assert_eq;

use super::ToolExecutionRecord;
use super::display::redact_user_input_display_result;
use super::progress_messages::{tool_start_progress_message, tool_terminal_progress_message};
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
