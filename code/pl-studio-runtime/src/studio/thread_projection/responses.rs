//! Saved model output projection, without rerunning an adapter or advancing a session.
use super::{ProjectionError, content::text_content};
use pl_core::{
    context::ContextContent,
    model::ModelStepOutput,
    thread::{AttemptOutcome, ThreadSnapshot},
};
use pl_model::completion::{
    CompletionPresentationItem, CompletionPresentationItemKind, CompletionPresentationPart,
    CompletionPresentationPartKind,
};
use pl_protocol::{
    ThreadContentLifecycle, ThreadItem, ThreadItemState, ThreadTextChannel, ThreadTextItem,
    ThreadThinkingItem,
};
use std::collections::{BTreeMap, BTreeSet};

/// Terminal streaming identities an attempt actually started.
///
/// Aggregate preview identities are probed even for itemized responses so an earlier preview can
/// be closed without emitting duplicate aggregate content. Provider part identities use the same
/// function as terminal projection and the caller's durable item/reservation fact.
pub(in crate::studio) fn started_channels(
    attempt: &pl_core::thread::RequestAttempt,
    known_ids: impl IntoIterator<Item = String>,
    started: impl Fn(&str) -> bool,
) -> BTreeSet<String> {
    let mut channels = BTreeSet::new();
    for kind in ["reasoning", "text"] {
        let id = super::order::response_id(&attempt.attempt_id, kind);
        if started(&id) {
            channels.insert(id);
        }
    }
    let output = match &attempt.outcome {
        AttemptOutcome::Committed(output)
        | AttemptOutcome::Rejected { output, .. }
        | AttemptOutcome::Cancelled { result: Ok(output) } => Some(output),
        _ => None,
    };
    // A malformed receipt is projected as Raw by project_attempt, not as presentation content.
    if let Some(Ok(Some(receipt))) = output.map(pl_model::runtime::model_response_receipt) {
        for id in super::order::presentation_ids(
            &attempt.attempt_id,
            &receipt.response.presentation_items,
        ) {
            if started(&id) {
                channels.insert(id);
            }
        }
    }
    let prefix = super::order::presentation_prefix(&attempt.attempt_id);
    for id in known_ids {
        if id.starts_with(&prefix) && started(&id) {
            channels.insert(id);
        }
    }
    channels
}

