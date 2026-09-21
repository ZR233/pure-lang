//! Saved model output projection, without rerunning an adapter or advancing a session.
use super::{ProjectionError, content::text_content};
use pl_core::{
    context::ContextContent,
    model::ModelStepOutput,
    thread::{AttemptOutcome, ThreadEffectBatch, ThreadSnapshot},
};
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
    ThreadThinkingItem,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub(in crate::studio) fn project_responses(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut stamps = BTreeMap::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        if let Some(attempt) = &commit.attempt {
            let stamp = stamps.entry(attempt.attempt_id.as_str()).or_insert((
                commit.sequence,
                commit.committed_at,
                commit.sequence,
                commit.committed_at,
            ));
            stamp.2 = commit.sequence;
            stamp.3 = commit.committed_at;
        }
    }
    let mut items = Vec::new();
    for attempt in snapshot.attempts.iter() {
        let &(ordinal, created_at, revision, updated_at) = stamps
            .get(attempt.attempt_id.as_str())
            .ok_or_else(|| ProjectionError::MissingAttempt(attempt.attempt_id.clone()))?;
        items.extend(project_attempt(
            thread_id,
            snapshot,
            attempt,
            ordinal,
            created_at,
            revision,
            updated_at,
            &BTreeSet::new(),
        )?);
    }
    Ok(items)
}

/// Terminal streaming channels an attempt actually started.
///
/// The channel identity is fixed by the attempt id, so the caller supplies the same set to the
/// durable writer and to the live projection from one durable fact (an existing history item or a
/// reserved ordinal). A channel that never streamed is absent, so no empty placeholder is created.
pub(in crate::studio) fn started_channels(
    attempt_id: &str,
    started: impl Fn(&str) -> bool,
) -> BTreeSet<String> {
    let mut channels = BTreeSet::new();
    for kind in ["reasoning", "text"] {
        let id = super::order::response_id(attempt_id, kind);
        if started(&id) {
            channels.insert(id);
        }
    }
    channels
}

