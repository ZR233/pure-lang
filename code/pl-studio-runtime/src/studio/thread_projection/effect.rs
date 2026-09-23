//! Incremental durable history projection from one core effect and its matching current state.

use std::collections::{BTreeMap, BTreeSet};

use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, ThreadSnapshot, input::InputChange};
use pl_protocol::{ThreadItem, ThreadItemState, ThreadRawItem, ThreadTurnItem};

use super::{ProjectionError, order};

/// Projects only newly accepted visible input from this effect. Admission cannot wait
/// for a history reader; later state transitions and old identities are resolved by
/// the durable writer against the canonical history.
pub(in crate::studio) fn project_accepted_inputs(
    thread_id: &str,
    state: &ThreadSnapshot,
    effect: &ThreadEffectBatch,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut items = Vec::new();
    for change in effect.inputs.iter() {
        let InputChange::Accepted(record) = change else {
            continue;
        };
        if record.accepted_sequence != effect.sequence {
            return Err(ProjectionError::MissingInput(record.input.id.clone()));
        }
        let input = state
            .inputs
            .iter()
            .find(|input| input.input.id == record.input.id)
            .ok_or_else(|| ProjectionError::MissingInput(record.input.id.clone()))?;
        if let Some(item) = super::inputs::project_input(
            thread_id,
            state,
            input,
            0,
            effect.committed_at,
            effect.sequence,
            effect.committed_at,
        )? {
            items.push(item);
        }
    }
    Ok(items)
}

/// Timeline items one committed effect contributes, plus the identities it references but cannot
/// materialize from the state that effect carried.
///
/// Both the durable writer and the live projection resolve identities in two passes: enumerate the
/// ids an effect touches, read their durable items and reservations, then project with that phase. A
/// consuming commit publishes a checkpoint that still holds the input and prunes the resident body
/// immediately afterwards, while every later effect of the same Turn — including its terminal one —
/// keeps referencing that input id. The body is therefore legitimately absent although the input
/// item is already durable, so enumeration must not fail: the identity is reported here and the
/// durable pass decides from the durable item itself whether the fact is complete.
pub(in crate::studio) struct EffectProjection {
    pub(in crate::studio) items: Vec<ThreadItem>,
    /// Pruned input identities this effect references whose durable item the caller did not supply.
    pub(in crate::studio) unresolved_inputs: BTreeSet<String>,
    /// Calls whose producing attempt was pruned; the original invocation belongs to history.
    pub(in crate::studio) unresolved_calls: BTreeSet<String>,
}

