//! 工具调用与 web search 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TraceEventKind, TracePart, TracePartAction, TracePartCompletion,
    TraceToolInvocation, TraceToolOutput, TraceToolState,
};

use crate::WebSearchAction;
use crate::completion::ToolCall;

use super::super::tool_stream::ToolCallAccumulatorSnapshot;
use super::TraceProjection;
use super::unix_seconds;

impl TraceProjection {
    pub(crate) fn start_tool(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_item_id(snapshot);
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        let invocation =
            TraceToolInvocation::new(item_id.clone(), snapshot.name.clone(), String::new())
                .with_provider_identity(
                    snapshot.call_id.clone(),
                    (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                );
        let item = TracePart::streaming_tool(
            self.turn_id.clone(),
            item_id.clone(),
            self.sequence,
            now,
            invocation,
        );
        if !self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now) {
            return Vec::new();
        }
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TracePartStarted { item }]
    }

    pub(crate) fn start_web_search(
        &mut self,
        provider_item_id: &str,
        action: WebSearchAction,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id =
            self.resolve_tool_item_id(vec![provider_item_id.to_string()], provider_item_id);
        if let Some(item) = self.started.get_mut(&item_id) {
            if !item.is_terminal() {
                let invocation = TraceToolInvocation::new(
                    item_id.clone(),
                    "web_search".to_string(),
                    web_search_arguments(&action),
                )
                .with_provider_identity(
                    None,
                    (!provider_item_id.is_empty()).then(|| provider_item_id.to_string()),
                );
                if let Err(error) = item
                    .apply(item.command(now, TracePartAction::UpdateToolInvocation { invocation }))
                {
                    self.trace_error.get_or_insert_with(|| {
                        pl_trace::TraceEventSinkError::new(format!(
                            "failed to update web search trace invocation: {error}"
                        ))
                    });
                }
            }
            return Vec::new();
        }
        let invocation = TraceToolInvocation::new(
            item_id.clone(),
            "web_search".to_string(),
            web_search_arguments(&action),
        )
        .with_provider_identity(
            None,
            (!provider_item_id.is_empty()).then(|| provider_item_id.to_string()),
        );
        let item = TracePart::streaming_tool(
            self.turn_id.clone(),
            item_id.clone(),
            self.sequence,
            now,
            invocation,
        );
        if !self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now) {
            return Vec::new();
        }
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TracePartStarted { item }]
    }

    pub(crate) fn complete_web_search(
        &mut self,
        provider_item_id: &str,
        action: WebSearchAction,
        results: Option<Vec<serde_json::Value>>,
    ) -> Vec<AgentEvent> {
        let mut events = self.start_web_search(provider_item_id, action.clone());
        let item_id =
            self.resolve_tool_item_id(vec![provider_item_id.to_string()], provider_item_id);
        let Some(item) = self.started.get_mut(&item_id) else {
            return events;
        };
        let arguments = web_search_arguments(&action);
        let artifacts = vec![serde_json::json!({
            "kind": "webSearch",
            "action": action,
            "results": results,
        })];
        if item.is_terminal() {
            return events;
        }
        let invocation =
            TraceToolInvocation::new(item_id.clone(), "web_search".to_string(), arguments)
                .with_provider_identity(
                    None,
                    (!provider_item_id.is_empty()).then(|| provider_item_id.to_string()),
                );
        let now = unix_seconds();
        if let Err(error) =
            item.apply(item.command(now, TracePartAction::UpdateToolInvocation { invocation }))
        {
            self.trace_error.get_or_insert_with(|| {
                pl_trace::TraceEventSinkError::new(format!(
                    "failed to finalize web search invocation: {error}"
                ))
            });
            return events;
        }
        let output = TraceToolOutput::new(String::new()).with_details(
            None,
            Vec::new(),
            artifacts,
            Vec::new(),
            None,
        );
        if let Err(error) = item.apply(item.command(
            now,
            TracePartAction::Complete(TracePartCompletion::Tool { output }),
        )) {
            self.trace_error.get_or_insert_with(|| {
                pl_trace::TraceEventSinkError::new(format!(
                    "failed to complete web search trace: {error}"
                ))
            });
            return events;
        }
        let item = item.clone();
        if !self.record(
            TraceEventKind::TracePartCompleted { item: item.clone() },
            item.updated_at(),
        ) {
            return events;
        }
        events.push(AgentEvent::TracePartCompleted { item });
        events
    }

    pub(crate) fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_item_id(snapshot);
        let mut events = self.start_tool(snapshot);
        let metadata_snapshot = {
            let Some(item) = self.started.get_mut(&item_id) else {
                return events;
            };
            let arguments = item
                .tool()
                .map(|tool| tool.invocation().arguments().to_string())
                .unwrap_or_default();
            let invocation =
                TraceToolInvocation::new(item_id.clone(), snapshot.name.clone(), arguments)
                    .with_provider_identity(
                        snapshot.call_id.clone(),
                        (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                    );
            match item
                .apply(item.command(now, TracePartAction::UpdateToolInvocation { invocation }))
            {
                Ok(decision) => decision.changed.then(|| item.clone()),
                Err(error) => {
                    self.trace_error.get_or_insert_with(|| {
                        pl_trace::TraceEventSinkError::new(format!(
                            "failed to update streamed tool invocation: {error}"
                        ))
                    });
                    return events;
                }
            }
        };
        if let Some(item) = metadata_snapshot {
            if !self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now) {
                return events;
            }
            events.push(AgentEvent::TracePartStarted { item });
        }
        if delta.is_empty() {
            return events;
        }
        let trace_delta = TraceDelta::ToolArguments { delta };
        let event = {
            let Some(item) = self.started.get_mut(&item_id) else {
                return events;
            };
            match item.apply_delta(now, trace_delta) {
                Ok(Some(event)) => event,
                Ok(None) => return events,
                Err(error) => {
                    self.trace_error.get_or_insert_with(|| {
                        pl_trace::TraceEventSinkError::new(format!(
                            "failed to append tool arguments trace delta: {error}"
                        ))
                    });
                    return events;
                }
            }
        };
        if !self.record(
            TraceEventKind::TracePartDelta {
                event: event.clone(),
            },
            now,
        ) {
            return events;
        }
        events.push(AgentEvent::TracePartDelta { event });
        events
    }

    pub(crate) fn update_tool_trace(&mut self, call: &ToolCall) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_call_item_id(call);
        let mut inserted = false;
        let item = self.started.entry(item_id.clone()).or_insert_with(|| {
            inserted = true;
            let invocation =
                TraceToolInvocation::new(item_id.clone(), call.name.clone(), call.payload_text())
                    .with_provider_identity(
                        Some(call.call_id.clone()),
                        (!call.id.is_empty()).then(|| call.id.clone()),
                    );
            TracePart::started_tool(
                self.turn_id.clone(),
                item_id.clone(),
                self.sequence,
                now,
                invocation,
            )
        });
        if !inserted
            && matches!(item.tool().map(|tool| tool.state()), Some(state) if !matches!(state, TraceToolState::Succeeded(_) | TraceToolState::Failed(_) | TraceToolState::Denied(_) | TraceToolState::Cancelled(_)))
        {
            let invocation =
                TraceToolInvocation::new(item_id.clone(), call.name.clone(), call.payload_text())
                    .with_provider_identity(Some(call.call_id.clone()), Some(call.id.clone()));
            match item
                .apply(item.command(now, TracePartAction::UpdateToolInvocation { invocation }))
            {
                Ok(_) => {}
                Err(error) => {
                    self.trace_error.get_or_insert_with(|| {
                        pl_trace::TraceEventSinkError::new(format!(
                            "failed to update canonical tool trace: {error}"
                        ))
                    });
                }
            }
        }
        let item = item.clone();
        if !self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now) {
            return Vec::new();
        }
        let events = vec![AgentEvent::TracePartStarted { item }];
        events
    }
}

fn web_search_arguments(action: &WebSearchAction) -> String {
    serde_json::to_string(action).unwrap_or_else(|_| "{\"type\":\"other\"}".to_string())
}

pub(super) fn trace_tool_part_id(call_id: Option<&String>, id: &str) -> String {
    if !id.is_empty() {
        return id.to_string();
    }
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| "tool_call".to_string())
}

pub(super) fn tool_aliases(call_id: Option<&String>, id: &str, trace_id: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    push_tool_alias(&mut aliases, trace_id);
    push_tool_alias(&mut aliases, id);
    if let Some(call_id) = call_id {
        push_tool_alias(&mut aliases, call_id);
    }
    aliases
}

fn push_tool_alias(aliases: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !aliases.iter().any(|alias| alias == value) {
        aliases.push(value.to_string());
    }
}
