//! responses websocket 请求规范化与事件流测试。

use pretty_assertions::assert_eq;
use serde_json::{Map, Value};

use super::{
    IncrementalRequestFallbackReason, canonical_response_history_items, incremental_request,
    normalize_websocket_request_body, responses_websocket_url,
};
use crate::runtime::session::ResponsesWebSocketSession;

#[test]
fn websocket_request_keeps_explicit_empty_tools_for_v2_schema() {
    let mut body = Map::from_iter([
        (
            "previous_response_id".to_string(),
            serde_json::json!("stale"),
        ),
        ("store".to_string(), Value::Bool(true)),
    ]);

    normalize_websocket_request_body(&mut body);

    assert_eq!(body["tools"], serde_json::json!([]));
    assert_eq!(body["store"], Value::Bool(false));
    assert!(!body.contains_key("previous_response_id"));
}

#[test]
fn builds_responses_websocket_url_without_losing_base_path() {
    assert_eq!(
        responses_websocket_url("https://api.openai.com/v1/")
            .unwrap()
            .as_str(),
        "wss://api.openai.com/v1/responses"
    );
    assert_eq!(
        responses_websocket_url("http://127.0.0.1:8080/proxy/v1")
            .unwrap()
            .as_str(),
        "ws://127.0.0.1:8080/proxy/v1/responses"
    );
}

#[test]
fn canonical_history_ignores_reasoning_and_provider_owned_fields() {
    let output = vec![
        serde_json::json!({ "type": "reasoning", "id": "reasoning-1" }),
        serde_json::json!({
            "type": "message",
            "id": "message-1",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "ok",
                "annotations": [],
            }],
        }),
    ];

    assert_eq!(
        canonical_response_history_items(&output),
        vec![serde_json::json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }],
        })]
    );
}

#[test]
fn incremental_request_sends_only_the_strict_suffix() {
    let session = ResponsesWebSocketSession {
        last_request: Some(Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "input".to_string(),
                serde_json::json!([{"role":"user","content":"a"}]),
            ),
        ])),
        last_response_id: Some("response-1".to_string()),
        last_response_items: vec![serde_json::json!({"role":"assistant","content":"b"})],
        ..ResponsesWebSocketSession::default()
    };
    let current = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        (
            "input".to_string(),
            serde_json::json!([
                {"role":"user","content":"a"},
                {"role":"assistant","content":"b"},
                {"role":"user","content":"c"}
            ]),
        ),
    ]);

    let incremental = incremental_request(&session, &current).unwrap();
    assert_eq!(
        incremental["input"],
        serde_json::json!([{"role":"user","content":"c"}])
    );
    assert_eq!(incremental["previous_response_id"], "response-1");
}

#[test]
fn incremental_request_reports_prefix_mismatch() {
    let session = ResponsesWebSocketSession {
        last_request: Some(Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            ("input".to_string(), serde_json::json!(["old-tail"])),
        ])),
        last_response_id: Some("response-1".to_string()),
        ..ResponsesWebSocketSession::default()
    };
    let current = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        ("input".to_string(), serde_json::json!(["new-tail"])),
    ]);

    assert_eq!(
        incremental_request(&session, &current).unwrap_err(),
        IncrementalRequestFallbackReason::InputPrefixMismatch {
            previous_prefix_items: 1,
            first_differing_index: 0,
        }
    );
}

#[test]
fn continuation_reuses_when_request_tools_unchanged_and_native_context_appended() {
    let tools = serde_json::json!([
        {"type": "function", "name": "exec"},
        {"type": "programmatic_tool_calling"}
    ]);
    let user = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": "list the git tools"}],
    });
    let program = serde_json::json!({
        "type": "program",
        "id": "program-1",
    });
    let program_output = serde_json::json!({
        "type": "program_output",
        "id": "program-output-1",
    });
    let next_user = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": "run git status"}],
    });
    let session = ResponsesWebSocketSession {
        last_request: Some(Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            ("tools".to_string(), tools.clone()),
            ("input".to_string(), serde_json::json!([user.clone()])),
        ])),
        last_response_id: Some("response-1".to_string()),
        last_response_items: vec![program.clone(), program_output.clone()],
        ..ResponsesWebSocketSession::default()
    };
    let current = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        ("tools".to_string(), tools),
        (
            "input".to_string(),
            serde_json::json!([user, program, program_output, next_user,]),
        ),
    ]);

    let incremental = incremental_request(&session, &current).unwrap();
    assert_eq!(
        incremental["input"],
        serde_json::json!([next_user]),
        "request tools 未变时，session 上下文追加的原生 item 之后 continuation 只发送严格后缀"
    );
    assert_eq!(incremental["previous_response_id"], "response-1");
}

#[test]
fn continuation_falls_back_when_request_tools_change() {
    let user = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": "continue"}],
    });
    let session = ResponsesWebSocketSession {
        last_request: Some(Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "tools".to_string(),
                serde_json::json!([
                    {"type": "function", "name": "exec"}
                ]),
            ),
            ("input".to_string(), serde_json::json!([user.clone()])),
        ])),
        last_response_id: Some("response-1".to_string()),
        last_response_items: Vec::new(),
        ..ResponsesWebSocketSession::default()
    };
    let current = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        (
            "tools".to_string(),
            serde_json::json!([
                {"type": "function", "name": "exec"},
                {"type": "function", "name": "read_file"}
            ]),
        ),
        ("input".to_string(), serde_json::json!([user])),
    ]);

    assert_eq!(
        incremental_request(&session, &current).unwrap_err(),
        IncrementalRequestFallbackReason::RequestPropertiesChanged
    );
}

#[test]
fn incremental_request_requires_working_context_to_keep_its_turn_anchor() {
    let user = serde_json::json!({
        "type": "message",
        "role": "user",
        "content": [{"type": "input_text", "text": "implement"}],
    });
    let working_context = serde_json::json!({
        "type": "message",
        "role": "developer",
        "content": [{"type": "input_text", "text": "# Current working context"}],
    });
    let tool_call = serde_json::json!({
        "type": "function_call",
        "name": "read_file",
        "arguments": "{\"path\":\"src/lib.rs\"}",
        "call_id": "call-1",
    });
    let tool_result = serde_json::json!({
        "type": "function_call_output",
        "call_id": "call-1",
        "output": "ok",
    });
    let session = ResponsesWebSocketSession {
        last_request: Some(Map::from_iter([
            ("model".to_string(), serde_json::json!("gpt-test")),
            (
                "input".to_string(),
                Value::Array(vec![user.clone(), working_context.clone()]),
            ),
        ])),
        last_response_id: Some("response-1".to_string()),
        last_response_items: vec![tool_call.clone()],
        ..ResponsesWebSocketSession::default()
    };
    let anchored = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        (
            "input".to_string(),
            Value::Array(vec![
                user.clone(),
                working_context.clone(),
                tool_call.clone(),
                tool_result.clone(),
            ]),
        ),
    ]);
    let relocated = Map::from_iter([
        ("model".to_string(), serde_json::json!("gpt-test")),
        (
            "input".to_string(),
            Value::Array(vec![user, tool_call, tool_result.clone(), working_context]),
        ),
    ]);

    let incremental = incremental_request(&session, &anchored).unwrap();
    assert_eq!(incremental["input"], Value::Array(vec![tool_result]));
    assert_eq!(incremental["previous_response_id"], "response-1");
    assert_eq!(
        incremental_request(&session, &relocated).unwrap_err(),
        IncrementalRequestFallbackReason::InputPrefixMismatch {
            previous_prefix_items: 3,
            first_differing_index: 1,
        }
    );
}
