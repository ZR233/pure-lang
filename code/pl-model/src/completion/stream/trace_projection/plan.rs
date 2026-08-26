//! Strict incremental JSON-string projection for plan-bearing tools.

use pl_trace::{
    AgentEvent, ToolInputTraceDiscriminator, ToolInputTraceProjection, TraceDelta, TraceEventKind,
    TracePart, TracePartAction, TraceToolFailureKind,
};

use super::super::tool_stream::ToolCallAccumulatorSnapshot;
use super::{TraceProjection, unix_seconds};

#[derive(Debug, Default)]
pub(super) struct PlanProjectionState {
    emitted: String,
    item_id: String,
    created: bool,
    failed: bool,
}

impl TraceProjection {
    pub(super) fn project_plan_arguments(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
    ) -> Vec<AgentEvent> {
        if !snapshot.function_arguments {
            return Vec::new();
        }
        let Some(projection) = self.input_trace_projections.get(&snapshot.name).cloned() else {
            return Vec::new();
        };
        let ToolInputTraceProjection::PlanMarkdown {
            content_field,
            discriminator,
        } = projection;
        let tool_item_id = self.active_tool_item_id(snapshot);
        let plan_item_id = format!("{tool_item_id}:plan");
        let extraction = match extract_plan_string(
            &snapshot.arguments,
            &content_field,
            discriminator.as_ref(),
        ) {
            Ok(extraction) => extraction,
            Err(error) => return self.fail_projected_plan(&tool_item_id, &error),
        };
        if !discriminator_matches(discriminator.as_ref(), extraction.discriminator.as_deref()) {
            return Vec::new();
        }
        let Some(content) = extraction.content else {
            return Vec::new();
        };

        let state = self
            .plan_projections
            .entry(tool_item_id.clone())
            .or_insert_with(|| PlanProjectionState {
                item_id: plan_item_id.clone(),
                ..PlanProjectionState::default()
            });
        if state.failed {
            return Vec::new();
        }
        if !content.starts_with(&state.emitted) {
            return self.fail_projected_plan(
                &tool_item_id,
                "streamed plan content changed after publication",
            );
        }
        let delta = content[state.emitted.len()..].to_string();
        if delta.is_empty() {
            return Vec::new();
        }

        let mut events = Vec::new();
        let created = state.created;
        state.created = true;
        state.emitted.push_str(&delta);
        if !created {
            let now = unix_seconds();
            let item = TracePart::started_plan(
                self.turn_id.clone(),
                plan_item_id.clone(),
                self.sequence,
                now,
            );
            self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
            self.started.insert(plan_item_id.clone(), item.clone());
            events.push(AgentEvent::TracePartStarted { item });
        }
        let Some(item) = self.started.get_mut(&plan_item_id) else {
            return events;
        };
        if item
            .apply(item.command(
                unix_seconds(),
                TracePartAction::Append(TraceDelta::Plan {
                    delta: delta.clone(),
                }),
            ))
            .is_err()
        {
            return events;
        }
        let Ok(delta_event) = item.delta_event(TraceDelta::Plan { delta }) else {
            return events;
        };
        self.record(
            TraceEventKind::TracePartDelta {
                event: delta_event.clone(),
            },
            delta_event.updated_at,
        );
        events.push(AgentEvent::TracePartDelta { event: delta_event });
        events
    }

    pub(super) fn fail_projected_plan(
        &mut self,
        tool_item_id: &str,
        error: &str,
    ) -> Vec<AgentEvent> {
        let Some(state) = self.plan_projections.get_mut(tool_item_id) else {
            return Vec::new();
        };
        if !state.created || state.failed {
            return Vec::new();
        }
        state.failed = true;
        let item_id = state.item_id.clone();
        let Some(item) = self.started.get_mut(&item_id) else {
            return Vec::new();
        };
        if item.is_terminal() {
            return Vec::new();
        }
        let now = unix_seconds();
        if item
            .apply(item.command(
                now,
                TracePartAction::Fail {
                    error: error.to_string(),
                    tool_kind: TraceToolFailureKind::Execution,
                },
            ))
            .is_err()
        {
            return Vec::new();
        }
        let item = item.clone();
        self.record(
            TraceEventKind::TracePartFailed { item: item.clone() },
            item.updated_at(),
        );
        vec![AgentEvent::TracePartFailed { item }]
    }
}

