//! Strict samples for in-memory replay and three native GUI lifecycles.
use super::*;

#[derive(Debug, Clone)]
pub struct ReplayExpectation {
    pub history: Vec<Value>,
    pub previous_response_id: Option<String>,
}

impl ReplayExpectation {
    fn matches(&self, body: &Value) -> bool {
        if body.get("previous_response_id").and_then(Value::as_str)
            != self.previous_response_id.as_deref()
        {
            return false;
        }
        if body.to_string().contains("unfinished replay note") {
            return false;
        }
        let Some(items) = body
            .get("input")
            .or_else(|| body.get("messages"))
            .and_then(Value::as_array)
        else {
            return false;
        };
        if self.previous_response_id.is_some() {
            // The server retains the verified preceding request; only newly admitted items travel.
            return !items.is_empty()
                && !items
                    .iter()
                    .any(|item| item.get("role").and_then(Value::as_str) == Some("assistant"));
        }
        let mut previous_position = None;
        for expected in &self.history {
            let positions = items
                .iter()
                .enumerate()
                .filter(|(_, item)| *item == expected)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if positions.len() != 1
                || previous_position.is_some_and(|position| position >= positions[0])
            {
                return false;
            }
            previous_position = Some(positions[0]);
        }
        true
    }
}

impl RequestMatch {
    pub(super) fn base(&self) -> &Self {
        match self {
            Self::Replay { request, .. } => request.base(),
            _ => self,
        }
    }

    pub(super) fn replay_matches(&self, body: &Value) -> bool {
        match self {
            Self::Replay { expected, .. } => expected.matches(body),
            _ => true,
        }
    }
}

impl Step {
    pub fn with_replay(mut self, expected: ReplayExpectation) -> Self {
        self.request = RequestMatch::Replay {
            request: Box::new(self.request),
            expected,
        };
        self
    }
}

/// Deliberate JSON whitespace must survive declaration, execution and history replay.
pub fn replay_tool_arguments() -> String {
    let command = if cfg!(windows) {
        "Write-Output 'replay-tool-marker'"
    } else {
        "printf 'replay-tool-marker\\n'"
    };
    format!("{{ \"command\" : {} }}", json!(command))
}

/// Provider output facts, independent of the production request encoder.
pub fn replay_output(id: &str, text: &str, call: Option<&RealtimeToolCall<'_>>) -> Vec<Value> {
    let mut items = vec![
        json!({"id":format!("{id}-reason"),"type":"reasoning","summary":[],"encrypted_content":format!("frozen-{id}")}),
        json!({"id":format!("{id}-message"),"type":"message","role":"assistant","phase":if call.is_some() {"commentary"} else {"final_answer"},"status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}),
    ];
    if let Some(call) = call {
        items.push(json!({"id":call.item_id,"type":"function_call","name":call.name,"call_id":call.call_id,"arguments":call.arguments,"status":"completed"}));
    }
    items
}

fn chat_history(output: &[Value]) -> Value {
    let message = output
        .iter()
        .find(|item| item["type"] == "message")
        .expect("fixture message");
    let tag = if message["phase"] == "commentary" {
        "commentary"
    } else {
        "final"
    };
    let text = message["content"][0]["text"]
        .as_str()
        .expect("fixture text");
    let calls = output.iter().filter(|item| item["type"] == "function_call").map(|item| json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}})).collect::<Vec<_>>();
    let mut message = json!({"role":"assistant","content":format!("<{tag}>{text}</{tag}>"),"reasoning_content":"recorded replay thought"});
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    message
}

fn events(protocol: Protocol, id: &str, output: &[Value]) -> Vec<Value> {
    if protocol == Protocol::Chat {
        let message = chat_history(output);
        let mut delta =
            json!({"content":message["content"],"reasoning_content":message["reasoning_content"]});
        if let Some(calls) = message.get("tool_calls") {
            let calls = calls
                .as_array()
                .expect("fixture calls")
                .iter()
                .enumerate()
                .map(|(index, call)| {
                    let mut call = call.clone();
                    call["index"] = json!(index);
                    call
                })
                .collect::<Vec<_>>();
            delta["tool_calls"] = json!(calls);
        }
        return vec![
            json!({"id":id,"model":"fixture-model","choices":[{"delta":delta,"finish_reason":null}]}),
            json!({"id":id,"model":"fixture-model","choices":[{"delta":{},"finish_reason":if message.get("tool_calls").is_some() {"tool_calls"} else {"stop"}}]}),
            json!({"id":id,"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":5,"total_tokens":25}}),
        ];
    }
    let mut events =
        vec![json!({"type":"response.created","response":{"id":id,"model":"fixture-model"}})];
    for (index, item) in output.iter().enumerate() {
        events.push(json!({"type":"response.output_item.added","output_index":index,"item":item}));
        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    events.push(json!({"type":"response.completed","response":{"id":id,"model":"fixture-model","output":output,"usage":{"input_tokens":20,"output_tokens":5}}}));
    events
}

fn reply(protocol: Protocol, events: Vec<Value>) -> Reply {
    if protocol == Protocol::ResponsesWebSocket {
        Reply::WebSocket(events)
    } else {
        Reply::Sse(events)
    }
}

