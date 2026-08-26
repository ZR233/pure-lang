use pl_trace::{EnabledToolsEvent, TraceEventKind};

use crate::trace::TraceRecorder;
pub(in crate::core) fn record_enabled_tools(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    step: u32,
    plan: &crate::tool::ToolPlan,
) {
    let tools = plan.names().map(ToOwned::to_owned).collect();
    recorder.record_trace_only(TraceEventKind::EnabledToolsRecorded {
        event: EnabledToolsEvent {
            turn_id: turn_id.to_string(),
            step,
            tools,
            wire_fingerprint: plan.wire_fingerprint().to_string(),
            execution_fingerprint: plan.execution_fingerprint().to_string(),
        },
    });
}
