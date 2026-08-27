use crate::core::tool_dispatch::ToolExecutionOutcome;
use crate::trace::TraceRecorder;

pub(super) fn record_plan_items(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    tool_results: &[super::super::tool_dispatch::ToolExecutionRecord],
) {
    for tool_result in tool_results {
        if tool_result.outcome != ToolExecutionOutcome::Succeeded {
            continue;
        }
        for event in &tool_result.runtime_events {
            if let crate::tool::ToolDirective::PlanCompleted { content } = event {
                let item_id = pl_trace::plan_trace_part_id(&tool_result.trace_part_id);
                recorder.complete_plan_item(turn_id, &item_id, content.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pl_trace::TracePartState;

    use super::*;
    use crate::core::tool_dispatch::ToolExecutionRecord;

    fn tool_result_with_identity(
        id: &str,
        call_id: &str,
        trace_part_id: &str,
        outcome: ToolExecutionOutcome,
        runtime_events: Vec<crate::tool::ToolDirective>,
    ) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: id.to_string(),
            call_id: call_id.to_string(),
            trace_part_id: trace_part_id.to_string(),
            name: "any_plan_tool".to_string(),
            kind: pl_protocol::ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "{}".to_string(),
            display_result: "{}".to_string(),
            outcome,
            exit_code: Some(0),
            timed_out: false,
            model_attachments: Vec::new(),
            runtime_events,
            execution_millis: 0,
        }
    }

    fn tool_result(
        outcome: ToolExecutionOutcome,
        runtime_events: Vec<crate::tool::ToolDirective>,
    ) -> ToolExecutionRecord {
        tool_result_with_identity("tool-1", "call-1", "turn-1-tool-1", outcome, runtime_events)
    }

    #[test]
    fn completed_plan_event_creates_a_plan_item_without_tool_name_coupling() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("thread-1".to_string(), event_tx, 0);
        let results = [tool_result(
            ToolExecutionOutcome::Succeeded,
            vec![crate::tool::ToolDirective::PlanCompleted {
                content: "# 计划\n\n- 实现修复".to_string(),
            }],
        )];

        record_plan_items(&mut recorder, "turn-1", &results);

        let item = recorder
            .latest_trace_part("turn-1-tool-1:plan")
            .expect("completed plan should create a trace item");
        assert!(matches!(
            item.state(),
            TracePartState::Plan(plan) if plan.content() == "# 计划\n\n- 实现修复"
        ));
    }

    #[test]
    fn failed_tool_cannot_create_a_plan_item() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("thread-1".to_string(), event_tx, 0);
        let results = [tool_result(
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
            vec![crate::tool::ToolDirective::PlanCompleted {
                content: "should not be recorded".to_string(),
            }],
        )];

        record_plan_items(&mut recorder, "turn-1", &results);

        assert!(recorder.latest_trace_part("turn-1-tool-1:plan").is_none());
    }

    #[test]
    fn streamed_plan_completion_reuses_the_dispatch_trace_identity() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("thread-1".to_string(), event_tx, 0);
        let trace_part_id = "turn-1-call-1";
        let plan_item_id = pl_trace::plan_trace_part_id(trace_part_id);
        recorder.start_item(pl_trace::TracePart::started_plan(
            "turn-1".to_string(),
            plan_item_id.clone(),
            0,
            0,
        ));
        let results = [tool_result_with_identity(
            "provider-item-1",
            "call-1",
            trace_part_id,
            ToolExecutionOutcome::Succeeded,
            vec![crate::tool::ToolDirective::PlanCompleted {
                content: "# 计划\n\n- 实现修复".to_string(),
            }],
        )];

        record_plan_items(&mut recorder, "turn-1", &results);

        let item = recorder
            .latest_trace_part(&plan_item_id)
            .expect("streamed plan should complete the original trace item");
        assert!(item.is_terminal());
        assert_eq!(item.revision(), 1);
        assert!(recorder.latest_trace_part("provider-item-1:plan").is_none());
        assert!(recorder.latest_trace_part("call-1:plan").is_none());
    }

    #[test]
    fn streamed_plan_allows_the_original_turn_to_complete_normally() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("thread-1".to_string(), event_tx, 0);
        let turn_item = recorder.running_turn_item("turn-1");
        recorder.start_item(turn_item);
        let trace_part_id = "turn-1-call-1";
        recorder.start_item(pl_trace::TracePart::started_plan(
            "turn-1".to_string(),
            pl_trace::plan_trace_part_id(trace_part_id),
            1,
            0,
        ));
        record_plan_items(
            &mut recorder,
            "turn-1",
            &[tool_result_with_identity(
                "provider-item-1",
                "call-1",
                trace_part_id,
                ToolExecutionOutcome::Succeeded,
                vec![crate::tool::ToolDirective::PlanCompleted {
                    content: "# 计划\n\n- 实现修复".to_string(),
                }],
            )],
        );

        let result = super::super::completion::finish(
            &mut recorder,
            "turn-1",
            super::super::completion::CompletedTurn {
                content: String::new(),
                reasoning_content: None,
                model: "test".to_string(),
                usage: pl_model::TokenUsage::default(),
                last_context_tokens: None,
                context_compactions: Vec::new(),
                session_message_count: 0,
                completion: pl_protocol::TurnCompletion::InteractionRequested,
            },
        );

        assert!(matches!(
            result.outcome,
            pl_protocol::TurnOutcome::Completed(_)
        ));
        let plan_terminal_ids = result
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                pl_trace::TraceEventKind::TracePartCompleted { item }
                    if item.kind() == pl_trace::TracePartKind::Plan =>
                {
                    Some(item.item_id())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(plan_terminal_ids, ["turn-1-call-1:plan"]);
    }
}