impl EffectProjection {
    /// Rejects a projection whose pruned input identities have no durable item.
    ///
    /// Such a body is gone from the state that carried the effect and from the durable fact source,
    /// so it can never be materialized later. The durable writer reports this as a real persistence
    /// error instead of committing a timeline with a hole and leaving a stale running Turn behind.
    pub(in crate::studio) fn ensure_complete(&self) -> Result<(), ProjectionError> {
        if !self.unresolved_inputs.is_empty() {
            return Err(ProjectionError::MissingDurableInput(
                self.unresolved_inputs
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        if !self.unresolved_calls.is_empty() {
            return Err(ProjectionError::MissingDurableToolCall(
                self.unresolved_calls
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        Ok(())
    }
}

pub(in crate::studio) fn project_effect_items(
    thread: &pl_protocol::Thread,
    state: &ThreadSnapshot,
    effect: &ThreadEffectBatch,
    existing: &BTreeMap<String, ThreadItem>,
    reserved: &BTreeMap<String, u64>,
) -> Result<EffectProjection, ProjectionError> {
    let mut items = Vec::new();
    let mut input_ids = BTreeSet::new();
    let mut changed_inputs = BTreeSet::new();
    let mut call_ids = BTreeSet::new();
    let mut turn_ids = BTreeSet::new();
    let mut unresolved_calls = BTreeSet::new();

    for change in effect.inputs.iter() {
        let id = match change {
            InputChange::Accepted(record) => record.input.id.as_str(),
            InputChange::Transition { id, .. } => id.as_str(),
        };
        input_ids.insert(id);
        // The effect itself changes this input, so its own committed checkpoint must still carry the
        // body: a missing record here is an invariant violation, not a pruned identity.
        changed_inputs.insert(id);
    }
    if let Some(turn) = &effect.turn {
        turn_ids.insert(turn.turn_id.as_str());
        if let Some(input_id) = &turn.input_id {
            input_ids.insert(input_id);
        }
    }
    let mut unresolved_inputs = BTreeSet::new();
    for id in input_ids {
        let Some(input) = state.inputs.iter().find(|record| record.input.id == id) else {
            if changed_inputs.contains(id) {
                return Err(ProjectionError::MissingInput(id.into()));
            }
            // A Turn keeps referencing the input id whose body its consuming commit pruned from the
            // resident state. The item itself was written by the effect that admitted or consumed it
            // and its content never changes afterwards, so the durable item is the committed
            // metadata: when the caller supplies it this effect leaves it untouched, and otherwise
            // the identity is reported so `ensure_complete` can fail the incomplete fact loudly.
            if !existing.contains_key(id) {
                unresolved_inputs.insert(id.to_string());
            }
            continue;
        };
        let (ordinal, created_at) = stamp(existing, reserved, id, effect.committed_at);
        if let Some(item) = super::inputs::project_input(
            &thread.id,
            state,
            input,
            ordinal,
            created_at,
            effect.sequence,
            effect.committed_at,
        )? {
            items.push(item);
        }
    }

    if let Some(parent_id) = thread.parent_thread_id.as_deref() {
        let source_id = format!("agent:{parent_id}");
        let consumed_turn = effect
            .attempt
            .as_ref()
            .filter(|_| effect.consumed_messages.is_some())
            .map(|attempt| attempt.turn_id.as_str());
        for record in state.inbox.iter().filter(|record| {
            record.message.source_id == source_id
                && (effect
                    .inbox
                    .iter()
                    .any(|changed| changed.message.id == record.message.id)
                    || effect
                        .consumed_messages
                        .is_some_and(|through| record.sequence <= through))
        }) {
            let id = order::message_id(&record.message.id);
            let (ordinal, created_at) = stamp(existing, reserved, &id, effect.committed_at);
            let turn_id = existing
                .get(&id)
                .map(|item| item.turn_id.clone())
                .filter(|turn| !turn.is_empty())
                .or_else(|| consumed_turn.map(str::to_owned))
                .unwrap_or_default();
            let payload = &record.message.payload;
            let state = if payload.format() == "text/plain" && payload.version() == 1 {
                ThreadItem::completed_parent_agent_message(
                    id.clone(),
                    thread.id.clone(),
                    turn_id.clone(),
                    payload.content().into(),
                    Vec::new(),
                    created_at,
                )
                .state()
                .clone()
            } else {
                ThreadItemState::Raw(ThreadRawItem {
                    payloads: vec![super::raw_payload(payload)],
                    notice: "Unsupported saved parent message encoding".into(),
                    recorded_at: created_at,
                })
            };
            items.push(ThreadItem::new(
                id,
                thread.id.clone(),
                turn_id,
                ordinal,
                effect.sequence,
                created_at,
                effect.committed_at,
                state,
            ));
        }
    }

    if let Some(attempt) = &effect.attempt {
        turn_ids.insert(attempt.turn_id.as_str());
        let current = state
            .attempts
            .iter()
            .find(|current| current.attempt_id == attempt.attempt_id)
            .ok_or_else(|| ProjectionError::MissingAttempt(attempt.attempt_id.clone()))?;
        let inference_id = order::response_id(&attempt.attempt_id, "inference");
        let (ordinal, created_at) = stamp(existing, reserved, &inference_id, effect.committed_at);
        // 收束集合来自同一份 durable 事实：已存在的 item 或已预留的 identity。writer 与 live 都
        // 传同一张 existing/reserved，因此同一 item 的终态一致，未开始的 channel 不会凭空出现。
        let finalize = super::responses::started_channels(
            current,
            existing.keys().chain(reserved.keys()).cloned(),
            |id| existing.contains_key(id) || reserved.contains_key(id),
        );
        let mut projected = super::responses::project_attempt(
            &thread.id,
            state,
            current,
            ordinal,
            created_at,
            effect.sequence,
            effect.committed_at,
            &finalize,
        )?;
        super::responses::finalize_missing_presentation(
            &thread.id,
            current,
            effect.sequence,
            effect.committed_at,
            &finalize,
            existing,
            &mut projected,
        )?;
        canonicalize(&mut projected, existing, reserved, effect.committed_at);
        items.extend(projected);
        if let AttemptOutcome::Committed(output) = &attempt.outcome {
            for call in &output.tool_calls {
                call_ids.insert(call.call_id.as_str());
            }
        }
    }
    for task in effect.tasks.iter() {
        call_ids.insert(task.call_id.as_str());
        turn_ids.insert(task.turn_id.as_str());
    }
    for permission in effect.permissions.iter() {
        call_ids.insert(permission.call_id.as_str());
    }
    for delivery in effect.deliveries.iter() {
        call_ids.insert(delivery.call_id.as_str());
    }

    for call_id in call_ids {
        let saved_call = state
            .attempts
            .iter()
            .find_map(|attempt| match &attempt.outcome {
                AttemptOutcome::Committed(output) => output
                    .tool_calls
                    .iter()
                    .find(|call| call.call_id == call_id)
                    .map(|call| (attempt.turn_id.as_str(), call)),
                _ => None,
            });
        let tool_id = order::tool_id(call_id);
        let (turn_id, mut projected) = if let Some((turn_id, call)) = saved_call {
            let (ordinal, created_at) = stamp(existing, reserved, &tool_id, effect.committed_at);
            (
                turn_id,
                super::tools::project_tool_call(
                    &thread.id,
                    state,
                    turn_id,
                    call,
                    ordinal,
                    created_at,
                    effect.sequence,
                    effect.committed_at,
                )?,
            )
        } else if let Some(saved) = existing.get(&tool_id) {
            (
                saved.turn_id.as_str(),
                super::tools::project_saved_tool_call(
                    &thread.id,
                    state,
                    saved,
                    effect.sequence,
                    effect.committed_at,
                )?,
            )
        } else {
            unresolved_calls.insert(tool_id);
            continue;
        };
        turn_ids.insert(turn_id);
        canonicalize(&mut projected, existing, reserved, effect.committed_at);
        items.extend(projected);
        if let Some(delivery) = effect
            .deliveries
            .iter()
            .find(|delivery| delivery.call_id == call_id)
            && let Some(mut completion) = super::completions::project_completion(
                &thread.id,
                turn_id,
                delivery,
                effect.sequence,
                effect.committed_at,
            )
        {
            canonicalize(
                std::slice::from_mut(&mut completion),
                existing,
                reserved,
                effect.committed_at,
            );
            items.push(completion);
        }
    }

    if effect.replacements.iter().any(|replacement| {
        replacement.reason == pl_core::thread::ContextReplacementReason::Compaction
    }) {
        for change in effect.extensions.iter() {
            let pl_core::thread::extensions::ExtensionChange::Put { id, record } = change else {
                continue;
            };
            let Some(receipt) = super::compactions::receipt(&record.payload)? else {
                continue;
            };
            if receipt.implementation.is_none() {
                continue;
            }
            let item_id = order::compaction_id(id);
            let (ordinal, created_at) = stamp(existing, reserved, &item_id, effect.committed_at);
            items.push(ThreadItem::new(
                item_id,
                thread.id.clone(),
                receipt.turn_id,
                ordinal,
                record.revision,
                created_at,
                effect.committed_at,
                ThreadItemState::ContextCompaction(pl_protocol::ThreadContextCompactionItem::new(
                    None,
                    None,
                    effect.committed_at,
                )),
            ));
        }
    }

    for turn_id in turn_ids {
        let Some(record) = state.turns.iter().find(|record| record.turn_id == turn_id) else {
            continue;
        };
        let id = order::turn_id(turn_id);
        let (ordinal, created_at) = stamp(existing, reserved, &id, effect.committed_at);
        let turn = super::turns::project_turn(
            &thread.id,
            state,
            record,
            created_at,
            effect.committed_at,
            effect.sequence,
        )?;
        items.push(ThreadItem::new(
            id,
            thread.id.clone(),
            turn.id.clone(),
            ordinal,
            turn.revision,
            created_at,
            turn.updated_at,
            ThreadItemState::Turn(ThreadTurnItem::new(turn.state).with_input_id(turn.input_id)),
        ));
    }

    items.sort_by_key(|item| (item.ordinal == 0, item.ordinal));
    // An empty phase is only for discovering provisional identities. Once the caller supplies
    // committed or Session-assigned orders, no item may fall through to storage-side allocation.
    if (!existing.is_empty() || !reserved.is_empty())
        && let Some(item) = items.iter().find(|item| item.ordinal == 0)
    {
        return Err(ProjectionError::ItemOrder(item.id.clone()));
    }
    Ok(EffectProjection {
        items,
        unresolved_inputs,
        unresolved_calls,
    })
}

/// Ordinal phase of one identity: the committed item, an in-memory Session assignment, or zero
/// only during the first identity-discovery projection. The final projection rejects zero.
fn stamp(
    existing: &BTreeMap<String, ThreadItem>,
    reserved: &BTreeMap<String, u64>,
    id: &str,
    fallback: i64,
) -> (u64, i64) {
    if let Some(item) = existing.get(id) {
        return (item.ordinal, item.created_at);
    }
    (reserved.get(id).copied().unwrap_or(0), fallback)
}

fn canonicalize(
    items: &mut [ThreadItem],
    existing: &BTreeMap<String, ThreadItem>,
    reserved: &BTreeMap<String, u64>,
    fallback: i64,
) {
    for item in items {
        let (ordinal, created_at) = stamp(existing, reserved, &item.id, fallback);
        item.ordinal = ordinal;
        item.created_at = created_at;
    }
}
