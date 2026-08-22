//! provider 流式增量到 trace part 的投影。
//!
//! 按域拆分:`text` 承载正文与 reasoning 流,`tool` 承载工具与 web search 流,
//! `ids` 承载 item id 解析与别名收敛;本模块保留投影状态、事件记录与
//! 终态收尾。

mod ids;
mod text;
mod tool;

use std::collections::HashMap;

use pl_trace::{AgentEvent, TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartStatus};

use crate::completion::CompletionTraceContext;

pub(crate) struct TraceProjection {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TracePart>,
    active_text_items: HashMap<String, String>,
    active_thinking_items: HashMap<String, String>,
    active_tool_items: HashMap<String, String>,
    segment_occurrences: HashMap<String, u64>,
    events: Vec<TraceEvent>,
}

impl TraceProjection {
    pub(crate) fn new(context: CompletionTraceContext) -> Self {
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            sequence: context.trace_sequence_base,
            started: HashMap::new(),
            active_text_items: HashMap::new(),
            active_thinking_items: HashMap::new(),
            active_tool_items: HashMap::new(),
            segment_occurrences: HashMap::new(),
            events: Vec::new(),
        }
    }

    pub(crate) fn events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    pub(crate) fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
        let item_ids = self
            .started
            .iter()
            .filter(|(_, item)| {
                matches!(
                    item.kind,
                    TracePartKind::Text | TracePartKind::Thinking | TracePartKind::Plan
                )
            })
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.status == TracePartStatus::Completed {
                continue;
            }
            item.status = TracePartStatus::Completed;
            item.updated_at = unix_seconds();
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartCompleted { item: item.clone() },
                item.updated_at,
            );
            events.push(AgentEvent::TracePartCompleted { item });
        }
        events
    }

    pub(crate) fn fail_attempt(&mut self, error: &str) -> Vec<AgentEvent> {
        let mut item_ids = self.started.keys().cloned().collect::<Vec<_>>();
        item_ids.sort_by_key(|item_id| {
            self.started
                .get(item_id)
                .map(|item| item.started_sequence)
                .unwrap_or_default()
        });
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.status == TracePartStatus::Failed {
                continue;
            }
            item.revision += 1;
            item.status = TracePartStatus::Failed;
            item.updated_at = unix_seconds();
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartFailed {
                    item: item.clone(),
                    error: error.to_string(),
                },
                item.updated_at,
            );
            events.push(AgentEvent::TracePartFailed {
                item,
                error: error.to_string(),
            });
        }
        events
    }

    fn record(&mut self, kind: TraceEventKind, timestamp: i64) {
        self.events.push(TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind,
        });
        self.sequence += 1;
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod unit_tests;
