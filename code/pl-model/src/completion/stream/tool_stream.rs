use std::collections::HashMap;

use pl_protocol::{PureError, Result};

use crate::completion::ToolCall;
use crate::completion::tool_arguments::function_tool_call_from_raw;

use super::event::ToolInputDeltaPayload;

pub(crate) struct ToolStream {
    accumulators: HashMap<String, ToolCallAccumulator>,
}

impl ToolStream {
    pub(crate) fn new() -> Self {
        Self {
            accumulators: HashMap::new(),
        }
    }

    pub(crate) fn append_delta(
        &mut self,
        stream_id: Option<&String>,
        item_id: String,
        call_id: Option<&String>,
        name: Option<String>,
        payload_delta: ToolInputDeltaPayload,
    ) -> ToolInputSnapshot {
        let key = self.resolve_key(stream_id, call_id, &item_id);
        let acc = self.get_or_insert_accumulator(&key, call_id, &item_id, || {
            ToolCallPayloadAccumulator::FunctionArguments(String::new())
        });
        acc.merge_metadata(&item_id, call_id, name);
        let delta_text = payload_delta.text().to_string();
        acc.push_delta(payload_delta);
        ToolInputSnapshot {
            tool: acc.snapshot(),
            delta: delta_text,
        }
    }

    pub(crate) fn start_input(
        &mut self,
        stream_id: Option<&String>,
        item_id: String,
        call_id: Option<&String>,
        name: Option<String>,
        payload: ToolInputDeltaPayload,
    ) -> ToolInputSnapshot {
        let key = self.resolve_key(stream_id, call_id, &item_id);
        let acc = self.get_or_insert_accumulator(&key, call_id, &item_id, || {
            ToolCallPayloadAccumulator::from_payload(payload)
        });
        acc.merge_metadata(&item_id, call_id, name);
        ToolInputSnapshot {
            tool: acc.snapshot(),
            delta: String::new(),
        }
    }

    pub(crate) fn finish_ready(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
        name: Option<String>,
        payload: Option<ToolInputDeltaPayload>,
    ) -> Result<Option<ToolCall>> {
        let key = self.resolve_key(stream_id, call_id, item_id);
        let mut acc = self.remove_accumulator(&key).unwrap_or_else(|| {
            ToolCallAccumulator::new(
                &key,
                call_id,
                item_id,
                ToolCallPayloadAccumulator::FunctionArguments(String::new()),
            )
        });
        acc.merge_metadata(item_id, call_id, name);
        if let Some(payload) = payload {
            acc.replace_payload(payload);
        }
        Ok(Some(acc.into_tool_call()?))
    }

    pub(crate) fn complete_input(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
        name: Option<String>,
        payload: Option<ToolInputDeltaPayload>,
    ) {
        let key = self.resolve_key(stream_id, call_id, item_id);
        let acc = self.get_or_insert_accumulator(&key, call_id, item_id, || {
            ToolCallPayloadAccumulator::FunctionArguments(String::new())
        });
        acc.merge_metadata(item_id, call_id, name);
        if let Some(payload) = payload {
            acc.replace_payload(payload);
        }
    }

    pub(crate) fn finish_all(&mut self, existing: &[ToolCall]) -> Result<Vec<ToolCall>> {
        let remaining = std::mem::take(&mut self.accumulators);
        let mut calls = Vec::new();
        for (_, acc) in remaining {
            if existing
                .iter()
                .any(|call| acc.matches_tool_call_identity(call))
            {
                continue;
            }
            calls.push(acc.into_tool_call()?);
        }
        Ok(calls)
    }

