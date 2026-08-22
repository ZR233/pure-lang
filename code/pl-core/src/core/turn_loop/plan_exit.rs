use crate::core::tool_dispatch::ToolExecutionOutcome;
use crate::trace::TraceRecorder;

pub(super) fn record_plan_exit_items(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    tool_results: &[super::super::tool_dispatch::ToolExecutionRecord],
) {
    for tool_result in tool_results {
        if tool_result.name != "plan_exit" || tool_result.outcome != ToolExecutionOutcome::Succeeded
        {
            continue;
        }
        if let Some(content) = plan_exit_content(&tool_result.arguments) {
            let item_id = format!("{turn_id}-plan");
            recorder.complete_plan_item(turn_id, &item_id, content);
        }
    }
}

fn plan_exit_content(arguments: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
    let content = value.get("content")?.as_str()?.trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}
