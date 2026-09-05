use std::collections::HashMap;

use pl_core::session::tool_history::*;
use pl_model::completion::ToolCall;

use pl_protocol::{
    AgentSessionSnapshot, PromptPrefixChangedReason, ResponsesContextItem, SessionNote,
    ThreadPromptMetadata, ThreadPromptSnapshot, ToolCallCaller, ToolCallKind, ToolResultRecord,
};

use pl_core::*;
use pl_model::completion::CompletionResponse;
use pretty_assertions::assert_eq;

fn text_message(text: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(text.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}

#[test]
fn push_assistant_tool_calls_stores_typed_records() {
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
    let records = session.messages()[0]
        .tool_calls
        .as_ref()
        .expect("tool calls");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].item_id, "call-1");
    assert_eq!(records[0].call_id, "call-1");
    assert_eq!(records[0].name, "exec");
    assert_eq!(records[0].kind, ToolCallKind::Function);
    assert_eq!(records[0].arguments["command"], "ls");
    assert!(session.messages()[0].metadata.is_empty());
}

#[test]
fn push_assistant_completion_response_adds_text_message() {
    let mut session = AgentSession::new();
    let response = CompletionResponse {
        content: Some("reply".to_string()),
        reasoning_content: Some("thinking".to_string()),
        tool_calls: Vec::new(),
        responses_context_items: Vec::new(),
        orchestration: Default::default(),
        timing: None,
        accounting: pl_protocol::InferenceAccounting::default(),
        model: "test-model".to_string(),
        response_id: Some("resp-1".to_string()),
    };

    session.push_assistant_completion_response(&response);

    assert_eq!(
        session.messages(),
        &[Message {
            presentation: Default::default(),
            role: MessageRole::Assistant,
            content: MessageContent::text("reply".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: None,
            tool_result: None,
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
        reasoning_content: Some("thinking".to_string()),
        tool_calls: tool_calls.clone(),
        responses_context_items: Vec::new(),
        orchestration: Default::default(),
        timing: None,
        accounting: pl_protocol::InferenceAccounting::default(),
        model: "test-model".to_string(),
        response_id: Some("resp-1".to_string()),
    };

    session.push_assistant_completion_response(&response);

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::Assistant);
    assert_eq!(
        session.messages()[0].content,
        MessageContent::text("running".to_string())
    );
    assert_eq!(
        session.messages()[0].reasoning_content,
        Some("thinking".to_string())
    );
    let records = session.messages()[0]
        .tool_calls
        .as_ref()
        .expect("tool calls");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].item_id, "call-1");
    assert_eq!(records[0].call_id, "call-1");
    assert_eq!(records[0].name, "exec");
    assert_eq!(records[0].arguments["command"], "pwd");
}

#[test]
fn push_tool_result_stores_typed_record() {
    let mut session = AgentSession::new();
    session.push_tool_result(
        ToolResultRecord {
            item_id: "provider-item-1".to_string(),
            call_id: "call-1".to_string(),
            name: "exec".to_string(),
            kind: ToolCallKind::Function,
        },
        "output".to_string(),
        r#"{"command":"echo hi"}"#.to_string(),
    );

    assert_eq!(session.len(), 1);
    assert_eq!(session.messages()[0].role, MessageRole::Tool);
    let record = session.messages()[0]
        .tool_result
        .as_ref()
        .expect("tool result record");
    assert_eq!(record.item_id, "provider-item-1");
    assert_eq!(record.call_id, "call-1");
    assert_eq!(record.name, "exec");
    assert_eq!(record.kind, ToolCallKind::Function);
    assert!(session.messages()[0].metadata.is_empty());
}

