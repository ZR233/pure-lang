use super::tool_history::*;
use pl_model::{CompletionResponse, FinishReason, TokenUsage, ToolCall, ToolCallKind};
use pl_protocol::ThreadPromptSnapshot;

use super::*;
use pretty_assertions::assert_eq;

fn text_message(text: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(text.to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

#[test]
fn push_assistant_tool_calls_stores_metadata() {
    let mut session = AgentSession::new();
    let tool_calls = vec![ToolCall::function(
        "call-1",
        "exec",
        serde_json::json!({"command": "ls"}),
        "call-1",
    )];
    session.push_assistant_tool_calls(Some("running...".to_string()), tool_calls, None);

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::Assistant);
    assert!(session.messages()[0].metadata.contains_key("tool_calls"));
}

#[test]
fn push_assistant_completion_response_adds_text_message() {
    let mut session = AgentSession::new();
    let response = CompletionResponse {
        content: Some("reply".to_string()),
        raw_content: Some("reply".to_string()),
        reasoning_content: Some("thinking".to_string()),
        tool_calls: Vec::new(),
        hosted_web_search_calls: Vec::new(),
        responses_context_items: Vec::new(),
        orchestration: Default::default(),
        trace_events: Vec::new(),
        next_sequence: 0,
        usage: TokenUsage::default(),
        finish_reason: FinishReason::Stop,
        model: "test-model".to_string(),
        response_id: Some("resp-1".to_string()),
    };

    session.push_assistant_completion_response(&response);

    assert_eq!(
        session.messages(),
        &[Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("reply".to_string()),
            reasoning_content: Some("thinking".to_string()),
            metadata: HashMap::new(),
        }]
    );
}

#[test]
fn push_assistant_completion_response_preserves_tool_call_history() {
    let mut session = AgentSession::new();
    let tool_calls = vec![ToolCall::function(
        "call-1",
        "exec",
        serde_json::json!({"command": "pwd"}),
        "call-1",
    )];
    let response = CompletionResponse {
        content: Some("running".to_string()),
        raw_content: Some("running".to_string()),
        reasoning_content: Some("thinking".to_string()),
        tool_calls: tool_calls.clone(),
        hosted_web_search_calls: Vec::new(),
        responses_context_items: Vec::new(),
        orchestration: Default::default(),
        trace_events: Vec::new(),
        next_sequence: 0,
        usage: TokenUsage::default(),
        finish_reason: FinishReason::ToolCalls,
        model: "test-model".to_string(),
        response_id: Some("resp-1".to_string()),
    };

    session.push_assistant_completion_response(&response);

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::Assistant);
    assert_eq!(
        session.messages()[0].content,
        MessageContent::Text("running".to_string())
    );
    assert_eq!(
        session.messages()[0].reasoning_content,
        Some("thinking".to_string())
    );
    let metadata = ToolCallHistoryMetadata::from_metadata(&session.messages()[0].metadata)
        .expect("tool calls");
    assert_eq!(
        metadata.tool_calls_json,
        serde_json::to_string(&tool_calls).expect("tool call json")
    );
}

#[test]
fn push_tool_result_stores_metadata() {
    let mut session = AgentSession::new();
    session.push_tool_result(
        "provider-item-1".to_string(),
        Some("call-1".to_string()),
        "exec".to_string(),
        ToolCallKind::Function,
        "output".to_string(),
        r#"{"command":"echo hi"}"#.to_string(),
    );

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::Tool);
    assert_eq!(
        session.messages()[0].metadata.get("tool_call_id").unwrap(),
        "provider-item-1"
    );
    assert_eq!(
        session.messages()[0]
            .metadata
            .get("tool_call_call_id")
            .unwrap(),
        "call-1"
    );
    assert_eq!(
        session.messages()[0].metadata.get("tool_name").unwrap(),
        "exec"
    );
    assert_eq!(
        session.messages()[0]
            .metadata
            .get("tool_call_kind")
            .unwrap(),
        "function"
    );
    assert_eq!(
        session.messages()[0]
            .metadata
            .get("tool_call_arguments")
            .unwrap(),
        r#"{"command":"echo hi"}"#
    );
}

#[test]
fn snapshot_round_trip_preserves_responses_items_and_program_caller() {
    let mut session = AgentSession::new();
    session.push_responses_context_items(vec![ResponsesContextItem {
        kind: pl_protocol::ResponsesContextItemKind::Program,
        value: serde_json::json!({"type": "program", "id": "program-1"}),
    }]);
    let receipt = ToolResultReceipt {
        call_id: "call-1".to_string(),
        tool_name: "read_file".to_string(),
        arguments_hash: "arguments-hash".to_string(),
        result_hash: "result-hash".to_string(),
        total_bytes: 16,
        visible_bytes: 16,
        truncated: false,
        artifacts: Vec::new(),
        continuation: None,
        reused_from_call_id: None,
    };
    session.push_tool_result_with_receipt_and_caller(
        "fc-1".to_string(),
        Some("call-1".to_string()),
        "read_file".to_string(),
        ToolCallKind::Function,
        r#"{"content":"ok"}"#.to_string(),
        r#"{"path":"README.md"}"#.to_string(),
        receipt,
        Some(ToolCallCaller::Program {
            caller_id: "program-1".to_string(),
        }),
    );

    let encoded = serde_json::to_string(&session.snapshot()).unwrap();
    let snapshot = serde_json::from_str::<AgentSessionSnapshot>(&encoded).unwrap();
    let restored = AgentSession::from_snapshot(snapshot);

    assert!(matches!(
        &restored.items()[0],
        ModelContextItem::Responses { item }
            if item.kind == pl_protocol::ResponsesContextItemKind::Program
    ));
    let metadata = ToolResultMetadata::from_metadata(&restored.messages()[0].metadata)
        .expect("programmatic tool metadata");
    assert_eq!(
        metadata.tool_call_caller,
        Some(ToolCallCaller::Program {
            caller_id: "program-1".to_string()
        })
    );
}

