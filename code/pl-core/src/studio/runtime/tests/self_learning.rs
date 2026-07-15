use super::*;
use pretty_assertions::assert_eq;

#[test]
fn counts_started_tool_items_for_self_learning_threshold() {
    let mut item = TracePart {
        turn_id: "turn".to_string(),
        item_id: "tool".to_string(),
        started_sequence: 1,
        revision: 0,
        kind: TracePartKind::Tool,
        status: TracePartStatus::Started,
        created_at: 1,
        updated_at: 1,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        usage: None,
    };
    let started = TraceEvent {
        session_id: "session".to_string(),
        sequence: 1,
        timestamp: 1,
        kind: TraceEventKind::TracePartStarted { item: item.clone() },
    };
    item.status = TracePartStatus::Running;
    let running = TraceEvent {
        session_id: "session".to_string(),
        sequence: 2,
        timestamp: 2,
        kind: TraceEventKind::TracePartStarted { item },
    };

    assert_eq!(
        started_tool_snapshot_count(&[started.clone(), running.clone()]),
        2
    );
    assert_eq!(tool_call_count(&[started, running]), 1);
}

#[test]
fn self_learning_runs_only_for_completed_simple_turns() {
    let mut config = crate::config::PureConfig::default();
    config.skills.enabled = true;
    config.skills.auto_learn = true;
    config.skills.auto_learn_min_tool_calls = 0;

    assert!(should_start_self_learning(
        &config,
        CompileMode::Simple,
        &TurnResultStatus::Completed,
        &[],
    ));
    assert!(!should_start_self_learning(
        &config,
        CompileMode::Task,
        &TurnResultStatus::Completed,
        &[],
    ));
}
