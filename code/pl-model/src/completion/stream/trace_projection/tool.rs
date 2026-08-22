//! 工具调用与 web search 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TraceEventKind, TracePart, TracePartDeltaEvent, TracePartKind,
    TracePartSource, TracePartStatus, TraceToolPart,
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
        let item = TracePart {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            started_sequence: self.sequence,
            revision: 0,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            source: TracePartSource::Model,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            reasoning_content_chunks: Vec::new(),
            tool: Some(TraceToolPart {
                tool_call_id: item_id.clone(),
                call_id: snapshot.call_id.clone(),
                provider_item_id: (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                name: snapshot.name.clone(),
                arguments: snapshot.arguments.clone(),
                result: None,
                exit_code: None,
                timed_out: false,
                output_artifacts: Vec::new(),
                audit_metadata: Vec::new(),
                output_metrics: None,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        };
        self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
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
            if item.status != TracePartStatus::Completed
                && let Some(tool) = &mut item.tool
            {
                tool.arguments = web_search_arguments(&action);
                item.updated_at = now;
            }
            return Vec::new();
        }
        let item = TracePart {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            started_sequence: self.sequence,
            revision: 0,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            source: TracePartSource::Model,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            reasoning_content_chunks: Vec::new(),
            tool: Some(TraceToolPart {
                tool_call_id: item_id.clone(),
                call_id: None,
                provider_item_id: (!provider_item_id.is_empty())
                    .then(|| provider_item_id.to_string()),
                name: "web_search".to_string(),
                arguments: web_search_arguments(&action),
                result: None,
                exit_code: None,
                timed_out: false,
                output_metrics: None,
                output_artifacts: Vec::new(),
                audit_metadata: Vec::new(),
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        };
        self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
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
        let changed = item
            .tool
            .as_ref()
            .is_none_or(|tool| tool.arguments != arguments || tool.output_artifacts != artifacts);
        if item.status == TracePartStatus::Completed && !changed {
            return events;
        }
        item.revision += 1;
        item.status = TracePartStatus::Completed;
        item.updated_at = unix_seconds();
        if let Some(tool) = &mut item.tool {
            tool.name = "web_search".to_string();
            tool.arguments = arguments;
            tool.output_artifacts = artifacts;
        }
        let item = item.clone();
        self.record(
            TraceEventKind::TracePartCompleted { item: item.clone() },
            item.updated_at,
        );
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
        if let Some(item) = self.started.get_mut(&item_id) {
            item.revision += 1;
            item.status = TracePartStatus::Streaming;
            item.updated_at = now;
            if let Some(tool) = &mut item.tool {
                tool.name = snapshot.name.clone();
                tool.arguments = snapshot.arguments.clone();
                tool.call_id = snapshot.call_id.clone();
                tool.provider_item_id = (!snapshot.id.is_empty()).then(|| snapshot.id.clone());
            }
        }
        let revision = self
            .started
            .get(&item_id)
            .map(|item| item.revision)
            .unwrap_or_default();
        let event = TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            started_sequence: self.sequence,
            revision,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TraceDelta::ToolArguments { delta },
        };
        self.record(
            TraceEventKind::TracePartDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TracePartDelta { event });
        events
    }

    pub(crate) fn update_tool_trace(&mut self, call: &ToolCall) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_call_item_id(call);
        let turn_id = self.turn_id.clone();
        let sequence = self.sequence;
        let mut inserted = false;
        let item = self.started.entry(item_id.clone()).or_insert_with(|| {
            inserted = true;
            TracePart {
                turn_id,
                item_id: item_id.clone(),
                started_sequence: sequence,
                revision: 0,
                kind: TracePartKind::Tool,
                status: TracePartStatus::Started,
                created_at: now,
                updated_at: now,
                source: TracePartSource::Model,
                text_channel: None,
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                reasoning_content_chunks: Vec::new(),
                tool: Some(TraceToolPart {
                    tool_call_id: item_id.clone(),
                    call_id: Some(call.call_id.clone()),
                    provider_item_id: (!call.id.is_empty()).then(|| call.id.clone()),
                    name: call.name.clone(),
                    arguments: call.payload_text(),
                    result: None,
                    exit_code: None,
                    timed_out: false,
                    output_artifacts: Vec::new(),
                    audit_metadata: Vec::new(),
                    output_metrics: None,
                    working_directory: None,
                    denial_reason: None,
                }),
                agent: None,
                inference: None,
                usage: None,
            }
        });
        if inserted {
            item.status = TracePartStatus::Started;
        }
        item.updated_at = now;
        if let Some(tool) = &mut item.tool {
            tool.tool_call_id = item_id.clone();
            tool.call_id = Some(call.call_id.clone());
            tool.provider_item_id = Some(call.id.clone());
            tool.name = call.name.clone();
            tool.arguments = call.payload_text();
        }
        let item = item.clone();
        self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
        vec![AgentEvent::TracePartStarted { item }]
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
