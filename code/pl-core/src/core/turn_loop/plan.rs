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
            if let crate::tool::ToolRuntimeEvent::PlanCompleted { content } = event {
                let item_id = format!("{turn_id}-plan");
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

    fn tool_result(
        outcome: ToolExecutionOutcome,
        runtime_events: Vec<crate::tool::ToolRuntimeEvent>,
    ) -> ToolExecutionRecord {
        ToolExecutionRecord {
            id: "tool-1".to_string(),
            call_id: "call-1".to_string(),
            name: "any_plan_tool".to_string(),
            kind: pl_protocol::ToolCallKind::Function,
            arguments: "{}".to_string(),
            result: "{}".to_string(),
            display_result: "{}".to_string(),
            outcome,
            exit_code: Some(0),
            timed_out: false,
            runtime_events,
            execution_millis: 0,
        }
    }

    #[test]
    fn completed_plan_event_creates_a_plan_item_without_tool_name_coupling() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("thread-1".to_string(), event_tx, 0);
        let results = [tool_result(
            ToolExecutionOutcome::Succeeded,
            vec![crate::tool::ToolRuntimeEvent::PlanCompleted {
                content: "# 计划\n\n- 实现修复".to_string(),
            }],
        )];

        record_plan_items(&mut recorder, "turn-1", &results);

        let item = recorder
            .latest_trace_part("turn-1-plan")
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
            vec![crate::tool::ToolRuntimeEvent::PlanCompleted {
                content: "should not be recorded".to_string(),
            }],
        )];

        record_plan_items(&mut recorder, "turn-1", &results);

        assert!(recorder.latest_trace_part("turn-1-plan").is_none());
    }
}
