//! Authoritative current snapshots and independently persisted history projections.
use super::{ProjectionError, project_active_turn};
use pl_core::thread::{
    ThreadLifecycle, ThreadSnapshot, TurnState,
    cold::{StorageExecutionPhase, StorageFaultKind},
    input::InputState,
};
use pl_protocol::{
    Thread, ThreadPersistenceSnapshot, ThreadStatus, ThreadStorageExecution, ThreadStorageState,
    studio::HistoryFault,
};

/// 权威快照：canonical Thread 状态 + typed 活动 + typed 存储状态。
///
/// 活动是 owner 已发布状态的**纯内存投影**（见 [`super::project_activity`]），不需要共享 Session；
/// `persistence` 是该 Thread 的 typed 持久化快照，缺失表示没有可报告的存储事实，因此 `storage`
/// 保持 `None`，不填占位值。快照是冷读/权威首帧：没有上一条活动，活动版本从 0 开始。
pub(in crate::studio) fn project_snapshot(
    mut thread: Thread,
    state: &ThreadSnapshot,
    usage: &pl_core::thread::UsageSummary,
    persistence: Option<&ThreadPersistenceSnapshot>,
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
    let activity = super::project_activity(&thread.id, state, None);
    let storage = persistence.map(|persistence| storage_state(state, persistence));
    Ok(pl_protocol::ThreadSnapshot {
        schema_version: pl_protocol::THREAD_SCHEMA_VERSION,
        revision: state.commit_sequence,
        runtime,
        thread,
        active_turn,
        interactions,
        activity,
        storage,
    })
}

/// Thread 存储状态：runtime 持久化协调器的 typed 故障/水位 + core 的存储执行阶段。
///
/// 故障类别与水位来自 typed 值（不是错误文本的解析结果）；core 的原始错误文本只作为诊断文本
/// 原样透出，绝不参与类别推断。
pub(in crate::studio) fn storage_state(
    state: &ThreadSnapshot,
    persistence: &ThreadPersistenceSnapshot,
) -> ThreadStorageState {
    ThreadStorageState {
        // The coordinator's typed fault is authoritative; when it has none the core storage latch's
        // typed category is used instead, so a plain SQLite Thread still reports a typed fault. Both
        // are values, not error-text parses.
        fault: persistence
            .fault
            .or_else(|| state.persistence.fault.map(storage_fault)),
        fault_generation: persistence.fault_generation,
        // The coordinator's watermarks are authoritative while it reports them. A plain core/SQLite
        // Thread has no product writer to report them, but an attached store still makes the owner's
        // own typed watermarks the same facts rather than unknown — a cold checkpoint without an
        // attached store keeps `None`, which is "unknown" and not a claimed zero.
        accepted_sequence: persistence.history_admitted_sequence.or_else(|| {
            state
                .persistence
                .attached
                .then_some(state.persistence.admitted_sequence)
        }),
        durable_sequence: persistence.history_durable_sequence.or_else(|| {
            state
                .persistence
                .attached
                .then_some(state.persistence.durable_sequence)
        }),
        execution: match state.persistence.execution_phase {
            StorageExecutionPhase::Running => ThreadStorageExecution::Running,
            StorageExecutionPhase::PausingForStorage => ThreadStorageExecution::Pausing,
            StorageExecutionPhase::PausedForStorage => ThreadStorageExecution::Paused,
        },
        pressure_paused: state.persistence.pressure_paused,
        // The explicit-continue latch is a core fact about the safety point, not a guess from the
        // fault category: a retried save clears the fault but must not resume admission on its own.
        resume_required: state.persistence.resume_required,
        // Readiness has exactly one owner: the Thread's storage owner recomputes it from its own
        // typed facts — the current fault generation, the durability fence it fixed for that
        // generation, the handed-over watermark, the byte threshold and — when the backend named the
        // generation — that backend's own verdict, which reaches the owner as the typed
        // `StoragePressure::recovered_generation` input rather than as a second published flag. No
        // other readiness exists to OR in here: a writer's own view of its retry can never light the
        // continue entry while a newer core fault still holds the Thread.
        can_resume: state.persistence.resume_ready,
        last_error: state.persistence.error.clone(),
    }
}

/// Core's typed storage fault as the protocol's typed fault category.
fn storage_fault(fault: StorageFaultKind) -> HistoryFault {
    match fault {
        StorageFaultKind::QueueFull => HistoryFault::QueueFull,
        StorageFaultKind::WriteFailed => HistoryFault::WriteFailed,
        StorageFaultKind::WriterUnavailable => HistoryFault::WriterUnavailable,
        StorageFaultKind::NoProgress => HistoryFault::NoProgress,
        StorageFaultKind::CheckpointFailed => HistoryFault::CheckpointFailed,
        StorageFaultKind::BlobFailed => HistoryFault::BlobFailed,
    }
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
