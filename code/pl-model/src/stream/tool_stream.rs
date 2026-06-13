use std::collections::HashMap;

use pl_protocol::{PureError, Result};

use crate::request::ToolCall;

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
        let key = tool_call_accumulator_key(stream_id, call_id, &item_id);
        let initial_id = if item_id.is_empty() {
            key.clone()
        } else {
            item_id.clone()
        };
        let initial_call_id = call_id
            .filter(|call_id| !call_id.is_empty())
            .map(ToOwned::to_owned);
        let acc = self
            .accumulators
            .entry(key.clone())
            .or_insert_with(|| ToolCallAccumulator {
                id: initial_id,
                has_stable_id: !item_id.is_empty(),
                call_id: initial_call_id,
                name: String::new(),
                payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
            });
        acc.merge_metadata(&key, &item_id, call_id, name);
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
        let key = tool_call_accumulator_key(stream_id, call_id, &item_id);
        let initial_id = if item_id.is_empty() {
            key.clone()
        } else {
            item_id.clone()
        };
        let initial_call_id = call_id
            .filter(|call_id| !call_id.is_empty())
            .map(ToOwned::to_owned);
        let acc = self
            .accumulators
            .entry(key.clone())
            .or_insert_with(|| ToolCallAccumulator {
                id: initial_id,
                has_stable_id: !item_id.is_empty(),
                call_id: initial_call_id,
                name: String::new(),
                payload: ToolCallPayloadAccumulator::from_payload(payload),
            });
        acc.merge_metadata(&key, &item_id, call_id, name);
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
        let mut acc = self
            .take_accumulator(stream_id, call_id, item_id)
            .unwrap_or_else(|| {
                let key = tool_call_accumulator_key(stream_id, call_id, item_id);
                ToolCallAccumulator {
                    id: if item_id.is_empty() {
                        key.clone()
                    } else {
                        item_id.to_string()
                    },
                    has_stable_id: !item_id.is_empty(),
                    call_id: call_id
                        .filter(|call_id| !call_id.is_empty())
                        .map(ToOwned::to_owned),
                    name: String::new(),
                    payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
                }
            });
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        acc.merge_metadata(&key, item_id, call_id, name);
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
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        let initial_id = if item_id.is_empty() {
            key.clone()
        } else {
            item_id.to_string()
        };
        let initial_call_id = call_id
            .filter(|call_id| !call_id.is_empty())
            .map(ToOwned::to_owned);
        let acc = self
            .accumulators
            .entry(key.clone())
            .or_insert_with(|| ToolCallAccumulator {
                id: initial_id,
                has_stable_id: !item_id.is_empty(),
                call_id: initial_call_id,
                name: String::new(),
                payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
            });
        acc.merge_metadata(&key, item_id, call_id, name);
        if let Some(payload) = payload {
            acc.replace_payload(payload);
        }
    }

    pub(crate) fn finish_all(&mut self, existing: &[ToolCall]) -> Result<Vec<ToolCall>> {
        let remaining = std::mem::take(&mut self.accumulators);
        let mut calls = Vec::new();
        for (_, acc) in remaining {
            if existing.iter().any(|call| call.id == acc.id) {
                continue;
            }
            calls.push(acc.into_tool_call()?);
        }
        Ok(calls)
    }

    fn take_accumulator(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> Option<ToolCallAccumulator> {
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        if self.accumulators.contains_key(&key) {
            return self.accumulators.remove(&key);
        }

        let fallback_key = self.accumulators.iter().find_map(|(key, accumulator)| {
            let call_id_matches = call_id
                .filter(|call_id| !call_id.is_empty())
                .zip(accumulator.call_id.as_ref())
                .is_some_and(|(left, right)| left == right);
            let item_id_matches = !item_id.is_empty() && accumulator.id == item_id;
            (call_id_matches || item_id_matches).then(|| key.clone())
        });
        fallback_key.and_then(|key| self.accumulators.remove(&key))
    }
}

pub(crate) struct ToolInputSnapshot {
    pub(crate) tool: ToolCallAccumulatorSnapshot,
    pub(crate) delta: String,
}

struct ToolCallAccumulator {
    id: String,
    has_stable_id: bool,
    call_id: Option<String>,
    name: String,
    payload: ToolCallPayloadAccumulator,
}

impl ToolCallAccumulator {
    fn merge_metadata(
        &mut self,
        key: &str,
        item_id: &str,
        call_id: Option<&String>,
        name: Option<String>,
    ) {
        if !item_id.is_empty() && (self.id.is_empty() || self.id == key) {
            self.id = item_id.to_string();
            self.has_stable_id = true;
        }
        if self.call_id.is_none()
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
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Ok(ToolCall::function(
                self.id,
                self.name,
                serde_json::from_str(&arguments).unwrap_or(serde_json::Value::String(arguments)),
                self.call_id,
            )),
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
    pub(crate) call_id: Option<String>,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

pub(crate) fn timeline_tool_item_id(call_id: Option<&String>, id: &str) -> String {
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| id.to_string())
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