#[allow(clippy::too_many_arguments)]
pub(super) fn project_attempt(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    attempt: &pl_core::thread::RequestAttempt,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
    finalize: &BTreeSet<String>,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut items = vec![inference_item(
        thread_id, attempt, created_at, updated_at, revision,
    )?];
    let output = match &attempt.outcome {
        AttemptOutcome::Committed(output)
        | AttemptOutcome::Rejected { output, .. }
        | AttemptOutcome::Cancelled { result: Ok(output) } => Some(output),
        AttemptOutcome::Running
        | AttemptOutcome::Interrupted
        | AttemptOutcome::Failed(_)
        | AttemptOutcome::Cancelled { result: Err(_) } => None,
    };
    let decoding_error = attempt
        .request_metadata
        .as_ref()
        .and_then(|payload| pl_model::runtime::model_request_receipt(payload).err())
        .map(|error| error.to_string())
        .or_else(|| {
            output
                .and_then(|output| response_content(output).err())
                .map(|error| error.to_string())
        });
    if let Some(notice) = decoding_error {
        let mut payloads = attempt
            .request_metadata
            .iter()
            .map(super::raw_payload)
            .collect::<Vec<_>>();
        if let Some(output) = output {
            for content in &output.content {
                payloads.push(match content {
                    ContextContent::Text { text } => pl_protocol::ThreadRawPayload {
                        format: "text/plain".into(),
                        version: 1,
                        content: text.to_string(),
                    },
                    ContextContent::Opaque { payload } => super::raw_payload(payload),
                    ContextContent::Resource { reference } => pl_protocol::ThreadRawPayload {
                        format: "pl.resource-reference".into(),
                        version: 1,
                        content: serde_json::to_string(reference)?,
                    },
                });
            }
        }
        items.push(ThreadItem::new(
            super::order::response_id(&attempt.attempt_id, "text"),
            thread_id.into(),
            attempt.turn_id.clone(),
            ordinal,
            revision,
            created_at,
            updated_at,
            ThreadItemState::Raw(pl_protocol::ThreadRawItem {
                payloads,
                notice,
                recorded_at: updated_at,
            }),
        ));
        return Ok(items);
    }
    let (response, lifecycle, channel) = match &attempt.outcome {
        AttemptOutcome::Committed(output) => (
            response_content(output)?,
            ThreadContentLifecycle::completed(updated_at),
            if output.tool_calls.is_empty() {
                ThreadTextChannel::Final
            } else {
                ThreadTextChannel::Commentary
            },
        ),
        AttemptOutcome::Rejected { output, reason } => (
            response_content(output)?,
            ThreadContentLifecycle::failed(updated_at, reason.to_string()),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Cancelled { result: Ok(output) } => (
            response_content(output)?,
            ThreadContentLifecycle::cancelled(
                updated_at,
                "The model returned after cancellation; this output was not committed to context."
                    .into(),
            ),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Running => {
            let Some(preview) = snapshot
                .model_progress
                .as_ref()
                .filter(|preview| preview.attempt_id == attempt.attempt_id)
            else {
                return Ok(items);
            };
            let reasoning = preview
                .progress
                .reasoning
                .as_ref()
                .map(|payload| {
                    if payload.format() != "text/plain" || payload.version() != 1 {
                        return Err(ProjectionError::UnsupportedOutput(
                            "unsupported live reasoning format".into(),
                        ));
                    }
                    Ok(payload.content().to_owned())
                })
                .transpose()?;
            (
                ResponseContent {
                    text: text_content(&preview.progress.content),
                    reasoning,
                },
                ThreadContentLifecycle::streaming(),
                ThreadTextChannel::Commentary,
            )
        }
        // 已经开始的 streaming overlay 必须由同一条目的终态收束：失败、取消或中断仍然发出该
        // attempt 的 text/reasoning 终态项（正文为空），否则实时侧会留下永远 streaming 的条目。
        AttemptOutcome::Interrupted => (
            ResponseContent {
                text: String::new(),
                reasoning: None,
            },
            ThreadContentLifecycle::cancelled(
                updated_at,
                "Execution interrupted before recovery".into(),
            ),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Failed(error) => (
            ResponseContent {
                text: String::new(),
                reasoning: None,
            },
            ThreadContentLifecycle::failed(updated_at, error.to_string()),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Cancelled { result: Err(error) } => (
            ResponseContent {
                text: String::new(),
                reasoning: None,
            },
            ThreadContentLifecycle::cancelled(updated_at, error.to_string()),
            ThreadTextChannel::Commentary,
        ),
    };
    // 上述终态即使正文为空也要发出，但只对**实际开始过 streaming 的同一 identity channel**
    // 收束；`finalize.contains` 是一次 durable 事实查询的结果，因此 writer 与 live 对同一
    // identity 落相同终态，而从未开始的 channel 不会凭空出现空条目。
    let terminal_without_content = matches!(
        attempt.outcome,
        AttemptOutcome::Interrupted
            | AttemptOutcome::Failed(_)
            | AttemptOutcome::Cancelled { result: Err(_) }
    );
    if let Some(reasoning) = response.reasoning.filter(|text| !text.is_empty()) {
        items.push(ThreadItem::new(
            super::order::response_id(&attempt.attempt_id, "reasoning"),
            thread_id.into(),
            attempt.turn_id.clone(),
            ordinal,
            revision,
            created_at,
            updated_at,
            ThreadItemState::Thinking(ThreadThinkingItem::new(
                Vec::new(),
                vec![reasoning],
                lifecycle.clone(),
            )),
        ));
    } else if terminal_without_content
        && finalize.contains(&super::order::response_id(&attempt.attempt_id, "reasoning"))
    {
        items.push(ThreadItem::new(
            super::order::response_id(&attempt.attempt_id, "reasoning"),
            thread_id.into(),
            attempt.turn_id.clone(),
            ordinal,
            revision,
            created_at,
            updated_at,
            ThreadItemState::Thinking(ThreadThinkingItem::new(
                Vec::new(),
                Vec::new(),
                lifecycle.clone(),
            )),
        ));
    }
    if !response.text.is_empty() {
        items.push(ThreadItem::new(
            super::order::response_id(&attempt.attempt_id, "text"),
            thread_id.into(),
            attempt.turn_id.clone(),
            ordinal,
            revision,
            created_at,
            updated_at,
            ThreadItemState::Text(ThreadTextItem::new(
                channel,
                response.text,
                Vec::new(),
                lifecycle,
            )),
        ));
    } else if terminal_without_content
        && finalize.contains(&super::order::response_id(&attempt.attempt_id, "text"))
    {
        items.push(ThreadItem::new(
            super::order::response_id(&attempt.attempt_id, "text"),
            thread_id.into(),
            attempt.turn_id.clone(),
            ordinal,
            revision,
            created_at,
            updated_at,
            ThreadItemState::Text(ThreadTextItem::new(
                channel,
                String::new(),
                Vec::new(),
                lifecycle,
            )),
        ));
    }
    Ok(items)
}

fn inference_item(
    thread_id: &str,
    attempt: &pl_core::thread::RequestAttempt,
    created_at: i64,
    updated_at: i64,
    revision: u64,
) -> Result<ThreadItem, ProjectionError> {
    use pl_protocol::{ThreadInferenceItem, ThreadInferenceState as State};
    let model = attempt
        .request_metadata
        .as_ref()
        .and_then(|payload| pl_model::runtime::model_request_receipt(payload).ok())
        .map_or_else(String::new, |receipt| receipt.binding.requested_model);
    let state = match &attempt.outcome {
        AttemptOutcome::Running => State::Running(pl_protocol::RunningThreadInference),
        AttemptOutcome::Committed(output) => {
            let usage = &output.usage;
            State::Completed(pl_protocol::CompletedThreadInference::new(
                updated_at,
                pl_protocol::TokenUsageSnapshot {
                    prompt_tokens: usage.input_tokens.unwrap_or(0),
                    completion_tokens: usage.output_tokens.unwrap_or(0),
                    cached_prompt_tokens: usage.cache_read_tokens.unwrap_or(0),
                    cache_write_tokens: usage.cache_write_tokens.unwrap_or(0),
                    cache_miss_tokens: usage
                        .input_tokens
                        .zip(usage.cache_read_tokens)
                        .map_or(0, |(input, cached)| input.saturating_sub(cached)),
                    reasoning_tokens: usage.reasoning_tokens.unwrap_or(0),
                    inference_count: 1,
                    total_tokens: usage
                        .input_tokens
                        .zip(usage.output_tokens)
                        .and_then(|(a, b)| a.checked_add(b))
                        .unwrap_or(0),
                },
            ))
        }
        AttemptOutcome::Failed(error) => State::Failed(pl_protocol::FailedThreadInference::new(
            updated_at,
            error.to_string(),
        )),
        AttemptOutcome::Rejected { reason, .. } => State::Failed(
            pl_protocol::FailedThreadInference::new(updated_at, reason.to_string()),
        ),
        AttemptOutcome::Cancelled { .. } => {
            State::Cancelled(pl_protocol::CancelledThreadInference::new(
                updated_at,
                "Model execution cancelled".into(),
            ))
        }
        AttemptOutcome::Interrupted => {
            State::Cancelled(pl_protocol::CancelledThreadInference::new(
                updated_at,
                "Execution interrupted before recovery".into(),
            ))
        }
    };
    Ok(ThreadItem::new(
        super::order::response_id(&attempt.attempt_id, "inference"),
        thread_id.into(),
        attempt.turn_id.clone(),
        0,
        revision,
        created_at,
        updated_at,
        ThreadItemState::Inference(ThreadInferenceItem::new(
            attempt.attempt_id.clone(),
            model,
            state,
        )),
    ))
}

struct ResponseContent {
    text: String,
    reasoning: Option<String>,
}
fn response_content(output: &ModelStepOutput) -> Result<ResponseContent, ProjectionError> {
    let receipt = pl_model::runtime::model_response_receipt(output)?;
    for content in &output.content {
        match content {
            ContextContent::Text { .. } => {}
            ContextContent::Opaque { payload }
                if receipt.is_some() && payload.format() == "pl.model.assistant" => {}
            ContextContent::Opaque { payload } => {
                return Err(ProjectionError::UnsupportedOutput(format!(
                    "{} version {}",
                    payload.format(),
                    payload.version()
                )));
            }
            ContextContent::Resource { reference } => {
                return Err(ProjectionError::UnsupportedOutput(format!(
                    "resource {}",
                    reference.id()
                )));
            }
        }
    }
    Ok(ResponseContent {
        text: text_content(&output.content),
        reasoning: receipt.and_then(|receipt| receipt.response.reasoning_content),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::OpaquePayload;
    use pretty_assertions::assert_eq;

    fn output(content: Vec<ContextContent>) -> ModelStepOutput {
        ModelStepOutput {
            attempt_id: "attempt".into(),
            base_context_revision: 0,
            content,
            tool_calls: Vec::new(),
            private_context: None,
            usage: Default::default(),
        }
    }

    #[test]
    fn text_projection_preserves_chunk_boundaries_without_inserting_or_trimming_bytes() {
        let result = response_content(&output(vec![
            ContextContent::Text {
                text: "  第一段\r".into(),
            },
            ContextContent::Text {
                text: "\n第二段  ".into(),
            },
        ]))
        .unwrap();
        assert_eq!(result.text, "  第一段\r\n第二段  ");
        assert_eq!(result.reasoning, None);
    }

    #[test]
    fn unknown_model_content_cannot_silently_become_an_empty_response() {
        let original = OpaquePayload::new("custom.model-output", 8, "original content\n").unwrap();
        let output = output(vec![ContextContent::Opaque {
            payload: original.clone(),
        }]);
        assert!(response_content(&output).is_err());
        assert_eq!(
            output.content,
            vec![ContextContent::Opaque { payload: original }]
        );
    }
}
