//! 工具调用与 web search 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TracePart, TracePartAction, TracePartCompletion, TraceToolInvocation,
    TraceToolOutput,
};

use crate::completion::ToolCall;
use crate::completion::WebSearchAction;

use super::super::tool_stream::ToolCallAccumulatorSnapshot;
use super::TraceProjection;

impl TraceProjection {
    pub(crate) fn start_tool(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> Vec<AgentEvent> {
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
        self.start_item(
            item_id,
            pl_trace::TracePartState::Tool(pl_trace::TraceToolPart::streaming(invocation)),
        )
    }

    pub(crate) fn start_web_search(
        &mut self,
        provider_item_id: &str,
        action: WebSearchAction,
    ) -> Vec<AgentEvent> {
        let item_id =
            self.resolve_tool_item_id(vec![provider_item_id.to_string()], provider_item_id);
        let invocation = TraceToolInvocation::new(
            item_id.clone(),
            "web_search".to_owned(),
            web_search_arguments(&action),
        )
        .with_provider_identity(
            None,
            (!provider_item_id.is_empty()).then(|| provider_item_id.to_owned()),
        );
        if self.started.contains_key(&item_id) {
            return self.update_invocation(&item_id, invocation);
        }
        self.start_item(
            item_id,
            pl_trace::TracePartState::Tool(pl_trace::TraceToolPart::streaming(invocation)),
        )
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
        if self
            .started
            .get(&item_id)
            .is_none_or(TracePart::is_terminal)
        {
            return events;
        }
        let artifacts =
            vec![serde_json::json!({"kind": "webSearch", "action": action, "results": results})];
        let output = TraceToolOutput::new(String::new()).with_details(
            None,
            Vec::new(),
            artifacts,
            Vec::new(),
            None,
        );
        events.extend(self.apply_item(
            &item_id,
            TracePartAction::Complete(TracePartCompletion::Tool { output }),
        ));
        events
    }

    fn update_invocation(
        &mut self,
        item_id: &str,
        invocation: TraceToolInvocation,
    ) -> Vec<AgentEvent> {
        let Some(item) = self.started.get(item_id) else {
            return Vec::new();
        };
        if item.is_terminal()
            || item
                .tool()
                .is_some_and(|tool| tool.invocation() == &invocation)
        {
            return Vec::new();
        }
        self.apply_item(
            item_id,
            TracePartAction::UpdateToolInvocation { invocation },
        )
    }

    pub(crate) fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let item_id = self.active_tool_item_id(snapshot);
        let mut events = self.start_tool(snapshot);
        let Some(item) = self.started.get(&item_id) else {
            return events;
        };
        let arguments = item
            .tool()
            .map(|tool| tool.invocation().arguments().to_owned())
            .unwrap_or_default();
        let invocation =
            TraceToolInvocation::new(item_id.clone(), snapshot.name.clone(), arguments)
                .with_provider_identity(
                    snapshot.call_id.clone(),
                    (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                );
        events.extend(self.update_invocation(&item_id, invocation));
        if !delta.is_empty() {
            events.extend(self.apply_item(
                &item_id,
                TracePartAction::Append(TraceDelta::ToolArguments { delta }),
            ));
        }
        events
    }

    pub(crate) fn update_tool_trace(&mut self, call: &ToolCall) -> Vec<AgentEvent> {
        let item_id = self.active_tool_call_item_id(call);
        let invocation =
            TraceToolInvocation::new(item_id.clone(), call.name.clone(), call.payload_text())
                .with_provider_identity(
                    Some(call.call_id.clone()),
                    (!call.id.is_empty()).then(|| call.id.clone()),
                );
        if self.started.contains_key(&item_id) {
            if self
                .started
                .get(&item_id)
                .is_some_and(TracePart::is_terminal)
            {
                return Vec::new();
            }
            return self.apply_item(
                &item_id,
                TracePartAction::UpdateToolInvocation { invocation },
            );
        }
        self.start_item(
            item_id,
            pl_trace::TracePartState::Tool(pl_trace::TraceToolPart::started(invocation)),
        )
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

        assert_eq!(updated.item_id(), "inference-1-provider-tool-1");
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

        assert_eq!(first_delta, "inference-1-call-1");
        assert_eq!(second_delta, "inference-1-call-1");
        assert_eq!(updated.item_id(), "inference-1-call-1");
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
    #[test]
    fn retried_tool_call_keeps_provider_identity_but_has_a_new_trace_item() {
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let mut first = trace_with_sink(sink.clone());
        let snapshot = accumulator_snapshot("provider-tool-1", "call-1");
        let first_item = first
            .start_tool(&snapshot)
            .iter()
            .find_map(started_tool_item)
            .unwrap()
            .clone();
        first.fail_attempt("disconnected");
        let mut context = super::super::test_support::test_trace_context("inference-1-retry-1");
        context.turn_id = "turn-1".into();
        let mut second = super::TraceProjection::with_sink(context, Some(sink));
        let second_item = second
            .start_tool(&snapshot)
            .iter()
            .find_map(started_tool_item)
            .unwrap()
            .clone();
        assert_ne!(first_item.item_id(), second_item.item_id());
        assert_eq!(
            first_item.tool().unwrap().invocation().call_id(),
            second_item.tool().unwrap().invocation().call_id()
        );
        assert!(first.take_trace_error().is_none());
        assert!(second.take_trace_error().is_none());
    }
}