    fn resolve_key(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> String {
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        if self.accumulators.contains_key(&key) {
            return key;
        }

        self.accumulators
            .iter()
            .find_map(|(key, accumulator)| {
                accumulator
                    .matches_identity(call_id, item_id)
                    .then(|| key.clone())
            })
            .or_else(|| self.unique_item_fallback_key(stream_id, call_id, item_id))
            .unwrap_or(key)
    }

    fn unique_item_fallback_key(
        &self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> Option<String> {
        // Some Responses-compatible streams first expose an item id, then only
        // expose a distinct call id. Without another correlation key, upgrading
        // is safe only while exactly one fallback-backed tool remains open.
        if stream_id.is_some_and(|stream_id| !stream_id.is_empty())
            || !call_id.is_some_and(|call_id| !call_id.is_empty() && call_id == item_id)
        {
            return None;
        }

        let mut candidates = self
            .accumulators
            .iter()
            .filter(|(_, accumulator)| accumulator.uses_item_id_as_call_id());
        let (key, _) = candidates.next()?;
        candidates.next().is_none().then(|| key.clone())
    }

    fn remove_accumulator(&mut self, key: &str) -> Option<ToolCallAccumulator> {
        self.accumulators.remove(key)
    }

    fn get_or_insert_accumulator(
        &mut self,
        key: &str,
        call_id: Option<&String>,
        item_id: &str,
        payload: impl FnOnce() -> ToolCallPayloadAccumulator,
    ) -> &mut ToolCallAccumulator {
        self.accumulators
            .entry(key.to_string())
            .or_insert_with(|| ToolCallAccumulator::new(key, call_id, item_id, payload()))
    }
}

pub(crate) struct ToolInputSnapshot {
    pub(crate) tool: ToolCallAccumulatorSnapshot,
    pub(crate) delta: String,
}

struct ToolCallAccumulator {
    id: String,
    trace_id: String,
    has_stable_id: bool,
    call_id: Option<String>,
    name: String,
    payload: ToolCallPayloadAccumulator,
}

impl ToolCallAccumulator {
    fn new(
        key: &str,
        call_id: Option<&String>,
        item_id: &str,
        payload: ToolCallPayloadAccumulator,
    ) -> Self {
        let trace_id = trace_tool_part_id(call_id, item_id, key);
        Self {
            id: if item_id.is_empty() {
                key.to_string()
            } else {
                item_id.to_string()
            },
            trace_id,
            has_stable_id: !item_id.is_empty(),
            call_id: call_id
                .filter(|call_id| !call_id.is_empty())
                .map(ToOwned::to_owned),
            name: String::new(),
            payload,
        }
    }

    fn matches_identity(&self, call_id: Option<&String>, item_id: &str) -> bool {
        let call_id_matches = call_id
            .filter(|call_id| !call_id.is_empty())
            .zip(self.call_id.as_ref())
            .is_some_and(|(left, right)| left == right);
        let item_id_matches = !item_id.is_empty() && self.id == item_id;
        call_id_matches || item_id_matches
    }

    fn matches_tool_call_identity(&self, call: &ToolCall) -> bool {
        call.id == self.id
            || self
                .call_id
                .as_deref()
                .is_some_and(|call_id| call_id == call.call_id)
    }

    fn uses_item_id_as_call_id(&self) -> bool {
        self.has_stable_id && self.call_id.as_deref() == Some(self.id.as_str())
    }

    fn merge_metadata(&mut self, item_id: &str, call_id: Option<&String>, name: Option<String>) {
        let call_id_was_item_fallback = self.call_id.as_deref() == Some(self.id.as_str());
        if !item_id.is_empty() && !self.has_stable_id {
            self.id = item_id.to_string();
            self.has_stable_id = true;
        }
        if (self.call_id.is_none() || call_id_was_item_fallback)
            && let Some(call_id) = call_id.filter(|call_id| !call_id.is_empty())
        {
            self.call_id = Some(call_id.clone());
        }
        if let Some(name) = name
            && !name.is_empty()
        {
            self.name = name;
        }
    }

    fn push_delta(&mut self, payload_delta: ToolInputDeltaPayload) {
        match (&mut self.payload, payload_delta) {
            (
                ToolCallPayloadAccumulator::FunctionArguments(arguments),
                ToolInputDeltaPayload::FunctionArguments(delta),
            ) => arguments.push_str(&delta),
            (
                ToolCallPayloadAccumulator::CustomInput(input),
                ToolInputDeltaPayload::CustomInput(delta),
            ) => input.push_str(&delta),
            (_, ToolInputDeltaPayload::FunctionArguments(delta)) => {
                self.payload = ToolCallPayloadAccumulator::FunctionArguments(delta);
            }
            (_, ToolInputDeltaPayload::CustomInput(delta)) => {
                self.payload = ToolCallPayloadAccumulator::CustomInput(delta);
            }
        }
    }

    fn replace_payload(&mut self, payload: ToolInputDeltaPayload) {
        self.payload = match payload {
            ToolInputDeltaPayload::FunctionArguments(arguments) => {
                ToolCallPayloadAccumulator::FunctionArguments(arguments)
            }
            ToolInputDeltaPayload::CustomInput(input) => {
                ToolCallPayloadAccumulator::CustomInput(input)
            }
        };
    }

    fn snapshot(&self) -> ToolCallAccumulatorSnapshot {
        ToolCallAccumulatorSnapshot {
            id: self.id.clone(),
            trace_id: self.trace_id.clone(),
            call_id: self.call_id.clone(),
            name: self.name.clone(),
        }
    }

    fn into_tool_call(self) -> Result<ToolCall> {
        if self.name.is_empty() {
            return Err(PureError::LlmError(
                "provider emitted tool call without name".to_string(),
            ));
        }
        if !self.has_stable_id && self.call_id.as_ref().is_none_or(String::is_empty) {
            return Err(PureError::LlmError(
                "provider emitted tool call without stable id".to_string(),
            ));
        }
        // call_id 必填：provider 只暴露 item id 时在解码边界确定性赋 call_id = item_id。
        let call_id = self
            .call_id
            .filter(|call_id| !call_id.is_empty())
            .unwrap_or_else(|| self.id.clone());
        match self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Ok(
                function_tool_call_from_raw(self.id, self.name, arguments, call_id),
            ),
            ToolCallPayloadAccumulator::CustomInput(input) => {
                Ok(ToolCall::custom(self.id, self.name, input, call_id))
            }
        }
    }
}

enum ToolCallPayloadAccumulator {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolCallPayloadAccumulator {
    fn from_payload(payload: ToolInputDeltaPayload) -> Self {
        match payload {
            ToolInputDeltaPayload::FunctionArguments(arguments) => {
                Self::FunctionArguments(arguments)
            }
            ToolInputDeltaPayload::CustomInput(input) => Self::CustomInput(input),
        }
    }
}

pub(crate) struct ToolCallAccumulatorSnapshot {
    pub(crate) id: String,
    pub(crate) trace_id: String,
    pub(crate) call_id: Option<String>,
    pub(crate) name: String,
}

fn trace_tool_part_id(call_id: Option<&String>, id: &str, fallback_id: &str) -> String {
    if !id.is_empty() {
        return id.to_string();
    }
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| fallback_id.to_string())
}

fn tool_call_accumulator_key(
    stream_id: Option<&String>,
    call_id: Option<&String>,
    item_id: &str,
) -> String {
    stream_id
        .filter(|stream_id| !stream_id.is_empty())
        .cloned()
        .or_else(|| {
            call_id
                .filter(|call_id| !call_id.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| (!item_id.is_empty()).then(|| item_id.to_string()))
        .unwrap_or_else(|| "tool_call".to_string())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn late_provider_item_id_keeps_original_trace_id() {
        let mut stream = ToolStream::new();
        let call_id = "call-1".to_string();

        let first = stream.append_delta(
            None,
            String::new(),
            Some(&call_id),
            Some("exec".to_string()),
            ToolInputDeltaPayload::FunctionArguments("{\"cmd\":\"ec".to_string()),
        );
        let second = stream.append_delta(
            None,
            "provider-tool-1".to_string(),
            Some(&call_id),
            None,
            ToolInputDeltaPayload::FunctionArguments("ho hi\"}".to_string()),
        );
        let ready = stream
            .finish_ready(
                None,
                Some(&call_id),
                "provider-tool-1",
                Some("exec".to_string()),
                None,
            )
            .unwrap()
            .expect("ready tool call");

        assert_eq!(first.tool.id, "call-1");
        assert_eq!(first.tool.trace_id, "call-1");
        assert_eq!(second.tool.id, "provider-tool-1");
        assert_eq!(second.tool.trace_id, "call-1");
        assert_eq!(ready.id, "provider-tool-1");
        assert_eq!(ready.call_id, "call-1");
    }

    #[test]
    fn stream_id_is_used_as_trace_id_until_provider_identity_arrives() {
        let mut stream = ToolStream::new();
        let first_stream_id = "chat_tool_call:0".to_string();
        let second_stream_id = "chat_tool_call:1".to_string();

        let first = stream.append_delta(
            Some(&first_stream_id),
            String::new(),
            None,
            Some("read_file".to_string()),
            ToolInputDeltaPayload::FunctionArguments("{\"path\":\"a\"}".to_string()),
        );
        let second = stream.append_delta(
            Some(&second_stream_id),
            String::new(),
            None,
            Some("read_file".to_string()),
            ToolInputDeltaPayload::FunctionArguments("{\"path\":\"b\"}".to_string()),
        );

        assert_eq!(first.tool.id, "chat_tool_call:0");
        assert_eq!(first.tool.trace_id, "chat_tool_call:0");
        assert_eq!(second.tool.id, "chat_tool_call:1");
        assert_eq!(second.tool.trace_id, "chat_tool_call:1");
    }

    #[test]
    fn invalid_function_arguments_are_preserved_for_tool_feedback() {
        let mut stream = ToolStream::new();
        let call_id = "call-1".to_string();
        stream.append_delta(
            None,
            "provider-tool-1".to_string(),
            Some(&call_id),
            Some("read_file".to_string()),
            ToolInputDeltaPayload::FunctionArguments("{bad".to_string()),
        );

        let call = stream
            .finish_ready(None, Some(&call_id), "provider-tool-1", None, None)
            .unwrap()
            .expect("tool call");

        assert_eq!(call.payload_text(), "{bad");
        assert_eq!(call.invalid_arguments.as_ref().unwrap().raw, "{bad");
        assert!(
            call.invalid_arguments_message()
                .unwrap()
                .contains("read_file")
        );
    }

    #[test]
    fn explicit_call_id_replaces_item_identity_fallback() {
        let mut stream = ToolStream::new();
        let fallback_call_id = "fc_1".to_string();
        let explicit_call_id = "call_1".to_string();

        stream.start_input(
            None,
            "fc_1".to_string(),
            Some(&fallback_call_id),
            Some("read_file".to_string()),
            ToolInputDeltaPayload::FunctionArguments(String::new()),
        );
        let call = stream
            .finish_ready(
                None,
                Some(&explicit_call_id),
                "fc_1",
                None,
                Some(ToolInputDeltaPayload::FunctionArguments("{}".to_string())),
            )
            .unwrap()
            .expect("ready tool call");

        assert_eq!(call.id, "fc_1");
        assert_eq!(call.call_id, "call_1");
    }

    #[test]
    fn call_id_only_metadata_upgrades_unique_item_fallback() {
        let mut stream = ToolStream::new();
        let fallback_call_id = "fc_1".to_string();
        let explicit_call_id = "call_1".to_string();

        let started = stream.start_input(
            None,
            "fc_1".to_string(),
            Some(&fallback_call_id),
            Some("read_file".to_string()),
            ToolInputDeltaPayload::FunctionArguments(String::new()),
        );
        let delta = stream.append_delta(
            None,
            explicit_call_id.clone(),
            Some(&explicit_call_id),
            None,
            ToolInputDeltaPayload::FunctionArguments("{}".to_string()),
        );
        let call = stream
            .finish_ready(None, Some(&explicit_call_id), "fc_1", None, None)
            .unwrap()
            .expect("ready tool call");

        assert_eq!(started.tool.trace_id, "fc_1");
        assert_eq!(delta.tool.id, "fc_1");
        assert_eq!(delta.tool.trace_id, "fc_1");
        assert_eq!(delta.tool.call_id.as_deref(), Some("call_1"));
        assert_eq!(call.id, "fc_1");
        assert_eq!(call.call_id, "call_1");
    }

    #[test]
    fn call_id_only_metadata_does_not_guess_between_item_fallbacks() {
        let mut stream = ToolStream::new();
        for item_id in ["fc_1", "fc_2"] {
            let fallback_call_id = item_id.to_string();
            stream.start_input(
                None,
                item_id.to_string(),
                Some(&fallback_call_id),
                Some("read_file".to_string()),
                ToolInputDeltaPayload::FunctionArguments(String::new()),
            );
        }
        let explicit_call_id = "call_1".to_string();

        let delta = stream.append_delta(
            None,
            explicit_call_id.clone(),
            Some(&explicit_call_id),
            None,
            ToolInputDeltaPayload::FunctionArguments("{}".to_string()),
        );

        assert_eq!(delta.tool.id, "call_1");
        assert_eq!(delta.tool.trace_id, "call_1");
    }
}
#[cfg(test)]
mod accumulator_tests {
    use pl_protocol::PureError;
    use pl_trace::{AgentEvent, TraceEventKind, TracePartKind};

    use super::super::StreamCompletionAccumulator;
    use super::*;
    use crate::completion::stream::event::ModelStreamEvent;
    use crate::completion::stream::test_support::*;
    use crate::completion::{CompletionTraceContext, ToolCallPayload};

    use pretty_assertions::assert_eq;

    #[test]
    fn stream_accumulator_merges_chat_tool_call_chunks_by_index() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: "call_1".to_string(),
                    call_id: None,
                    name: Some("read_file".to_string()),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(String::new()),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: String::new(),
                    call_id: None,
                    name: None,
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        "{\"path\":\"Cargo.toml\"}".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "call_1");
        assert_eq!(response.tool_calls[0].name, "read_file");
        match &response.tool_calls[0].payload {
            ToolCallPayload::Function { arguments } => {
                assert_eq!(arguments, &serde_json::json!({"path": "Cargo.toml"}));
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn stream_accumulator_splits_reasoning_and_text_across_tool_boundary() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(summary_started("thinking"), &event_tx)
            .unwrap();
        accumulator
            .apply(summary_delta("thinking", 0, "before"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_started("msg_1"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("msg_1", "prelude"), &event_tx)
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputStarted {
                    stream_id: None,
                    item_id: "call_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("exec".to_string()),
                    payload_kind:
                        crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "call_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("exec".to_string()),
                    payload: Some(ToolInputDeltaPayload::FunctionArguments(
                        "{\"command\":\"pwd\"}".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(summary_started("thinking#2"), &event_tx)
            .unwrap();
        accumulator
            .apply(summary_delta("thinking#2", 0, "after"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_started("msg_1#2"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("msg_1#2", "done"), &event_tx)
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();
        let completed = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => {
                    Some((item.item_id(), item.kind(), trace_part_text(item)))
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();
        let tool_seen = response.trace_events.iter().any(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item } => {
                item.item_id() == "turn-1-call_1" && item.kind() == TracePartKind::Tool
            }
            TraceEventKind::TracePartDelta { event } => {
                event.item_id == "turn-1-call_1" && event.kind() == TracePartKind::Tool
            }
            TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => false,
        });

        assert!(completed.contains(&(
            "turn-1-inf-0-reasoning-1",
            TracePartKind::Thinking,
            "before".to_string(),
        )));
        assert!(completed.contains(&(
            "turn-1-inf-0-text-final-1",
            TracePartKind::Text,
            "prelude".to_string(),
        )));
        assert!(tool_seen);
        assert!(completed.contains(&(
            "turn-1-inf-0-reasoning-2",
            TracePartKind::Thinking,
            "after".to_string(),
        )));
        assert!(completed.contains(&(
            "turn-1-inf-0-text-final-2",
            TracePartKind::Text,
            "done".to_string(),
        )));
    }

    #[test]
    fn tagged_stream_flushes_visible_text_before_tool_call() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));
        let mut decoder = tagged_decoder();

        apply_tagged(
            &mut decoder,
            &mut accumulator,
            final_delta("chat-final", "我先检查项目结构。"),
            &event_tx,
        );
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            ModelStreamEvent::ToolInputStarted {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload_kind:
                    crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        );
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            ModelStreamEvent::ToolCallReady {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload: Some(ToolInputDeltaPayload::FunctionArguments(
                    r#"{"path":"Cargo.toml"}"#.to_string(),
                )),
            },
            &event_tx,
        );

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();
        let ordered_trace = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => {
                    Some((item.kind(), item.item_id(), trace_part_text(item)))
                }
                TraceEventKind::TracePartStarted { item } => {
                    Some((item.kind(), item.item_id(), trace_part_text(item)))
                }
                TraceEventKind::TracePartDelta { event } => Some((
                    event.kind(),
                    event.item_id.as_str(),
                    trace_delta_text(&event.delta),
                )),
                TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(response.content.as_deref(), Some("我先检查项目结构。"));
        assert_eq!(response.tool_calls.len(), 1);
        assert!(ordered_trace.iter().any(|(kind, _, text)| {
            *kind == TracePartKind::Text && text == "我先检查项目结构。"
        }));
        let text_index = ordered_trace
            .iter()
            .position(|(kind, _, text)| {
                *kind == TracePartKind::Text && text == "我先检查项目结构。"
            })
            .expect("text part should complete before tool");
        let tool_index = ordered_trace
            .iter()
            .position(|(kind, _, _)| *kind == TracePartKind::Tool)
            .expect("tool part should start");
        assert!(text_index < tool_index);
    }

    #[test]
    fn stream_accumulator_terminal_snapshots_converge_with_live_deltas() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(32);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(summary_started("thinking"), &event_tx)
            .unwrap();
        accumulator
            .apply(summary_delta("thinking", 0, "think"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_started("msg_1"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("msg_1", "hello"), &event_tx)
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("exec".to_string()),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        "{\"command\":\"pwd\"}".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("exec".to_string()),
                    payload: Some(ToolInputDeltaPayload::FunctionArguments(
                        "{\"command\":\"pwd\"}".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();
        let live_events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();

        let started = live_events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::TracePartStarted { item } => {
                    Some((item.item_id(), item.kind(), trace_part_text(item)))
                }
                AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::Done
                | AgentEvent::Error { .. } => None,
            })
            .collect::<Vec<_>>();
        assert!(started.contains(&(
            "turn-1-inf-0-reasoning-1",
            TracePartKind::Thinking,
            String::new(),
        )));
        assert!(started.contains(&(
            "turn-1-inf-0-text-final-1",
            TracePartKind::Text,
            String::new(),
        )));
        assert!(started.contains(&("turn-1-fc_1", TracePartKind::Tool, String::new(),)));

        let mut live = std::collections::HashMap::new();
        for event in &live_events {
            match event {
                AgentEvent::TracePartStarted { item } | AgentEvent::TracePartCompleted { item } => {
                    live.insert(item.item_id().to_string(), trace_part_text(item));
                }
                AgentEvent::TracePartDelta { event } => {
                    live.entry(event.item_id.clone())
                        .or_insert_with(String::new)
                        .push_str(&trace_delta_text(&event.delta));
                }
                AgentEvent::TracePartFailed { item } => {
                    live.insert(item.item_id().to_string(), trace_part_text(item));
                }
                AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::Done
                | AgentEvent::Error { .. } => {}
            }
        }
        let replay = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if matches!(item.kind(), TracePartKind::Text | TracePartKind::Thinking) =>
                {
                    Some((item.item_id().to_string(), trace_part_text(item)))
                }
                TraceEventKind::TracePartStarted { item }
                    if item.kind() == TracePartKind::Tool
                        && item.item_id() == "turn-1-fc_1"
                        && item
                            .tool()
                            .is_some_and(|tool| !tool.invocation().arguments().is_empty()) =>
                {
                    Some((item.item_id().to_string(), trace_part_text(item)))
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<std::collections::HashMap<_, _>>();

        assert_eq!(
            live.get("turn-1-inf-0-reasoning-1"),
            replay.get("turn-1-inf-0-reasoning-1")
        );
        assert_eq!(
            live.get("turn-1-inf-0-text-final-1"),
            replay.get("turn-1-inf-0-text-final-1")
        );
        assert_eq!(live.get("turn-1-fc_1"), replay.get("turn-1-fc_1"));
    }

    #[test]
    fn stream_trace_part_ids_are_scoped_to_turn() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "call_0".to_string(),
                    call_id: Some("call_0".to_string()),
                    name: Some("exec".to_string()),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        r#"{"command":"pwd"}"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "call_0".to_string(),
                    call_id: Some("call_0".to_string()),
                    name: Some("exec".to_string()),
                    payload: Some(ToolInputDeltaPayload::FunctionArguments(
                        "{\"command\":\"pwd\"}".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.tool_calls[0].id, "call_0");
        let item_ids = response
            .trace_events
            .iter()
            .map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item } => item.item_id(),
                TraceEventKind::TracePartDelta { event } => event.item_id.as_str(),
                TraceEventKind::TracePartFailed { item } => item.item_id(),
                TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => "",
            })
            .filter(|item_id| !item_id.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            item_ids,
            vec!["turn-1-call_0", "turn-1-call_0", "turn-1-call_0"]
        );
    }

    #[test]
    fn stream_accumulator_merges_tool_call_with_late_call_id() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ToolInputStarted {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: None,
                    name: Some("read_file".to_string()),
                    payload_kind:
                        crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        r#"{"path":"Cargo.toml"}"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload: None,
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "fc_1");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
        assert_eq!(response.tool_calls[0].name, "read_file");
        let item_ids = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Tool =>
                {
                    Some(item.item_id())
                }
                TraceEventKind::TracePartDelta { event } if event.kind() == TracePartKind::Tool => {
                    Some(event.item_id.as_str())
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            item_ids,
            vec!["turn-1-fc_1", "turn-1-fc_1", "turn-1-fc_1", "turn-1-fc_1"]
        );
    }

    #[test]
    fn stream_accumulator_keeps_tool_trace_id_when_item_id_arrives_late() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: String::new(),
                    call_id: Some("call_1".to_string()),
                    name: Some("read_file".to_string()),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        r#"{"path":"Car"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        r#"go.toml"}"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "fc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload: None,
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "fc_1");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
        assert_eq!(response.tool_calls[0].name, "read_file");
        let item_ids = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Tool =>
                {
                    Some(item.item_id())
                }
                TraceEventKind::TracePartDelta { event } if event.kind() == TracePartKind::Tool => {
                    Some(event.item_id.as_str())
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            item_ids
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["turn-1-call_1"])
        );
        assert_eq!(
            item_ids,
            vec![
                "turn-1-call_1",
                "turn-1-call_1",
                "turn-1-call_1",
                "turn-1-call_1",
                "turn-1-call_1"
            ]
        );
    }