fn discriminator_matches(
    expected: Option<&ToolInputTraceDiscriminator>,
    actual: Option<&str>,
) -> bool {
    expected.is_none_or(|expected| actual == Some(expected.value.as_str()))
}

#[derive(Debug, Default)]
struct PlanExtraction {
    content: Option<String>,
    discriminator: Option<String>,
}

fn extract_plan_string(
    input: &str,
    content_field: &str,
    discriminator: Option<&ToolInputTraceDiscriminator>,
) -> Result<PlanExtraction, String> {
    let bytes = input.as_bytes();
    let mut index = skip_whitespace(bytes, 0);
    if index == bytes.len() {
        return Ok(PlanExtraction::default());
    }
    if bytes[index] != b'{' {
        return Err("tool arguments must be one JSON object".to_string());
    }
    index += 1;
    let mut extraction = PlanExtraction::default();
    loop {
        index = skip_whitespace(bytes, index);
        if index == bytes.len() {
            return Ok(extraction);
        }
        if bytes[index] == b'}' {
            return Ok(extraction);
        }
        let (key, next) = match decode_json_string(input, index)? {
            DecodedString::Complete { value, next } => (value, next),
            DecodedString::Partial { .. } => return Ok(extraction),
        };
        index = skip_whitespace(bytes, next);
        if index == bytes.len() {
            return Ok(extraction);
        }
        if bytes[index] != b':' {
            return Err("JSON object key must be followed by `:`".to_string());
        }
        index = skip_whitespace(bytes, index + 1);
        if index == bytes.len() {
            return Ok(extraction);
        }
        let is_content = key == content_field;
        let is_discriminator = discriminator.is_some_and(|value| key == value.field);
        if is_content || is_discriminator {
            if bytes[index] != b'"' {
                return Err(format!("field `{key}` must be a JSON string"));
            }
            match decode_json_string(input, index)? {
                DecodedString::Complete { value, next } => {
                    if is_content {
                        if extraction.content.replace(value).is_some() {
                            return Err(format!("field `{content_field}` must not be repeated"));
                        }
                    } else if extraction.discriminator.replace(value).is_some() {
                        return Err(format!("field `{key}` must not be repeated"));
                    }
                    index = next;
                }
                DecodedString::Partial { value } => {
                    if is_content {
                        extraction.content = Some(value);
                    }
                    return Ok(extraction);
                }
            }
        } else {
            let Some(next) = skip_json_value(input, index)? else {
                return Ok(extraction);
            };
            index = next;
        }
        index = skip_whitespace(bytes, index);
        if index == bytes.len() {
            return Ok(extraction);
        }
        match bytes[index] {
            b',' => index += 1,
            b'}' => return Ok(extraction),
            _ => return Err("JSON object fields must be separated by `,`".to_string()),
        }
    }
}

enum DecodedString {
    Complete { value: String, next: usize },
    Partial { value: String },
}

