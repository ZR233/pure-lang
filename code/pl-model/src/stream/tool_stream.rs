use std::collections::HashMap;

use pl_protocol::{PureError, Result};

use crate::request::ToolCall;
use crate::tool_arguments::function_tool_call_from_raw;

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
            || call
                .call_id
                .as_ref()
                .filter(|call_id| !call_id.is_empty())
                .zip(self.call_id.as_ref())
                .is_some_and(|(left, right)| left == right)
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
            arguments: self.payload.text().to_string(),
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
        match self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Ok(
                function_tool_call_from_raw(self.id, self.name, arguments, self.call_id),
            ),
            ToolCallPayloadAccumulator::CustomInput(input) => {
                Ok(ToolCall::custom(self.id, self.name, input, self.call_id))
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

    fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(arguments) | Self::CustomInput(arguments) => arguments,
        }
    }
}

pub(crate) struct ToolCallAccumulatorSnapshot {
    pub(crate) id: String,
    pub(crate) trace_id: String,
    pub(crate) call_id: Option<String>,
    pub(crate) name: String,
    pub(crate) arguments: String,
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
        assert_eq!(ready.call_id.as_deref(), Some("call-1"));
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
        assert_eq!(call.call_id.as_deref(), Some("call_1"));
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
        assert_eq!(call.call_id.as_deref(), Some("call_1"));
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