/// Closes preview parts absent from a terminal receipt (notably a bounded failure preview).
/// The previous item is required to preserve its channel and partially streamed content.
pub(super) fn finalize_missing_presentation(
    thread_id: &str,
    attempt: &pl_core::thread::RequestAttempt,
    revision: u64,
    updated_at: i64,
    finalize: &BTreeSet<String>,
    existing: &BTreeMap<String, ThreadItem>,
    items: &mut Vec<ThreadItem>,
) -> Result<(), ProjectionError> {
    let lifecycle = match &attempt.outcome {
        AttemptOutcome::Running => return Ok(()),
        AttemptOutcome::Committed(_) => ThreadContentLifecycle::completed(updated_at),
        AttemptOutcome::Rejected { reason, .. } => {
            ThreadContentLifecycle::failed(updated_at, reason.to_string())
        }
        AttemptOutcome::Failed(error) => {
            ThreadContentLifecycle::failed(updated_at, error.to_string())
        }
        AttemptOutcome::Cancelled { result: Ok(_) } => ThreadContentLifecycle::cancelled(
            updated_at,
            "The model returned after cancellation; this output was not committed to context."
                .into(),
        ),
        AttemptOutcome::Cancelled { result: Err(error) } => {
            ThreadContentLifecycle::cancelled(updated_at, error.to_string())
        }
        AttemptOutcome::Interrupted => ThreadContentLifecycle::cancelled(
            updated_at,
            "Execution interrupted before recovery".into(),
        ),
    };
    let prefix = super::order::presentation_prefix(&attempt.attempt_id);
    let emitted = items
        .iter()
        .map(|item| item.id.clone())
        .collect::<BTreeSet<_>>();
    for id in finalize.iter().filter(|id| id.starts_with(&prefix)) {
        if emitted.contains(id) {
            continue;
        }
        let previous = existing.get(id).ok_or_else(|| {
            ProjectionError::History(format!("missing previously started provider item {id}"))
        })?;
        if previous.is_terminal() {
            continue;
        }
        let state = match previous.state() {
            ThreadItemState::Text(text) => ThreadItemState::Text(ThreadTextItem::new(
                text.channel(),
                text.text().to_owned(),
                text.attachments().to_vec(),
                lifecycle.clone(),
            )),
            ThreadItemState::Thinking(thinking) => {
                ThreadItemState::Thinking(ThreadThinkingItem::new(
                    thinking.summary().to_vec(),
                    thinking.content().to_vec(),
                    lifecycle.clone(),
                ))
            }
            _ => {
                return Err(ProjectionError::UnsupportedOutput(
                    "started provider presentation has no content state".into(),
                ));
            }
        };
        items.push(ThreadItem::new(
            id.clone(),
            thread_id.into(),
            attempt.turn_id.clone(),
            previous.ordinal,
            revision,
            previous.created_at,
            updated_at,
            state,
        ));
    }
    Ok(())
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
            (
                progress_content(&preview.progress)?,
                ThreadContentLifecycle::streaming(),
                ThreadTextChannel::Commentary,
            )
        }
        // 已经开始的 overlay 由同一身份的终态收束；重启中断没有可恢复的正文。
        AttemptOutcome::Interrupted => (
            ResponseContent {
                text: String::new(),
                reasoning: None,
                presentation_items: Vec::new(),
            },
            ThreadContentLifecycle::cancelled(
                updated_at,
                "Execution interrupted before recovery".into(),
            ),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Failed(error) => (
            failure_content(error)?,
            ThreadContentLifecycle::failed(updated_at, error.to_string()),
            ThreadTextChannel::Commentary,
        ),
        AttemptOutcome::Cancelled { result: Err(error) } => (
            failure_content(error)?,
            ThreadContentLifecycle::cancelled(updated_at, error.to_string()),
            ThreadTextChannel::Commentary,
        ),
    };
    // 上述终态即使正文为空也要发出，但只对**实际开始过 streaming 的同一 identity channel**
    // 收束；`finalize.contains` 是一次 durable 事实查询的结果，因此 writer 与 live 对同一
    // identity 落相同终态，而从未开始的 channel 不会凭空出现空条目。
    let terminates_live_channels = !matches!(attempt.outcome, AttemptOutcome::Running);
    if !response.presentation_items.is_empty() {
        items.extend(project_presentation_items(
            thread_id,
            attempt,
            ordinal,
            created_at,
            revision,
            updated_at,
            &response.presentation_items,
            &lifecycle,
        )?);
        // The legacy preview has only two aggregate identities. When one was already started,
        // finalize it without repeating the provider's aggregate text or reasoning.
        if terminates_live_channels
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
        if terminates_live_channels
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
                    ThreadTextChannel::Commentary,
                    String::new(),
                    Vec::new(),
                    lifecycle,
                )),
            ));
        }
        return Ok(items);
    }
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
    } else if terminates_live_channels
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
    } else if terminates_live_channels
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

