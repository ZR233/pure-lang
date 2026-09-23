//! Authoritative current snapshots and independently persisted history projections.
use super::{ProjectionError, project_active_turn, project_items};
use pl_core::thread::{
    ThreadEffectBatch, ThreadLifecycle, ThreadSnapshot, TurnState, input::InputState,
};
use pl_protocol::{Thread, ThreadStatus};
use std::sync::Arc;

pub(in crate::studio) fn project_snapshot(
    mut thread: Thread,
    state: &ThreadSnapshot,
    usage: &pl_core::thread::UsageSummary,
) -> Result<pl_protocol::ThreadSnapshot, ProjectionError> {
    let active_turn = project_active_turn(&thread.id, state, thread.updated_at)?;
    let mut interactions = Vec::new();
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        if let Ok(Some(interaction)) =
            crate::thread_assembler::project_thread_interaction(&thread.id, id, state)
        {
            interactions.push(interaction);
        }
    }
    thread.status = status(state);
    if let Ok(Some(mode)) = saved_mode(state) {
        thread.mode = mode;
    }
    let runtime = super::runtime::project_runtime(&thread.id, state, thread.updated_at, usage).ok();
    Ok(pl_protocol::ThreadSnapshot {
        schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
        revision: state.commit_sequence,
        runtime,
        thread,
        active_turn,
        interactions,
    })
}

/// Independently persisted history items for one Thread.
///
/// This is the one-way legacy-history projection: `state` must be a durable session state (the
/// migration export replays the old journal into one) and `journal` the committed effect window
/// that state was captured with. Live product reads take their items from the durable history
/// database, so this projection is not part of the normal observation path.
pub(in crate::studio) fn project_history_items(
    thread: &Thread,
    state: &ThreadSnapshot,
    journal: &[Arc<ThreadEffectBatch>],
) -> Result<Vec<pl_protocol::ThreadItem>, ProjectionError> {
    let mut items = project_items(
        &thread.id,
        thread.parent_thread_id.as_deref(),
        state,
        journal,
    )?;
    let recorded_at = journal.last().map_or(0, |commit| commit.committed_at);
    for id in state.permissions.keys().chain(state.interactions.keys()) {
        if let Err(error) =
            crate::thread_assembler::project_thread_interaction(&thread.id, id, state)
        {
            let payloads = state
                .permissions
                .get(id)
                .map(|record| &record.payload)
                .or_else(|| {
                    state
                        .interactions
                        .get(id)
                        .map(|record| &record.request.payload)
                })
                .into_iter()
                .map(super::raw_payload)
                .collect();
            append_raw_notice(
                &mut items,
                thread,
                RawNotice {
                    id: format!("raw-interaction:{id}"),
                    payloads,
                    notice: error.to_string(),
                    revision: state.commit_sequence,
                    recorded_at,
                },
            );
        }
    }
    if let Err(error) = saved_mode(state) {
        append_raw_notice(
            &mut items,
            thread,
            RawNotice {
                id: "raw-mode".into(),
                payloads: state
                    .extensions
                    .get("studio.mode")
                    .map(|record| super::raw_payload(&record.payload))
                    .into_iter()
                    .collect(),
                notice: error.to_string(),
                revision: state.commit_sequence,
                recorded_at,
            },
        );
    }
    // 历史投影只把 runtime 投影当作可解码性检查；累计使用量由 writer 折叠进 checkpoint 摘要，
    // 这里读同一份持久载体，不重新聚合历史集合。
    if let Err(error) =
        super::runtime::project_runtime(&thread.id, state, recorded_at, &state.usage_summary)
    {
        let mut payloads = state
            .extensions
            .values()
            .map(|record| super::raw_payload(&record.payload))
            .collect::<Vec<_>>();
        for attempt in state.attempts.iter() {
            if let Some(payload) = &attempt.request_metadata {
                payloads.push(super::raw_payload(payload));
            }
        }
        append_raw_notice(
            &mut items,
            thread,
            RawNotice {
                id: format!("raw-runtime:{}", state.commit_sequence),
                payloads,
                notice: error.to_string(),
                revision: state.commit_sequence,
                recorded_at: journal
                    .last()
                    .map_or(thread.updated_at, |commit| commit.committed_at),
            },
        );
    }
    Ok(items)
}

struct RawNotice {
    id: String,
    payloads: Vec<pl_protocol::ThreadRawPayload>,
    notice: String,
    revision: u64,
    recorded_at: i64,
}
fn append_raw_notice(items: &mut Vec<pl_protocol::ThreadItem>, thread: &Thread, record: RawNotice) {
    items.push(pl_protocol::ThreadItem::new(
        record.id,
        thread.id.clone(),
        String::new(),
        items
            .last()
            .map_or(1, |item| item.ordinal.saturating_add(1)),
        record.revision,
        record.recorded_at,
        record.recorded_at,
        pl_protocol::ThreadItemState::Raw(pl_protocol::ThreadRawItem {
            payloads: record.payloads,
            notice: record.notice,
            recorded_at: record.recorded_at,
        }),
    ));
}

pub(in crate::studio) fn status(state: &ThreadSnapshot) -> ThreadStatus {
    match state.lifecycle {
        ThreadLifecycle::Closing => ThreadStatus::Closing,
        ThreadLifecycle::Closed => ThreadStatus::Closed,
        ThreadLifecycle::Open => {
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Interrupting { .. }
            ) {
                return ThreadStatus::Cancelling;
            }
            if matches!(
                state.input_execution,
                pl_core::thread::input::InputExecution::Failed { .. }
            ) {
                return ThreadStatus::Faulted;
            }
            if state.interactions.values().any(|record| {
                record.state == pl_core::thread::interactions::InteractionState::Pending
            }) || state.permissions.values().any(|record| {
                record.state == pl_core::thread::permissions::PermissionState::Pending
            }) {
                ThreadStatus::WaitingInteraction
            } else if state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            {
                ThreadStatus::Running
            } else if state
                .inputs
                .iter()
                .any(|input| input.state == InputState::Pending)
            {
                ThreadStatus::Queued
            } else {
                ThreadStatus::Idle
            }
        }
    }
}

pub(in crate::studio) fn saved_mode(
    state: &ThreadSnapshot,
) -> Result<Option<pl_protocol::ThreadModeId>, ProjectionError> {
    state
        .extensions
        .get("studio.mode")
        .map(|record| {
            if record.payload.format() != "pl.studio.mode" || record.payload.version() != 1 {
                return Err(ProjectionError::UnsupportedOutput(
                    "unsupported saved Mode".into(),
                ));
            }
            Ok(serde_json::from_str(record.payload.content())?)
        })
        .transpose()
}