    #[test]
    fn stream_trace_scope_rejects_similar_turn_prefix() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "turn-10-call".to_string(),
                    call_id: None,
                    name: Some("exec".to_string()),
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        r#"{"command":"pwd"}"#.to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "turn-10-call".to_string(),
                    call_id: None,
                    name: None,
                    payload: None,
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartStarted { item }
                if item.kind() == TracePartKind::Tool
                    && item.item_id() == "turn-1-turn-10-call"
        )));
    }

    #[test]
    fn stream_accumulator_uses_responses_added_item_name_when_done_omits_name() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: Some("apply_patch".to_string()),
                    payload_delta: ToolInputDeltaPayload::CustomInput(String::new()),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload_delta: ToolInputDeltaPayload::CustomInput(
                        "*** Begin Patch\n*** End Patch".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::ToolCallReady {
                    stream_id: None,
                    item_id: "ctc_1".to_string(),
                    call_id: Some("call_1".to_string()),
                    name: None,
                    payload: Some(ToolInputDeltaPayload::CustomInput(
                        "*** Begin Patch\n*** End Patch".to_string(),
                    )),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "ctc_1");
        assert_eq!(response.tool_calls[0].name, "apply_patch");
        match &response.tool_calls[0].payload {
            ToolCallPayload::Custom { input } => {
                assert_eq!(input, "*** Begin Patch\n*** End Patch");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn stream_accumulator_requires_completed_event() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(final_started("final"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("final", "partial"), &event_tx)
            .unwrap();

        let error = accumulator.finish(&event_tx).unwrap_err();

        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, pl_protocol::ProviderFailureKind::Transport);
        assert_eq!(failure.message, "provider stream ended before completion");
        assert_eq!(failure.retry.retry_after_ms(), None);
    }

    #[test]
    fn stream_accumulator_rejects_events_after_completed() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        apply_completed(&mut accumulator, &event_tx);
        let error = accumulator
            .apply(
                ModelStreamEvent::ReasoningRawDelta {
                    id: "thinking".to_string(),
                    content_index: 0,
                    delta: "late".to_string(),
                },
                &event_tx,
            )
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert_eq!(message, "provider stream emitted event after completion");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn stream_accumulator_projects_raw_reasoning_into_thinking_trace() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ReasoningRawDelta {
                    id: "thinking".to_string(),
                    content_index: 0,
                    delta: "raw only".to_string(),
                },
                &event_tx,
            )
            .unwrap();
        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.reasoning_content.as_deref(), Some("raw only"));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Thinking && trace_part_text(item) == "raw only"
        )));
    }

    #[test]
    fn stream_accumulator_rejects_tool_delta_without_name() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                ModelStreamEvent::ToolInputDelta {
                    stream_id: Some("chat_tool_call:0".to_string()),
                    item_id: "call_1".to_string(),
                    call_id: None,
                    name: None,
                    payload_delta: ToolInputDeltaPayload::FunctionArguments(
                        "{\"path\":\"Cargo.toml\"}".to_string(),
                    ),
                },
                &event_tx,
            )
            .unwrap();
        let error = accumulator
            .apply(ModelStreamEvent::Completed { response_id: None }, &event_tx)
            .unwrap_err();

        match error {
            PureError::LlmError(message) => {
                assert_eq!(message, "provider emitted tool call without name");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
