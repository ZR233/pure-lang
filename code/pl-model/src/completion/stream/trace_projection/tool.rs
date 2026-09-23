//! 工具调用与 web search 流的 trace part 投影。

use pl_protocol::trace::{
    AgentEvent, TraceDelta, TracePart, TracePartAction, TracePartCompletion, TraceToolInvocation,
    TraceToolOutput,
};

use crate::completion::ToolCall;
use pl_protocol::search::WebSearchAction;

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
            pl_protocol::trace::TracePartState::Tool(pl_protocol::trace::TraceToolPart::streaming(
                invocation,
            )),
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
            pl_protocol::trace::TracePartState::Tool(pl_protocol::trace::TraceToolPart::streaming(
                invocation,
            )),
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
            pl_protocol::trace::TracePartState::Tool(pl_protocol::trace::TraceToolPart::started(
                invocation,
            )),
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