#[allow(clippy::too_many_arguments)]
fn project_presentation_items(
    thread_id: &str,
    attempt: &pl_core::thread::RequestAttempt,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
    presentation_items: &[CompletionPresentationItem],
    lifecycle: &ThreadContentLifecycle,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut projected = Vec::new();
    let mut ids = BTreeSet::new();
    for item in presentation_items {
        for part in item
            .parts
            .iter()
            .map(Some)
            .chain(item.parts.is_empty().then_some(None))
        {
            let id = super::order::presentation_id(
                &attempt.attempt_id,
                &item.provider_item_id,
                part.map(super::order::presentation_part),
            );
            if !ids.insert(id.clone()) {
                return Err(ProjectionError::UnsupportedOutput(
                    "duplicate provider presentation identity".into(),
                ));
            }
            let state = match (item.kind, part) {
                (CompletionPresentationItemKind::Text(channel), None)
                | (
                    CompletionPresentationItemKind::Text(channel),
                    Some(CompletionPresentationPart {
                        kind: CompletionPresentationPartKind::OutputText,
                        ..
                    }),
                ) => {
                    let channel = match channel {
                        pl_protocol::trace::TraceTextChannel::User => ThreadTextChannel::User,
                        pl_protocol::trace::TraceTextChannel::Commentary => {
                            ThreadTextChannel::Commentary
                        }
                        pl_protocol::trace::TraceTextChannel::Final => ThreadTextChannel::Final,
                    };
                    ThreadItemState::Text(ThreadTextItem::new(
                        channel,
                        part.map_or_else(String::new, |part| part.text.clone()),
                        Vec::new(),
                        lifecycle.clone(),
                    ))
                }
                (CompletionPresentationItemKind::Reasoning, None)
                | (
                    CompletionPresentationItemKind::Reasoning,
                    Some(CompletionPresentationPart {
                        kind: CompletionPresentationPartKind::ReasoningText,
                        ..
                    }),
                ) => ThreadItemState::Thinking(ThreadThinkingItem::new(
                    Vec::new(),
                    part.map_or_else(Vec::new, |part| vec![part.text.clone()]),
                    lifecycle.clone(),
                )),
                (
                    CompletionPresentationItemKind::Reasoning,
                    Some(CompletionPresentationPart {
                        kind: CompletionPresentationPartKind::SummaryText,
                        text,
                        ..
                    }),
                ) => ThreadItemState::Thinking(ThreadThinkingItem::new(
                    vec![text.clone()],
                    Vec::new(),
                    lifecycle.clone(),
                )),
                _ => {
                    return Err(ProjectionError::UnsupportedOutput(
                        "provider presentation part does not match its item".into(),
                    ));
                }
            };
            projected.push(ThreadItem::new(
                id,
                thread_id.into(),
                attempt.turn_id.clone(),
                ordinal,
                revision,
                created_at,
                updated_at,
                state,
            ));
        }
    }
    Ok(projected)
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
    presentation_items: Vec<CompletionPresentationItem>,
}

fn failure_content(error: &pl_core::model::ModelError) -> Result<ResponseContent, ProjectionError> {
    let receipt = error
        .details
        .as_ref()
        .is_some_and(|details| details.format() == "pl.model.failure")
        .then(|| pl_model::runtime::model_failure_receipt(error))
        .transpose()?
        .flatten();
    let mut content = match receipt
        .as_ref()
        .and_then(|receipt| receipt.partial_progress.as_ref())
    {
        Some(progress) => progress_content(progress)?,
        None => ResponseContent {
            text: String::new(),
            reasoning: None,
            presentation_items: Vec::new(),
        },
    };
    if let Some(receipt) = receipt
        && !receipt.presentation_items.is_empty()
    {
        content.presentation_items = receipt.presentation_items;
    }
    Ok(content)
}

fn progress_content(
    progress: &pl_core::model::ModelProgress,
) -> Result<ResponseContent, ProjectionError> {
    let presentation_items = progress
        .presentation
        .iter()
        .map(|payload| {
            if payload.format() != "pl.model.presentation-item" || payload.version() != 1 {
                return Err(ProjectionError::UnsupportedOutput(
                    "unsupported model presentation preview".into(),
                ));
            }
            serde_json::from_str(payload.content()).map_err(ProjectionError::Encoding)
        })
        .collect::<Result<Vec<CompletionPresentationItem>, _>>()?;
    let reasoning = progress
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
    Ok(ResponseContent {
        text: text_content(&progress.content),
        reasoning,
        presentation_items,
    })
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
    let (reasoning, presentation_items) = receipt.map_or_else(
        || (None, Vec::new()),
        |receipt| {
            (
                receipt.response.reasoning_content,
                receipt.response.presentation_items,
            )
        },
    );
    Ok(ResponseContent {
        text: text_content(&output.content),
        reasoning,
        presentation_items,
    })
}