#[test]
fn snapshot_round_trip_preserves_responses_items_and_typed_records() {
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
    session.push_assistant_tool_calls(
        None,
        vec![
            pl_model::completion::ToolCall::function(
                "fc-1",
                "read_file",
                serde_json::json!({"path": "README.md"}),
                "call-1",
            )
            .with_caller(Some(ToolCallCaller::Program {
                caller_id: "program-1".to_string(),
            })),
        ],
        None,
    );
    session.push_tool_result_with_receipt(
        ToolResultRecord {
            item_id: "fc-1".to_string(),
            call_id: "call-1".to_string(),
            name: "read_file".to_string(),
            kind: ToolCallKind::Function,
        },
        r#"{"content":"ok"}"#.to_string(),
        receipt,
    );

    let encoded = serde_json::to_string(&session.snapshot()).unwrap();
    let snapshot = serde_json::from_str::<AgentSessionSnapshot>(&encoded).unwrap();
    let restored = AgentSession::from_snapshot(snapshot);

    assert!(matches!(
        &restored.items()[0],
        ModelContextItem::Responses { item }
            if item.kind == pl_protocol::ResponsesContextItemKind::Program
    ));
    let tool_message = restored
        .messages()
        .iter()
        .find(|message| message.role == MessageRole::Tool)
        .expect("tool result message");
    let record = tool_message
        .tool_result
        .as_ref()
        .expect("typed tool result record");
    assert_eq!(record.item_id, "fc-1");
    assert_eq!(record.call_id, "call-1");
    assert_eq!(record.kind, ToolCallKind::Function);

    let assistant_message = restored
        .messages()
        .iter()
        .find(|message| message.role == MessageRole::Assistant)
        .expect("assistant message");
    let call = assistant_message
        .tool_calls
        .as_ref()
        .and_then(|calls| calls.first())
        .expect("typed tool call record");
    assert_eq!(
        call.caller,
        Some(ToolCallCaller::Program {
            caller_id: "program-1".to_string()
        })
    );
}

#[test]
fn tool_call_history_message_stores_typed_record() {
    let message = tool_call_history_message(
        "call-1".to_string(),
        "read_file".to_string(),
        r#"{"path":"README.md"}"#.to_string(),
    );

    assert_eq!(message.role, MessageRole::Assistant);
    let call = message
        .tool_calls
        .as_ref()
        .and_then(|calls| calls.first())
        .expect("typed tool call record");
    assert_eq!(call.item_id, "call-1");
    assert_eq!(call.call_id, "call-1");
    assert_eq!(call.name, "read_file");
    assert_eq!(call.arguments["path"], "README.md");
}

#[test]
fn tool_result_history_message_stores_result_metadata() {
    let message = tool_result_history_message(
        "call-1".to_string(),
        "read_file".to_string(),
        "ok".to_string(),
    );

    assert_eq!(message.role, MessageRole::Tool);
    assert_eq!(message.content, MessageContent::text("ok".to_string()));
    let record = message
        .tool_result
        .as_ref()
        .expect("typed tool result record");
    assert_eq!(record.item_id, "call-1");
    assert_eq!(record.call_id, "call-1");
    assert_eq!(record.name, "read_file");
    assert_eq!(record.kind, ToolCallKind::Function);
}

