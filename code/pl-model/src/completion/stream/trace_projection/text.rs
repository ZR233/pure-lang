//! 正文与 reasoning 流的 trace part 投影。

use pl_trace::{AgentEvent, TraceDelta, TracePartCompletion, TracePartKind, TraceTextChannel};

use super::TraceProjection;

/// 每个供应方 reasoning 分块都会投影成独立 TracePart，因此条目内索引从零开始。
const LOCAL_THINKING_CHUNK_INDEX: u32 = 0;

impl TraceProjection {
    pub(crate) fn start_text(
        &mut self,
        item_id: &str,
        channel: TraceTextChannel,
    ) -> Vec<AgentEvent> {
        let item_id = self.active_text_item_id(item_id, channel);
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        self.start_item(
            item_id,
            pl_trace::TracePartState::Text(pl_trace::TraceTextPart::streaming(
                channel,
                String::new(),
            )),
        )
    }

    pub(crate) fn append_text_delta(
        &mut self,
        item_id: &str,
        channel: TraceTextChannel,
        delta: String,
    ) -> Vec<AgentEvent> {
        let mut events = self.start_text(item_id, channel);
        if !delta.is_empty() {
            let item_id = self.active_text_item_id(item_id, channel);
            events.extend(self.apply_item(
                &item_id,
                pl_trace::TracePartAction::Append(TraceDelta::Text { channel, delta }),
            ));
        }
        events
    }

    pub(crate) fn complete_text(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        authoritative_text: Option<String>,
    ) -> Vec<AgentEvent> {
        let key = text_provider_key(item_id, text_channel);
        let Some(item_id) = self.active_text_items.remove(&key) else {
            return Vec::new();
        };
        self.complete_item_by_resolved_id(
            &item_id,
            TracePartKind::Text,
            TracePartCompletion::Text {
                authoritative_content: authoritative_text,
            },
        )
    }

    pub(crate) fn start_thinking(&mut self, item_id: &str, chunk_index: u32) -> Vec<AgentEvent> {
        let item_id = self.active_thinking_item_id(item_id, chunk_index);
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        self.start_item(
            item_id,
            pl_trace::TracePartState::Thinking(pl_trace::TraceThinkingPart::streaming()),
        )
    }

    pub(crate) fn append_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
    ) -> Vec<AgentEvent> {
        let mut events = self.start_thinking(item_id, chunk_index);
        if !delta.is_empty() {
            let item_id = self.active_thinking_item_id(item_id, chunk_index);
            events.extend(self.apply_item(
                &item_id,
                pl_trace::TracePartAction::Append(TraceDelta::Thinking {
                    chunk_index: LOCAL_THINKING_CHUNK_INDEX,
                    delta,
                }),
            ));
        }
        events
    }

    pub(crate) fn append_reasoning_content_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
    ) -> Vec<AgentEvent> {
        let mut events = self.start_thinking(item_id, chunk_index);
        if !delta.is_empty() {
            let item_id = self.active_thinking_item_id(item_id, chunk_index);
            events.extend(self.apply_item(
                &item_id,
                pl_trace::TracePartAction::Append(TraceDelta::ReasoningContent {
                    chunk_index: LOCAL_THINKING_CHUNK_INDEX,
                    delta,
                }),
            ));
        }
        events
    }

    pub(crate) fn complete_thinking(
        &mut self,
        item_id: &str,
        authoritative_summary: Option<Vec<String>>,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if let Some(summary) = authoritative_summary.as_ref() {
            for (index, _text) in summary.iter().enumerate() {
                let chunk_index = index as u32;
                events.extend(self.start_thinking(item_id, chunk_index));
            }
        }
        let prefix = thinking_provider_key_prefix(item_id);
        let item_ids = self
            .active_thinking_items
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        let mut item_ids = item_ids;
        item_ids.sort();
        for key in item_ids {
            if let Some(item_id) = self.active_thinking_items.remove(&key) {
                let authoritative_summary = authoritative_summary.as_ref().and_then(|summary| {
                    key.strip_prefix(&prefix)
                        .and_then(|index| index.parse::<usize>().ok())
                        .and_then(|index| summary.get(index))
                        .map(|text| vec![text.clone()])
                });
                events.extend(self.complete_item_by_resolved_id(
                    &item_id,
                    TracePartKind::Thinking,
                    TracePartCompletion::Thinking {
                        authoritative_summary,
                    },
                ));
            }
        }
        events
    }
}

pub(super) fn text_provider_key(provider_item_id: &str, channel: TraceTextChannel) -> String {
    let channel_str = channel.as_str();
    format!("text:{channel_str}:{provider_item_id}")
}

