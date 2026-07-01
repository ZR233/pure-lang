use pl_trace::{EnabledToolsEvent, TraceEventKind};

use crate::trace::TraceRecorder;
use crate::turn::CompileMode;

pub(in crate::core) fn record_enabled_tools(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: CompileMode,
    tool_schemas: &[pl_model::ToolSchema],
) {
    let tools = tool_schemas
        .iter()
        .map(pl_model::ToolSchema::name)
        .map(ToOwned::to_owned)
        .collect();
    recorder.record_trace_only(TraceEventKind::EnabledToolsRecorded {
        event: EnabledToolsEvent {
            turn_id: turn_id.to_string(),
            mode: mode.label().to_string(),
            tools,
        },
    });
}
