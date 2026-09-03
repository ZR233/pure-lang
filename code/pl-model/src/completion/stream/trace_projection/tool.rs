//! 工具调用与 web search 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TraceEventKind, TracePart, TracePartAction, TracePartCompletion,
    TraceToolInvocation, TraceToolOutput, TraceToolState,
};

use crate::completion::ToolCall;
use crate::completion::WebSearchAction;

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pl_trace::{TraceEventKind, TracePartKind, TraceToolState};

    use crate::completion::{ToolCall, ToolCallPayload};

    use super::super::test_support::{
        TracePartEvent, started_tool_item, trace, trace_part_event, trace_with_sink,
    };
    use super::ToolCallAccumulatorSnapshot;

    fn accumulator_snapshot(id: &str, trace_id: &str) -> ToolCallAccumulatorSnapshot {
        ToolCallAccumulatorSnapshot {
            id: id.to_string(),
            trace_id: trace_id.to_string(),
            call_id: Some("call-1".to_string()),
            name: "exec".to_string(),
        }
    }

    fn canonical_tool_call() -> ToolCall {
        ToolCall {
            id: "provider-tool-1".to_string(),
            call_id: "call-1".to_string(),
            name: "exec".to_string(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::json!({"cmd": "echo hi"}),
            },
            invalid_arguments: None,
            caller: None,
        }
    }

    fn tool_delta_item_id(event: &pl_trace::AgentEvent) -> Option<String> {
        match trace_part_event(event)? {
            TracePartEvent::Delta {
                item_id,
                kind: TracePartKind::Tool,
            } => Some(item_id.to_string()),
            _ => None,
        }
    }

    #[test]
    fn update_tool_trace_keeps_streaming_tool_status_after_arguments_delta() {
        let mut trace = trace();
        let snapshot = accumulator_snapshot("provider-tool-1", "provider-tool-1");
        let _ = trace.append_tool_arguments_delta(&snapshot, "{\"cmd\":\"ec".to_string());
        let updated_events = trace.update_tool_trace(&canonical_tool_call());
        let updated = updated_events
            .iter()
            .find_map(started_tool_item)
            .expect("updated tool snapshot");

        assert_eq!(updated.item_id(), "turn-1-provider-tool-1");
        assert!(matches!(
            updated.tool().map(|tool| tool.state()),
            Some(TraceToolState::Streaming(_))
        ));
        assert_eq!(updated.revision(), 2);
        let tool = updated.tool().expect("tool metadata");
        assert_eq!(tool.invocation().arguments(), "{\"cmd\":\"echo hi\"}");
    }

    #[test]
    fn late_provider_tool_id_keeps_original_trace_part_id() {
        let mut trace = trace();
        let early = accumulator_snapshot("call-1", "call-1");
        let late = accumulator_snapshot("provider-tool-1", "call-1");

        let first_delta = trace
            .append_tool_arguments_delta(&early, "{\"cmd\":\"ec".to_string())
            .iter()
            .find_map(tool_delta_item_id)
            .expect("first tool delta");
        let second_delta = trace
            .append_tool_arguments_delta(&late, "ho hi\"}".to_string())
            .iter()
            .find_map(tool_delta_item_id)
            .expect("second tool delta");
        let updated_events = trace.update_tool_trace(&canonical_tool_call());
        let updated = updated_events
            .iter()
            .find_map(started_tool_item)
            .expect("updated tool snapshot");

        assert_eq!(first_delta, "turn-1-call-1");
        assert_eq!(second_delta, "turn-1-call-1");
        assert_eq!(updated.item_id(), "turn-1-call-1");
        assert_eq!(updated.revision(), 3);
        let tool = updated.tool().expect("tool metadata");
        assert_eq!(
            tool.invocation().provider_item_id(),
            Some("provider-tool-1")
        );
        assert_eq!(tool.invocation().call_id(), Some("call-1"));
    }

    #[test]
    fn tool_metadata_and_argument_deltas_share_one_revision_chain() {
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let mut trace = trace_with_sink(sink.clone());
        let early = accumulator_snapshot("call-1", "call-1");
        let late = accumulator_snapshot("provider-tool-1", "call-1");

        let first = trace.append_tool_arguments_delta(&early, "{\"cmd\":\"ec".to_string());
        let ignored = trace.append_tool_arguments_delta(&early, String::new());
        let second = trace.append_tool_arguments_delta(&late, "ho hi\"}".to_string());
        let canonical = trace.update_tool_trace(&canonical_tool_call());

        assert!(ignored.is_empty());
        assert!(first.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartDelta { event } if event.revision == 1
        )));
        assert!(second.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartStarted { item } if item.revision() == 2
        )));
        assert!(second.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartDelta { event } if event.revision == 3
        )));
        assert!(canonical.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartStarted { item } if item.revision() == 3
        )));
        assert!(trace.take_trace_error().is_none());
        assert_eq!(
            sink.events()
                .into_iter()
                .filter_map(|event| match event.kind {
                    TraceEventKind::TracePartStarted { item } =>
                        Some(("snapshot", item.revision())),
                    TraceEventKind::TracePartDelta { event } => Some(("delta", event.revision)),
                    TraceEventKind::TracePartCompleted { .. }
                    | TraceEventKind::TracePartFailed { .. }
                    | TraceEventKind::InteractionChanged { .. }
                    | TraceEventKind::SkillActivated { .. }
                    | TraceEventKind::EnabledToolsRecorded { .. } => None,
                })
                .collect::<Vec<_>>(),
            vec![
                ("snapshot", 0),
                ("delta", 1),
                ("snapshot", 2),
                ("delta", 3),
                ("snapshot", 3),
            ]
        );
    }
}