#[test]
fn tool_call_history_message_parses_arguments_into_metadata() {
    let message = tool_call_history_message(
        "call-1".to_string(),
        "read_file".to_string(),
        r#"{"path":"README.md"}"#.to_string(),
    );

    assert_eq!(message.role, MessageRole::Assistant);
    let metadata = ToolCallHistoryMetadata::from_metadata(&message.metadata).expect("tool calls");
    let value: serde_json::Value =
        serde_json::from_str(&metadata.tool_calls_json).expect("tool call json");
    assert_eq!(value[0]["id"], "call-1");
    assert_eq!(value[0]["name"], "read_file");
    assert_eq!(value[0]["payload"]["arguments"]["path"], "README.md");
}

#[test]
fn tool_result_history_message_stores_result_metadata() {
    let message = tool_result_history_message(
        "call-1".to_string(),
        "read_file".to_string(),
        r#"{"path":"README.md"}"#.to_string(),
        "ok".to_string(),
    );

    assert_eq!(message.role, MessageRole::Tool);
    assert_eq!(message.content, MessageContent::Text("ok".to_string()));
    let metadata =
        ToolResultMetadata::from_metadata(&message.metadata).expect("tool result metadata");
    assert_eq!(metadata.tool_call_id, "call-1");
    assert_eq!(metadata.tool_name, "read_file");
    assert_eq!(
        metadata.tool_call_arguments.as_deref(),
        Some(r#"{"path":"README.md"}"#)
    );
}

#[test]
fn from_messages_preserves_order() {
    let msgs = vec![
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("q".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("a".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
    ];
    let session = AgentSession::from_messages(msgs.clone());
    assert_eq!(session.len(), 2);
    assert_eq!(session.messages()[0].role, MessageRole::User);
    assert_eq!(session.messages()[1].role, MessageRole::Assistant);
}

#[test]
fn child_fork_excludes_open_and_completed_tool_protocol_messages() {
    let mut parent = AgentSession::new();
    parent.push_user_prompt("implement".to_string());
    parent.push_assistant_response("working".to_string(), None);
    parent.push_assistant_tool_calls(
        None,
        vec![ToolCall::function(
            "call-1",
            "task_request_delivery_review",
            serde_json::json!({}),
            "call-1",
        )],
        None,
    );
    parent.push_tool_result(
        "call-1".to_string(),
        Some("call-1".to_string()),
        "task_request_delivery_review".to_string(),
        ToolCallKind::Function,
        "ok".to_string(),
        "{}".to_string(),
    );

    let child = parent.fork(AgentSessionForkPolicy::AllMessages);

    assert_eq!(
        child
            .messages()
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        vec![MessageRole::User, MessageRole::Assistant]
    );
    assert!(
        child
            .messages()
            .iter()
            .all(|message| message.metadata.is_empty())
    );
}

#[test]
fn from_items_preserves_checkpoint_order_but_message_view_filters_it() {
    let user = text_message("retained user");
    let items = vec![
        ModelContextItem::from(user.clone()),
        ModelContextItem::Compaction {
            encrypted_content: "encrypted".to_string(),
        },
    ];

    let session = AgentSession::from_items(items.clone());

    assert_eq!(session.items(), items.as_slice());
    assert_eq!(session.messages(), &[user]);
    assert_eq!(session.len(), 2);
}

#[test]
fn replace_items_increments_revision_and_preserves_prompt_cache_key() {
    let mut session = AgentSession::from_messages(vec![text_message("old")]);
    session.set_prompt_cache_key("cache-1".to_string());
    let original_revision = session.revision();

    session.replace_items(vec![ModelContextItem::Compaction {
        encrypted_content: "encrypted".to_string(),
    }]);

    assert_eq!(session.revision(), original_revision + 1);
    assert_eq!(session.prompt_cache_key(), Some("cache-1"));
    assert!(session.messages().is_empty());
}

#[test]
fn context_recovery_starts_a_new_prompt_generation_and_drops_cache_continuation() {
    let mut session = AgentSession::from_messages(vec![text_message("retained")]);
    session.set_prompt_cache_key("old-cache".to_string());
    session.replace_prompt_metadata(ThreadPromptMetadata {
        active_scope: "executor".to_string(),
        slots: [(
            "executor".to_string(),
            ThreadPromptSnapshot {
                scope: "executor".to_string(),
                generation: 4,
                provider: "local".to_string(),
                provider_hash: "provider".to_string(),
                model: "model".to_string(),
                fixed_prefix_hash: "fixed".to_string(),
                fixed_prefix_section_hashes: Default::default(),
                request_properties_hash: "request".to_string(),
                tool_schema_hash: "tools".to_string(),
                tool_catalog_hash: None,
                registry_revision: None,
                context_hash: "context".to_string(),
                prompt_cache_policy: "session".to_string(),
                prefix_changed_reason: PromptPrefixChangedReason::Initial,
                updated_at: 1,
            },
        )]
        .into_iter()
        .collect(),
    });

    session.mark_context_recovered(9);

    let prompt = &session.prompt_metadata().slots["executor"];
    assert_eq!(prompt.generation, 5);
    assert_eq!(
        prompt.prefix_changed_reason,
        PromptPrefixChangedReason::ContextRecovered
    );
    assert_eq!(prompt.updated_at, 9);
    assert_eq!(session.prompt_cache_key(), None);
    assert_eq!(session.messages(), &[text_message("retained")]);
}

#[test]
fn replace_messages_updates_history_and_revision() {
    let mut session = AgentSession::new();
    session.push_user_prompt("old".to_string());
    let note = session_note(3, "durable");
    session.replace_session_note(note.clone());
    let original_revision = session.revision();
    let messages = vec![Message {
        role: MessageRole::User,
        content: MessageContent::Text("summary".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }];

    session.replace_messages(messages.clone());

    assert_eq!(session.revision(), original_revision + 1);
    assert_eq!(session.messages(), messages.as_slice());
    assert_eq!(session.session_note(), Some(&note));
}

#[test]
fn truncate_messages_keeps_prefix_and_invalidates_history_revision() {
    let mut session = AgentSession::new();
    session.push_user_prompt("first".to_string());
    session.push_assistant_response("second".to_string(), None);
    let note = session_note(4, "survives truncation");
    session.replace_session_note(note.clone());
    let original_revision = session.revision();

    session.truncate_messages(1);

    assert_eq!(session.revision(), original_revision + 1);
    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::User);
    assert_eq!(
        session.messages()[0].content,
        MessageContent::Text("first".to_string())
    );
    assert_eq!(session.session_note(), Some(&note));
}

#[test]
fn compaction_preserves_note_and_child_forks_do_not_inherit_it() {
    let mut parent = AgentSession::from_messages(vec![text_message("before")]);
    let note = session_note(7, "important checkpoint");
    parent.replace_session_note(note.clone());

    parent.replace_compactable_items(vec![ModelContextItem::from(text_message("summary"))]);
    let child = parent.fork(AgentSessionForkPolicy::AllMessages);

    assert_eq!(parent.session_note(), Some(&note));
    assert_eq!(parent.messages(), &[text_message("summary")]);
    assert_eq!(child.messages(), &[text_message("summary")]);
    assert_eq!(child.session_note(), None);
}

#[test]
fn clone_shares_state_until_first_write() {
    let mut original = AgentSession::from_messages(vec![text_message("shared")]);
    let mut cloned = original.clone();

    assert!(Arc::ptr_eq(&original.state, &cloned.state));

    cloned.push_user_prompt("copy-on-write".to_string());

    assert!(!Arc::ptr_eq(&original.state, &cloned.state));
    assert_eq!(original.messages(), &[text_message("shared")]);
    assert_eq!(cloned.messages().len(), 2);

    original.set_prompt_cache_key("original-cache".to_string());
    assert_eq!(original.prompt_cache_key(), Some("original-cache"));
    assert_eq!(cloned.prompt_cache_key(), None);
}

fn session_note(revision: u64, content: &str) -> SessionNote {
    SessionNote {
        revision,
        content: content.to_string(),
        content_hash: canonical_content_hash(content.as_bytes()),
        updated_at: 1,
    }
}

#[test]
fn repair_incomplete_tool_history_inserts_missing_result_before_next_user_message() {
    let mut session = AgentSession::new();
    session.push_assistant_tool_calls(
        None,
        vec![ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({"command": "pwd"}),
            "call-1",
        )],
        None,
    );
    let mut history = session.messages().to_vec();
    history.push(text_message("continue"));

    assert!(repair_incomplete_tool_history(&mut history));

    assert_eq!(history.len(), 3);
    assert_eq!(history[1].role, MessageRole::Tool);
    let metadata = ToolResultMetadata::from_metadata(&history[1].metadata).expect("tool metadata");
    assert_eq!(metadata.tool_call_id, "call-1");
    assert_eq!(metadata.tool_call_call_id.as_deref(), Some("call-1"));
    assert_eq!(metadata.tool_name, "exec");
    assert_eq!(
        metadata.tool_call_arguments.as_deref(),
        Some(r#"{"command":"pwd"}"#)
    );
    assert_eq!(history[2].role, MessageRole::User);
}
