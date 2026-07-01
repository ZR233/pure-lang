use super::*;

#[test]
fn detects_explicit_subagent_partition_requests() {
    assert!(prompt_requires_subagent_dispatch(
        "每个 crate 分一个 subagent 探索，然后介绍整个项目"
    ));
    assert!(prompt_requires_subagent_dispatch(
        "请分别用子代理探索前端和后端"
    ));
    assert!(!prompt_requires_subagent_dispatch("介绍整个项目"));
    assert!(!prompt_requires_subagent_dispatch(
        "用 bash 看一下每个 crate"
    ));
    assert!(!prompt_requires_subagent_dispatch(
        "读取 src/tool/subagent.rs，并总结每个模块的职责"
    ));
}

#[test]
fn subagent_dispatch_instructions_describe_recoverable_429() {
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("429"));
    assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("recoverableSubagentProvider429"));
    assert!(SUBAGENT_FORCE_DISPATCH_INSTRUCTION.contains("429"));
}

#[test]
fn detects_recoverable_subagent_tool_result_marker() {
    let records = vec![
        ToolExecutionRecord {
            id: "item-1".to_string(),
            call_id: Some("call-1".to_string()),
            name: "spawn_agent".to_string(),
            kind: ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "recoverableSubagentProvider429: retry locally".to_string(),
            display_result: "recoverableSubagentProvider429: retry locally".to_string(),
            status: TracePartStatus::Completed,
            exit_code: None,
            timed_out: false,
            revision: None,
            runtime_events: Vec::new(),
        },
        ToolExecutionRecord {
            id: "item-2".to_string(),
            call_id: Some("call-2".to_string()),
            name: "bash".to_string(),
            kind: ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "recoverableSubagentProvider429: unrelated text".to_string(),
            display_result: "recoverableSubagentProvider429: unrelated text".to_string(),
            status: TracePartStatus::Completed,
            exit_code: None,
            timed_out: false,
            revision: None,
            runtime_events: Vec::new(),
        },
    ];

    assert!(tool_results_include_recoverable_subagent_capacity(&records));
    assert!(!tool_results_include_recoverable_subagent_capacity(
        &records[1..]
    ));
}
