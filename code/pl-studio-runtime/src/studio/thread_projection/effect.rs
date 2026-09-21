//! Incremental durable history projection from one core effect and its matching current state.

use std::collections::{BTreeMap, BTreeSet};

use pl_core::thread::{AttemptOutcome, ThreadEffectBatch, ThreadSnapshot, input::InputChange};
use pl_protocol::{ThreadItem, ThreadItemState, ThreadRawItem, ThreadTurnItem};

use super::{ProjectionError, order};

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
}

impl EffectProjection {
    /// Rejects a projection whose pruned input identities have no durable item.
    ///
    /// Such a body is gone from the state that carried the effect and from the durable fact source,
    /// so it can never be materialized later. The durable writer reports this as a real persistence
    /// error instead of committing a timeline with a hole and leaving a stale running Turn behind.
    pub(in crate::studio) fn ensure_complete(&self) -> Result<(), ProjectionError> {
        if self.unresolved_inputs.is_empty() {
            return Ok(());
        }
        Err(ProjectionError::MissingDurableInput(
            self.unresolved_inputs
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(","),
        ))
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
        let finalize = super::responses::started_channels(&attempt.attempt_id, |id| {
            existing.contains_key(id) || reserved.contains_key(id)
        });
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
        let (turn_id, call) = state
            .attempts
            .iter()
            .find_map(|attempt| match &attempt.outcome {
                AttemptOutcome::Committed(output) => output
                    .tool_calls
                    .iter()
                    .find(|call| call.call_id == call_id)
                    .map(|call| (attempt.turn_id.as_str(), call)),
                _ => None,
            })
            .ok_or_else(|| ProjectionError::DuplicateCall(call_id.into()))?;
        turn_ids.insert(turn_id);
        let tool_id = order::tool_id(call_id);
        let (ordinal, created_at) = stamp(existing, reserved, &tool_id, effect.committed_at);
        let mut projected = super::tools::project_tool_call(
            &thread.id,
            state,
            turn_id,
            call,
            ordinal,
            created_at,
            effect.sequence,
            effect.committed_at,
        )?;
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
    Ok(EffectProjection {
        items,
        unresolved_inputs,
    })
}

/// Ordinal phase of one identity: the durable item when it exists, otherwise the ordinal a live
/// reservation already took, otherwise zero so the durable writer allocates it on insert.
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

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::OpaquePayload;
    use pl_core::thread::{
        TurnOutcome, TurnRecord, TurnState,
        input::{InputDelivery, InputRecord, InputState, ThreadInput, input_identity},
    };
    use std::sync::Arc;

    const INPUT_ID: &str = "skill-discovery-input";
    const TURN_ID: &str = "turn-1";
    const ATTEMPT_ID: &str = "attempt-1";

    fn thread() -> pl_protocol::Thread {
        let (_, thread) = crate::studio::store::directory::DirectoryDelta::register_root_thread(
            "thread-projection".into(),
            "project-projection",
            "skill discovery",
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            "/tmp/workspace".into(),
        );
        thread
    }

    fn input_record() -> InputRecord {
        InputRecord {
            accepted_sequence: 3,
            delivery: InputDelivery::NextTurn,
            ordinal: 1,
            revision: 5,
            state: InputState::Consumed {
                turn_id: TURN_ID.into(),
                attempt_id: ATTEMPT_ID.into(),
            },
            input: ThreadInput {
                id: INPUT_ID.into(),
                payload: OpaquePayload::new(
                    "pl.studio.prompt",
                    1,
                    r#"{"text":"skill discovery","presentation":"visible","attachments":[]}"#,
                )
                .expect("static product payload"),
                context: Vec::new(),
            },
        }
    }

    fn turn_record() -> TurnRecord {
        TurnRecord {
            elapsed_ms: Some(5),
            input_id: Some(INPUT_ID.into()),
            turn_id: TURN_ID.into(),
            state: TurnState::Finished(TurnOutcome::Completed),
            model_steps: 1,
        }
    }

    /// The state a terminal commit carries: the Turn survives, its input body was already pruned
    /// into the bounded terminal identity ledger.
    fn pruned_state() -> ThreadSnapshot {
        ThreadSnapshot {
            commit_sequence: 8,
            turns: Arc::from(vec![turn_record()]),
            terminal_inputs: Arc::from(vec![input_identity(&input_record())]),
            ..Default::default()
        }
    }

    fn committed_state() -> ThreadSnapshot {
        ThreadSnapshot {
            commit_sequence: 8,
            inputs: Arc::from(vec![input_record()]),
            turns: Arc::from(vec![turn_record()]),
            ..Default::default()
        }
    }

    fn terminal_effect(thread_id: &str) -> ThreadEffectBatch {
        ThreadEffectBatch {
            thread_id: thread_id.into(),
            sequence: 8,
            committed_at: 1_700_000_100,
            turn: Some(turn_record()),
            ..Default::default()
        }
    }