pub(super) fn thinking_provider_key_prefix(provider_item_id: &str) -> String {
    format!("reasoning:{provider_item_id}:")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pl_trace::{TraceEventKind, TracePart, TracePartKind, TraceTextChannel, TraceTextState};

    use super::super::test_support::{
        TracePartEvent, completed_thinking_item, delta_item_id, test_trace_context, trace,
        trace_part_event, trace_with_sink,
    };
    use super::TraceProjection;

    fn trace_part_text(item: &TracePart) -> String {
        item.thinking()
            .expect("thinking part")
            .summary()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn repeated_provider_thinking_id_after_completion_gets_new_part_id() {
        let mut trace = trace();

        let first = trace.append_thinking_delta("thinking", 0, "first".to_string());
        let first_completed = trace.complete_thinking("thinking", None);
        let second = trace.append_thinking_delta("thinking", 0, "second".to_string());
        let second_completed = trace.complete_thinking("thinking", None);

        let first_delta = first.iter().find_map(delta_item_id).expect("first delta");
        let first_completed = first_completed
            .iter()
            .find_map(|event| match trace_part_event(event)? {
                TracePartEvent::Completed(item) if item.kind() == TracePartKind::Thinking => {
                    Some(item.item_id().to_string())
                }
                _ => None,
            })
            .expect("first complete");
        let second_delta = second.iter().find_map(delta_item_id).expect("second delta");
        let second_completed = second_completed
            .iter()
            .find_map(|event| match trace_part_event(event)? {
                TracePartEvent::Completed(item) if item.kind() == TracePartKind::Thinking => {
                    Some(item.item_id().to_string())
                }
                _ => None,
            })
            .expect("second complete");

        assert_eq!(first_delta, "inference-1-reasoning-1");
        assert_eq!(first_completed, first_delta);
        assert_eq!(second_delta, "inference-1-reasoning-2");
        assert_eq!(second_completed, second_delta);
    }

    #[test]
    fn reasoning_summary_sections_get_distinct_part_ids() {
        let mut trace = trace();

        let first = trace.append_thinking_delta("thinking", 0, "first".to_string());
        let second = trace.append_thinking_delta("thinking", 1, "second".to_string());
        let completed = trace.complete_thinking(
            "thinking",
            Some(vec!["first done".to_string(), "second done".to_string()]),
        );

        let first_delta = first.iter().find_map(delta_item_id).expect("first delta");
        let second_delta = second.iter().find_map(delta_item_id).expect("second delta");
        let completed = completed
            .iter()
            .filter_map(completed_thinking_item)
            .map(|item| (item.item_id().to_string(), trace_part_text(item)))
            .collect::<Vec<_>>();

        assert_eq!(first_delta, "inference-1-reasoning-1");
        assert_eq!(second_delta, "inference-1-reasoning-2");
        assert_eq!(
            completed,
            vec![
                (
                    "inference-1-reasoning-1".to_string(),
                    "first done".to_string(),
                ),
                (
                    "inference-1-reasoning-2".to_string(),
                    "second done".to_string(),
                ),
            ]
        );
    }

    #[test]
    fn raw_reasoning_starts_the_part_and_later_summary_updates_the_same_part() {
        let mut trace = trace();

        let raw = trace.append_reasoning_content_delta("thinking", 0, "raw".to_string());
        let summary = trace.append_thinking_delta("thinking", 0, "summary".to_string());
        let completed_events =
            trace.complete_thinking("thinking", Some(vec!["summary done".to_string()]));
        let completed = completed_events
            .iter()
            .find_map(completed_thinking_item)
            .expect("completed reasoning part");

        assert_eq!(
            raw.iter().find_map(delta_item_id).as_deref(),
            Some("inference-1-reasoning-1")
        );
        assert_eq!(
            summary.iter().find_map(delta_item_id).as_deref(),
            Some("inference-1-reasoning-1")
        );
        assert_eq!(trace_part_text(completed), "summary done");
        assert_eq!(
            completed
                .thinking()
                .expect("thinking part")
                .content()
                .iter()
                .map(|chunk| chunk.content.as_str())
                .collect::<String>(),
            "raw"
        );
    }

    #[test]
    fn empty_reasoning_delta_does_not_create_a_revision_gap() {
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let mut trace = trace_with_sink(sink.clone());

        let started = trace.append_reasoning_content_delta("thinking", 0, String::new());
        let first = trace.append_reasoning_content_delta("thinking", 0, "first".to_string());
        let ignored = trace.append_reasoning_content_delta("thinking", 0, String::new());
        let second = trace.append_reasoning_content_delta("thinking", 0, " second".to_string());

        assert!(matches!(
            started.as_slice(),
            [pl_trace::AgentEvent::TracePartStarted { .. }]
        ));
        assert!(ignored.is_empty());
        assert!(first.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartDelta { event } if event.revision == 1
        )));
        assert!(second.iter().any(|event| matches!(
            event,
            pl_trace::AgentEvent::TracePartDelta { event } if event.revision == 2
        )));
        assert!(trace.take_trace_error().is_none());
        assert_eq!(
            sink.events()
                .into_iter()
                .filter_map(|event| match event.kind {
                    TraceEventKind::TracePartDelta { event } => Some(event.revision),
                    TraceEventKind::TracePartStarted { .. }
                    | TraceEventKind::TracePartCompleted { .. }
                    | TraceEventKind::TracePartFailed { .. }
                    | TraceEventKind::InteractionChanged { .. }
                    | TraceEventKind::SkillActivated { .. }
                    | TraceEventKind::EnabledToolsRecorded { .. } => None,
                })
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn provider_reasoning_chunk_indices_are_local_to_distinct_parts() {
        let mut trace = trace();

        let first = trace.append_reasoning_content_delta("thinking", 0, "first raw".to_string());
        let second = trace.append_reasoning_content_delta("thinking", 1, "second raw".to_string());
        let completed_events = trace.complete_thinking(
            "thinking",
            Some(vec![
                "first summary".to_string(),
                "second summary".to_string(),
            ]),
        );
        let completed = completed_events
            .iter()
            .filter_map(completed_thinking_item)
            .collect::<Vec<_>>();

        assert_eq!(
            first.iter().find_map(delta_item_id).as_deref(),
            Some("inference-1-reasoning-1")
        );
        assert_eq!(
            second.iter().find_map(delta_item_id).as_deref(),
            Some("inference-1-reasoning-2")
        );
        assert_eq!(completed.len(), 2);
        assert_eq!(trace_part_text(completed[0]), "first summary");
        assert_eq!(trace_part_text(completed[1]), "second summary");
        assert_eq!(
            completed[0]
                .thinking()
                .expect("first thinking part")
                .content()
                .iter()
                .map(|chunk| chunk.content.as_str())
                .collect::<String>(),
            "first raw"
        );
        assert_eq!(
            completed[1]
                .thinking()
                .expect("second thinking part")
                .content()
                .iter()
                .map(|chunk| chunk.content.as_str())
                .collect::<String>(),
            "second raw"
        );
    }

    #[test]
    fn raw_only_reasoning_is_preserved_in_the_authoritative_part() {
        let mut trace = trace();

        let _ = trace.append_reasoning_content_delta("thinking", 0, "raw only".to_string());
        let completed_events = trace.complete_thinking("thinking", None);
        let completed = completed_events
            .iter()
            .find_map(completed_thinking_item)
            .expect("completed reasoning part");

        assert!(
            completed
                .thinking()
                .expect("thinking part")
                .summary()
                .is_empty()
        );
        assert_eq!(
            completed
                .thinking()
                .expect("thinking part")
                .content()
                .iter()
                .map(|chunk| chunk.content.as_str())
                .collect::<String>(),
            "raw only"
        );
    }

    #[test]
    fn generated_part_ids_are_scoped_to_inference() {
        let mut first = trace();
        let mut second = TraceProjection::new(test_trace_context("inference-2"));

        let first_delta = first
            .append_thinking_delta("thinking", 0, "one".to_string())
            .iter()
            .find_map(delta_item_id)
            .expect("first delta");
        let second_delta = second
            .append_thinking_delta("thinking", 0, "two".to_string())
            .iter()
            .find_map(delta_item_id)
            .expect("second delta");

        assert_eq!(first_delta, "inference-1-reasoning-1");
        assert_eq!(second_delta, "inference-2-reasoning-1");
    }

    #[test]
    fn completed_text_uses_authoritative_text_and_revision() {
        let mut trace = trace();
        let _ = trace.append_text_delta("msg_1", TraceTextChannel::Final, "par".to_string());
        let completed_events = trace.complete_text(
            "msg_1",
            TraceTextChannel::Final,
            Some("final text".to_string()),
        );
        let completed = completed_events
            .iter()
            .find_map(|event| match trace_part_event(event)? {
                TracePartEvent::Completed(item)
                    if matches!(
                        item.text(),
                        Some(text) if text.channel() == TraceTextChannel::Final
                            && matches!(text.state(), TraceTextState::Completed(_))
                    ) =>
                {
                    Some(item)
                }
                _ => None,
            })
            .expect("completed text item");

        assert_eq!(completed.text().expect("text part").content(), "final text");
        assert_eq!(completed.revision(), 2);
    }
}