/// All three protocol groups use their own isolated home, retained across three GUI processes.
pub fn gui_context_replay_recovery_script() -> Vec<Step> {
    let mut script = RealtimeScript::new();
    for (label, protocol) in [
        ("http", Protocol::ResponsesHttp),
        ("ws", Protocol::ResponsesWebSocket),
        ("chat", Protocol::Chat),
    ] {
        let inspect = format!("Replay inspect {label}");
        let title = session_title_prompt(&inspect);
        script.add(|step| {
            Step::prompt(
                protocol,
                title.clone(),
                step,
                reply(
                    protocol,
                    events(
                        protocol,
                        &format!("{label}-title"),
                        &replay_output(&format!("{label}-title"), "Replay Session", None),
                    ),
                ),
            )
            .optional()
        });
        let first_id = format!("{label}-first");
        let first_call_id = format!("{label}-tool-first");
        let call = RealtimeToolCall {
            item_id: &first_call_id,
            call_id: &first_call_id,
            name: "exec",
            arguments: replay_tool_arguments(),
        };
        let first = replay_output(
            &first_id,
            &format!("replay progress {label} first"),
            Some(&call),
        );
        script.add(|step| {
            Step::prompt(
                protocol,
                inspect,
                step,
                reply(protocol, events(protocol, &first_id, &first)),
            )
        });
        script.add(|step| {
            Step::prompt(
                protocol,
                title.clone(),
                step,
                reply(
                    protocol,
                    events(
                        protocol,
                        &format!("{label}-title"),
                        &replay_output(&format!("{label}-title"), "Replay Session", None),
                    ),
                ),
            )
            .optional()
        });
        let mut history = if protocol == Protocol::Chat {
            vec![chat_history(&first)]
        } else {
            first
        };
        let expected = |history: &Vec<Value>, previous: &str| ReplayExpectation {
            history: history.clone(),
            previous_response_id: (protocol == Protocol::ResponsesWebSocket)
                .then(|| previous.to_owned()),
        };
        let final_id = format!("{label}-final");
        let final_output = replay_output(&final_id, &format!("replay final {label} first"), None);
        script.add(|step| {
            Step::tool_output(
                protocol,
                &first_call_id,
                "replay-tool-marker",
                step,
                reply(protocol, events(protocol, &final_id, &final_output)),
            )
            .with_replay(expected(&history, &first_id))
        });
        script.add(|step| {
            Step::prompt(
                protocol,
                title.clone(),
                step,
                reply(
                    protocol,
                    events(
                        protocol,
                        &format!("{label}-title"),
                        &replay_output(&format!("{label}-title"), "Replay Session", None),
                    ),
                ),
            )
            .optional()
        });
        history.extend(if protocol == Protocol::Chat {
            vec![chat_history(&final_output)]
        } else {
            final_output
        });
        let second_id = format!("{label}-second");
        let second_output =
            replay_output(&second_id, &format!("replay final {label} second"), None);
        script.add(|step| {
            Step::prompt(
                protocol,
                format!("Replay continue {label}"),
                step,
                reply(protocol, events(protocol, &second_id, &second_output)),
            )
            .with_replay(expected(&history, &final_id))
        });
        history.extend(if protocol == Protocol::Chat {
            vec![chat_history(&second_output)]
        } else {
            second_output
        });
        let failure = if protocol == Protocol::Chat {
            Reply::HangingSse(vec![
                json!({"id":"unfinished","choices":[{"delta":{"content":"<commentary>unfinished replay note</commentary>"},"finish_reason":null}]}),
            ])
        } else {
            let mut failure = events(
                protocol,
                &format!("{label}-failed"),
                &replay_output(&format!("{label}-failed"), "unfinished replay note", None),
            );
            failure.pop();
            failure.push(json!({"type":"response.failed","response":{"error":{"code":"invalid_request_error","message":"scripted permanent failure"}}}));
            reply(protocol, failure)
        };
        script.add(|step| {
            Step::prompt(protocol, format!("Replay failed {label}"), step, failure)
                .with_replay(expected(&history, &second_id))
        });
        let resume_id = format!("{label}-resume");
        let resume_call_id = format!("{label}-tool-resume");
        let call = RealtimeToolCall {
            item_id: &resume_call_id,
            call_id: &resume_call_id,
            name: "exec",
            arguments: replay_tool_arguments(),
        };
        let resumed = replay_output(
            &resume_id,
            &format!("replay progress {label} resumed"),
            Some(&call),
        );
        script.add(|step| {
            Step::prompt(
                protocol,
                format!("Replay resume {label}"),
                step,
                reply(protocol, events(protocol, &resume_id, &resumed)),
            )
            .with_replay(ReplayExpectation {
                history: history.clone(),
                previous_response_id: None,
            })
        });
        history.extend(if protocol == Protocol::Chat {
            vec![chat_history(&resumed)]
        } else {
            resumed
        });
        let done_id = format!("{label}-resumed-final");
        script.add(|step| {
            Step::tool_output(
                protocol,
                &resume_call_id,
                "replay-tool-marker",
                step,
                reply(
                    protocol,
                    events(
                        protocol,
                        &done_id,
                        &replay_output(&done_id, &format!("replay final {label} resumed"), None),
                    ),
                ),
            )
            .with_replay(expected(&history, &resume_id))
        });
    }
    script.finish()
}