    /// The input item the effect that admitted or consumed the input already wrote.
    fn durable_input_item(thread_id: &str) -> ThreadItem {
        ThreadItem::new(
            INPUT_ID.into(),
            thread_id.into(),
            TURN_ID.into(),
            1,
            5,
            1_700_000_000,
            1_700_000_000,
            ThreadItemState::Raw(ThreadRawItem {
                payloads: Vec::new(),
                notice: "durable input item".into(),
                recorded_at: 1_700_000_000,
            }),
        )
    }

    fn assert_terminal_turn_item(items: &[ThreadItem]) {
        let item = items
            .iter()
            .find(|item| item.id == super::order::turn_id(TURN_ID))
            .expect("the terminal Turn must project a Turn item");
        let ThreadItemState::Turn(turn) = item.state() else {
            panic!("the Turn identity must project a Turn item");
        };
        assert_eq!(
            turn.input_id(),
            Some(INPUT_ID),
            "the Turn keeps the stable identity of the input its commit consumed"
        );
        assert!(matches!(turn.state(), pl_protocol::TurnState::Completed(_)));
    }

    /// A terminal Turn keeps referencing the input its consuming commit pruned from the resident
    /// state. Its durable item is the committed metadata that completes the projection; without it
    /// the fact is irrecoverable and must be reported instead of projected with a hole.
    #[test]
    fn terminal_turn_resolves_its_pruned_input_from_the_durable_item() {
        let thread = thread();
        let state = pruned_state();
        let effect = terminal_effect(&thread.id);

        let enumeration =
            project_effect_items(&thread, &state, &effect, &BTreeMap::new(), &BTreeMap::new())
                .expect("a pruned input body must not fail effect enumeration");
        assert_eq!(
            enumeration
                .unresolved_inputs
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![INPUT_ID.to_string()]
        );
        assert!(enumeration.items.iter().all(|item| item.id != INPUT_ID));
        assert!(
            enumeration.ensure_complete().is_err(),
            "a pruned identity without a durable item is an irrecoverable fact"
        );
        assert_terminal_turn_item(&enumeration.items);
        assert_eq!(
            enumeration
                .items
                .iter()
                .find(|item| item.id == super::order::turn_id(TURN_ID))
                .expect("terminal Turn item")
                .revision,
            effect.sequence
        );

        let existing = BTreeMap::from([(INPUT_ID.to_string(), durable_input_item(&thread.id))]);
        let projected = project_effect_items(&thread, &state, &effect, &existing, &BTreeMap::new())
            .expect("the durable input item completes the pruned reference");
        assert!(projected.unresolved_inputs.is_empty());
        projected.ensure_complete().expect("complete projection");
        assert!(
            projected.items.iter().all(|item| item.id != INPUT_ID),
            "the already durable input payload must not be rewritten"
        );
        assert_terminal_turn_item(&projected.items);
    }

    /// An effect that changes an input must still carry that input's own committed body: a missing
    /// record there is an invariant violation, not a pruned identity.
    #[test]
    fn effect_that_changes_an_input_requires_its_own_committed_body() {
        let thread = thread();
        let effect = ThreadEffectBatch {
            thread_id: thread.id.clone(),
            sequence: 9,
            committed_at: 1_700_000_130,
            inputs: Arc::from(vec![InputChange::Transition {
                id: INPUT_ID.into(),
                revision: 9,
                state: InputState::Consumed {
                    turn_id: TURN_ID.into(),
                    attempt_id: ATTEMPT_ID.into(),
                },
            }]),
            ..Default::default()
        };
        let error = match project_effect_items(
            &thread,
            &pruned_state(),
            &effect,
            &BTreeMap::new(),
            &BTreeMap::new(),
        ) {
            Ok(_) => panic!("a transition must carry its own committed body"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            format!("missing committed metadata for input {INPUT_ID}")
        );
    }

    /// The resident path is unchanged: a body that is still committed keeps producing the input
    /// item with the Turn identity it belongs to.
    #[test]
    fn committed_input_body_still_projects_its_item_with_the_turn_identity() {
        let thread = thread();
        let effect = terminal_effect(&thread.id);
        let projected = project_effect_items(
            &thread,
            &committed_state(),
            &effect,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .expect("a resident input body is projected as before");
        assert!(projected.unresolved_inputs.is_empty());
        let input_item = projected
            .items
            .iter()
            .find(|item| item.id == INPUT_ID)
            .expect("the committed input projects its item");
        assert_eq!(input_item.turn_id, TURN_ID);
        assert_eq!(
            input_item.text().map(pl_protocol::ThreadTextItem::text),
            Some("skill discovery")
        );
        assert_terminal_turn_item(&projected.items);
    }
}
