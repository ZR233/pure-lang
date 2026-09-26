//! Saved model output projection, without rerunning an adapter or advancing a session.
use super::{ProjectionError, content::text_content};
use pl_core::{
    context::ContextContent,
    model::{
        AggregateChannel, ModelStepOutput, ModelTextChannel, ObservedItemKind,
        ObservedPartIdentity, ObservedPartKind,
    },
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

/// Whether a commit's own receipt cannot carry the provider presentation parts it started.
///
/// Only a bounded reception qualifies: a failed, cancelled or interrupted stream stopped before the
/// provider's authoritative item, so the prefix the window already received stays on the identity it
/// was shown on. A normally received response ([`AttemptOutcome::Committed`]) — and a response the
/// workflow rejected after receiving it ([`AttemptOutcome::Rejected`]) — always carries its complete
/// provider parts, so its committed content is never taken from a subscriber's view of a preview.
pub(in crate::studio) fn keeps_bounded_preview(outcome: &AttemptOutcome) -> bool {
    !matches!(
        outcome,
        AttemptOutcome::Committed(_) | AttemptOutcome::Rejected { .. }
    )
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
    // The provider itemized this attempt, so its canonical content is the provider parts — never a
    // second aggregate identity for the same text, and never an empty `reasoning` row that no provider
    // part produced. A bounded reception closes the started identity with the prefix the window
    // already received; a normally received response always carries those parts in its own receipt,
    // so an empty part set here means the authoritative output was lost. Failing closed keeps a
    // partial preview body from being committed as that response's content.
    if finalize
        .iter()
        .any(|id| id.starts_with(&super::order::presentation_prefix(&attempt.attempt_id)))
    {
        if keeps_bounded_preview(&attempt.outcome) {
            return Ok(items);
        }
        return Err(ProjectionError::UnsupportedOutput(
            "terminal receipt lost its committed provider presentation identity".into(),
        ));
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

/// Projects the live observation of one attempt, without re-decoding a producer payload.
///
/// An aggregate observation is the preview of a whole channel before the adapter itemizes the
/// response; provider observations regroup by their stable item identity. This is a read of the
/// observation snapshot, so it materializes text only where this projection needs it.
fn progress_content(
    progress: &pl_core::model::ModelProgress,
) -> Result<ResponseContent, ProjectionError> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut items: Vec<CompletionPresentationItem> = Vec::new();
    for part in progress.parts() {
        let identity = match part.identity() {
            ObservedPartIdentity::Aggregate { channel } => {
                match channel {
                    AggregateChannel::Text => text.push_str(&part.text()),
                    AggregateChannel::Reasoning => reasoning.push_str(&part.text()),
                }
                continue;
            }
            ObservedPartIdentity::Provider(identity) => identity,
        };
        let kind = match identity.item_kind {
            ObservedItemKind::Text(channel) => {
                CompletionPresentationItemKind::Text(trace_channel(channel))
            }
            ObservedItemKind::Reasoning => CompletionPresentationItemKind::Reasoning,
        };
        let part_kind = match identity.part {
            ObservedPartKind::OutputText => CompletionPresentationPartKind::OutputText,
            ObservedPartKind::ReasoningText => CompletionPresentationPartKind::ReasoningText,
            ObservedPartKind::SummaryText => CompletionPresentationPartKind::SummaryText,
        };
        if !matches!(
            (identity.item_kind, part_kind),
            (
                ObservedItemKind::Text(_),
                CompletionPresentationPartKind::OutputText
            ) | (
                ObservedItemKind::Reasoning,
                CompletionPresentationPartKind::ReasoningText
                    | CompletionPresentationPartKind::SummaryText
            )
        ) {
            return Err(ProjectionError::UnsupportedOutput(
                "provider presentation part does not match its item".into(),
            ));
        }
        let observed_part = CompletionPresentationPart {
            content_index: identity.content_index,
            provider_part_id: None,
            kind: part_kind,
            text: part.text(),
        };
        let item_id: &str = identity.item_id.as_ref();
        // The output index can arrive after streaming starts, so only the provider item id groups
        // parts; a late index completes the item instead of starting a second one.
        match items
            .iter_mut()
            .find(|item| item.provider_item_id == item_id)
        {
            Some(item) => {
                item.output_index = item.output_index.or(identity.output_index);
                item.parts.push(observed_part);
            }
            None => items.push(CompletionPresentationItem {
                provider_item_id: item_id.to_owned(),
                output_index: identity.output_index,
                kind,
                parts: vec![observed_part],
            }),
        }
    }
    Ok(ResponseContent {
        text,
        reasoning: (!reasoning.is_empty()).then_some(reasoning),
        presentation_items: items,
    })
}

fn trace_channel(channel: ModelTextChannel) -> pl_protocol::trace::TraceTextChannel {
    match channel {
        ModelTextChannel::User => pl_protocol::trace::TraceTextChannel::User,
        ModelTextChannel::Commentary => pl_protocol::trace::TraceTextChannel::Commentary,
        ModelTextChannel::Final => pl_protocol::trace::TraceTextChannel::Final,
    }
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