fn decode_json_string(input: &str, start: usize) -> Result<DecodedString, String> {
    let bytes = input.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return Err("expected JSON string".to_string());
    }
    let mut output = String::new();
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                return Ok(DecodedString::Complete {
                    value: output,
                    next: index + 1,
                });
            }
            b'\\' => {
                let Some(escape) = bytes.get(index + 1).copied() else {
                    return Ok(DecodedString::Partial { value: output });
                };
                match escape {
                    b'"' => output.push('"'),
                    b'\\' => output.push('\\'),
                    b'/' => output.push('/'),
                    b'b' => output.push('\u{0008}'),
                    b'f' => output.push('\u{000c}'),
                    b'n' => output.push('\n'),
                    b'r' => output.push('\r'),
                    b't' => output.push('\t'),
                    b'u' => {
                        let Some((unit, next)) = decode_hex_unit(bytes, index + 2)? else {
                            return Ok(DecodedString::Partial { value: output });
                        };
                        index = next;
                        let scalar = if (0xD800..=0xDBFF).contains(&unit) {
                            if bytes.get(index..index + 2) != Some(b"\\u") {
                                if index == bytes.len() || bytes.get(index) == Some(&b'\\') {
                                    return Ok(DecodedString::Partial { value: output });
                                }
                                return Err("high surrogate must be followed by a low surrogate"
                                    .to_string());
                            }
                            let Some((low, next)) = decode_hex_unit(bytes, index + 2)? else {
                                return Ok(DecodedString::Partial { value: output });
                            };
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err("invalid low surrogate in JSON string".to_string());
                            }
                            index = next;
                            0x1_0000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32
                        } else if (0xDC00..=0xDFFF).contains(&unit) {
                            return Err("unexpected low surrogate in JSON string".to_string());
                        } else {
                            unit as u32
                        };
                        output.push(
                            char::from_u32(scalar).ok_or_else(|| {
                                "invalid Unicode scalar in JSON string".to_string()
                            })?,
                        );
                        continue;
                    }
                    _ => return Err("invalid JSON string escape".to_string()),
                }
                index += 2;
            }
            byte if byte < 0x20 => {
                return Err("unescaped control character in JSON string".to_string());
            }
            _ => {
                let character = input[index..]
                    .chars()
                    .next()
                    .ok_or_else(|| "invalid UTF-8 in JSON string".to_string())?;
                output.push(character);
                index += character.len_utf8();
            }
        }
    }
    Ok(DecodedString::Partial { value: output })
}

fn decode_hex_unit(bytes: &[u8], start: usize) -> Result<Option<(u16, usize)>, String> {
    if bytes.len().saturating_sub(start) < 4 {
        return Ok(None);
    }
    let mut value = 0_u16;
    for byte in &bytes[start..start + 4] {
        value = value
            .checked_mul(16)
            .and_then(|value| hex_value(*byte).map(|digit| value + digit as u16))
            .ok_or_else(|| "invalid Unicode escape in JSON string".to_string())?;
    }
    Ok(Some((value, start + 4)))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn skip_json_value(input: &str, start: usize) -> Result<Option<usize>, String> {
    let bytes = input.as_bytes();
    if bytes.get(start) == Some(&b'"') {
        return match decode_json_string(input, start)? {
            DecodedString::Complete { next, .. } => Ok(Some(next)),
            DecodedString::Partial { .. } => Ok(None),
        };
    }
    let mut index = start;
    let mut depth = 0_i32;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => match decode_json_string(input, index)? {
                DecodedString::Complete { next, .. } => index = next,
                DecodedString::Partial { .. } => return Ok(None),
            },
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                index += 1;
            }
            b',' | b'}' if depth == 0 => return Ok(Some(index)),
            _ => index += 1,
        }
    }
    Ok(None)
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_partial_unicode_and_surrogate_pairs() {
        let partial = extract_plan_string(r##"{"plan":"# 计划\n中\uD83D"##, "plan", None).unwrap();
        assert_eq!(partial.content.as_deref(), Some("# 计划\n中"));

        let complete =
            extract_plan_string(r##"{"plan":"# 计划\n中\uD83D\uDE80"}"##, "plan", None).unwrap();
        assert_eq!(complete.content.as_deref(), Some("# 计划\n中🚀"));
    }

    #[test]
    fn buffers_content_until_late_discriminator_matches() {
        let discriminator = ToolInputTraceDiscriminator {
            field: "action".to_string(),
            value: "submitPlan".to_string(),
        };
        let extraction = extract_plan_string(
            r##"{"summary":"# Plan","action":"submitPlan"}"##,
            "summary",
            Some(&discriminator),
        )
        .unwrap();
        assert!(discriminator_matches(
            Some(&discriminator),
            extraction.discriminator.as_deref()
        ));
        assert_eq!(extraction.content.as_deref(), Some("# Plan"));
    }
}