#[test]
fn from_messages_preserves_order() {
    let msgs = vec![
        Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text("q".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        },
        Message {
            presentation: Default::default(),
            role: MessageRole::Assistant,
            content: MessageContent::text("a".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
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
            "example_tool",
            serde_json::json!({}),
            "call-1",
        )],
        None,
    );
    parent.push_tool_result(
        ToolResultRecord {
            item_id: "call-1".to_string(),
            call_id: "call-1".to_string(),
            name: "example_tool".to_string(),
            kind: ToolCallKind::Function,
        },
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
fn child_fork_does_not_inherit_parent_plan_state() {
    let mut parent = AgentSession::new();
    parent.push_user_prompt("approved baseline is passed in the spawn task".to_string());
    parent.replace_plan(Some(pl_protocol::AgentSessionPlanState::default()));

    let child = parent.fork(AgentSessionForkPolicy::AllMessages);

    assert!(parent.plan().is_some());
    assert_eq!(child.plan(), None);
}

#[test]
fn batched_plan_replacement_converges_with_actor_stepwise_revisions() {
    use crate::session::plan::{
        AgentSessionPlanConfirmationDecision, AgentSessionPlanMachine,
        AgentSessionPlanResolveCommand, AgentSessionPlanSubmitCommand,
    };

    let mut machine = AgentSessionPlanMachine::default();
    assert!(
        machine
            .submit(AgentSessionPlanSubmitCommand {
                expected_revision: 0,
                plan: "# Plan\n\nInitial plan.".to_string(),
                interaction_id: "plan-initial".to_string(),
                operation_id: "submit-initial".to_string(),
                argument_hash: "submit-initial-hash".to_string(),
                submitted_at: 1,
            })
            .accepted
    );
    assert!(
        machine
            .resolve(AgentSessionPlanResolveCommand {
                expected_revision: 1,
                interaction_id: "plan-initial".to_string(),
                operation_id: "revise-initial".to_string(),
                argument_hash: "revise-initial-hash".to_string(),
                decision: AgentSessionPlanConfirmationDecision::RequestRevision {
                    feedback: "Add integration coverage.".to_string(),
                },
                resolved_at: 2,
            })
            .accepted
    );

    let mut base = AgentSession::new();
    assert!(base.replace_plan(Some(machine.state().clone())));
    let baseline_revision = base.snapshot().working_state.revision;
    let mut actor = base.clone();
    let mut checkpoint = base;

    assert!(
        machine
            .submit(AgentSessionPlanSubmitCommand {
                expected_revision: 2,
                plan: "# Plan\n\nRevised plan with integration coverage.".to_string(),
                interaction_id: "plan-revised".to_string(),
                operation_id: "submit-revised".to_string(),
                argument_hash: "submit-revised-hash".to_string(),
                submitted_at: 3,
            })
            .accepted
    );
    assert!(actor.replace_plan(Some(machine.state().clone())));
    assert!(
        machine
            .resolve(AgentSessionPlanResolveCommand {
                expected_revision: 3,
                interaction_id: "plan-revised".to_string(),
                operation_id: "approve-revised".to_string(),
                argument_hash: "approve-revised-hash".to_string(),
                decision: AgentSessionPlanConfirmationDecision::Approve,
                resolved_at: 4,
            })
            .accepted
    );
    let approved = machine.into_state();
    assert!(actor.replace_plan(Some(approved.clone())));
    assert!(checkpoint.replace_plan(Some(approved)));

    assert_eq!(
        actor.snapshot().working_state.revision,
        baseline_revision + 2
    );
    assert_eq!(
        checkpoint.snapshot().working_state.revision,
        actor.snapshot().working_state.revision,
        "a checkpoint that batches the same Plan transitions must not regress the actor-owned working-state revision"
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
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text("summary".to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }];

    session.replace_messages(messages.clone());

    assert_eq!(session.revision(), original_revision + 1);
    assert_eq!(session.messages(), messages.as_slice());
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
fn deferred_tool_reveals_persist_in_session_but_child_forks_start_empty() {
    let mut parent = AgentSession::from_messages(vec![text_message("before")]);
    assert!(parent.replace_tool_discovery(ToolDiscoveryState {
        catalog_fingerprint: Some("catalog-v1".to_string()),
        revealed_tool_names: vec![
            "mcp__write".to_string(),
            "mcp__read".to_string(),
            "mcp__read".to_string(),
        ],
    }));
    assert_eq!(
        parent.tool_discovery(),
        &ToolDiscoveryState {
            catalog_fingerprint: Some("catalog-v1".to_string()),
            revealed_tool_names: vec!["mcp__read".to_string(), "mcp__write".to_string()],
        }
    );

    let restored = AgentSession::from_snapshot(parent.snapshot());
    let child = parent.fork(AgentSessionForkPolicy::AllMessages);

    assert_eq!(restored.tool_discovery(), parent.tool_discovery());
    assert_eq!(child.tool_discovery(), &ToolDiscoveryState::default());
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
    let record = history[1]
        .tool_result
        .as_ref()
        .expect("synthetic typed tool result record");
    assert_eq!(record.item_id, "call-1");
    assert_eq!(record.call_id, "call-1");
    assert_eq!(record.name, "exec");
    assert_eq!(record.kind, ToolCallKind::Function);
    assert_eq!(
        history[1].content,
        MessageContent::text("error: tool execution interrupted".to_string())
    );
    assert_eq!(history[2].role, MessageRole::User);
}
