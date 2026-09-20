//! Shared test fixtures: canonical core journals and Studio index seeding.
//!
//! This is a `#[cfg(test)]` support module, not a test target: the production modules keep their
//! own `#[cfg(test)] mod tests` and share only these builders.

use crate::studio::thread_projection::engine::ProjectionState;
use crate::studio::timeline_store::{TimelineReader, TimelineWriter};
use pl_core::context::{
    ContextContent, ContextRecord, ContextSnapshot, ContextSource, OpaquePayload,
};
use pl_core::model::{ModelStepOutput, ModelUsage};
use pl_core::persistence::{SqliteSessionOptions, SqliteSessionStore};
use pl_core::thread::cold::ColdStore;
use pl_core::thread::inbox::{InboxRecord, ThreadMessage};
use pl_core::thread::input::{InputChange, InputDelivery, InputRecord, InputState, ThreadInput};
use pl_core::thread::journal::{AttemptUpdate, ThreadCommit};
use pl_core::thread::{
    AttemptOutcome, ContextReplacement, ContextReplacementReason, TurnRecord, TurnState,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn commit(sequence: u64) -> ThreadCommit {
    ThreadCommit {
        committed_at: sequence as i64,
        thread_id: "thread".into(),
        sequence,
        permissions: Vec::new().into(),
        wake_messages_through: None,
        inputs: Vec::new().into(),
        tasks: Vec::new().into(),
        context: None,
        private_context: None,
        attempt: None,
        turn: None,
        discovered_tools: None,
        deliveries: Vec::new().into(),
        extensions: Vec::new().into(),
        inbox: Vec::new().into(),
        consumed_messages: None,
        interactions: Vec::new().into(),
        replacements: Vec::new().into(),
        runtime_facts: None,
        lifecycle: None,
    }
}

fn prompt(text: &str) -> OpaquePayload {
    OpaquePayload::new(
        "pl.studio.prompt",
        1,
        serde_json::json!({ "text": text, "presentation": "visible", "attachments": [] })
            .to_string(),
    )
    .unwrap()
}

pub(super) fn accepted(id: &str, ordinal: u64, text: &str) -> InputChange {
    InputChange::Accepted(InputRecord {
        accepted_sequence: 1,
        delivery: InputDelivery::NextTurn,
        ordinal,
        revision: 1,
        state: InputState::Pending,
        input: ThreadInput {
            id: id.into(),
            payload: prompt(text),
            context: Vec::new(),
        },
    })
}

pub(super) fn turn(input_id: Option<&str>) -> TurnRecord {
    turn_named("t1", input_id)
    }

/// One Turn admission with an explicit turn id, for multi-Turn fixtures.
pub(super) fn turn_named(turn_id: &str, input_id: Option<&str>) -> TurnRecord {
    TurnRecord {
        elapsed_ms: None,
        input_id: input_id.map(Into::into),
        turn_id: turn_id.into(),
        state: TurnState::Running,
        model_steps: 0,
}
}

/// One parent-source inbox message, so the projection keeps durable message facts for it.
pub(super) fn inbox(id: &str, sequence: u64, source: &str) -> InboxRecord {
    InboxRecord {
        sequence,
        message: ThreadMessage {
            id: id.into(),
            source_id: source.into(),
            payload: OpaquePayload::text(id),
            context: Vec::new(),
        },
    }
}

pub(super) fn running_attempt(attempt_id: &str) -> AttemptUpdate {
    AttemptUpdate {
        request_metadata: None,
        tool_projection: None,
        turn_id: "t1".into(),
        attempt_id: attempt_id.into(),
        retry_of: None,
        input_revision: 0,
        tools: Vec::new().into(),
        outcome: AttemptOutcome::Running,
        input_estimate: None,
    }
}

pub(super) fn committed(attempt_id: &str, text: &str) -> AttemptUpdate {
    AttemptUpdate {
        request_metadata: None,
        tool_projection: None,
        turn_id: "t1".into(),
        attempt_id: attempt_id.into(),
        retry_of: None,
        input_revision: 0,
        tools: Vec::new().into(),
        outcome: AttemptOutcome::Committed(ModelStepOutput {
            attempt_id: attempt_id.into(),
            base_context_revision: 0,
            content: vec![ContextContent::Text { text: text.into() }],
            tool_calls: Vec::new(),
            private_context: None,
            usage: ModelUsage::default(),
        }),
        input_estimate: None,
    }
}

/// A rewind replacement that drops `removed_turn` from the live context.
pub(super) fn rewind(removed_turn: &str) -> ContextReplacement {
    ContextReplacement {
        reason: ContextReplacementReason::Rewind,
        previous: ContextSnapshot {
            revision: 1,
            records: Arc::from(vec![ContextRecord {
                id: format!("record-{removed_turn}"),
                turn_id: Some(removed_turn.into()),
                source: ContextSource::Assistant,
                content: Vec::new(),
                tool_calls: Vec::new(),
            }]),
        },
        current: ContextSnapshot {
            revision: 2,
            records: Arc::from(Vec::new()),
        },
        previous_private_context: None,
    }
}

pub(super) fn session_path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("thread.sqlite")
}

/// Writes a genuine core session database with the given canonical commits.
pub(super) async fn seed(path: &Path, thread_id: &str, commits: &[ThreadCommit]) {
    let owned: Vec<Arc<ThreadCommit>> = commits.iter().cloned().map(Arc::new).collect();
    seed_arc(path, thread_id, &owned).await;
}

/// Writes a genuine core session database from shared commits (the journal-reader shape).
pub(super) async fn seed_arc(path: &Path, thread_id: &str, commits: &[Arc<ThreadCommit>]) {
    let store = SqliteSessionStore::open(SqliteSessionOptions {
        path: path.to_path_buf(),
    })
    .await
    .expect("core session store");
    for commit in commits {
        ColdStore::admit(&store, thread_id, commit.sequence, commit.encode().unwrap()).unwrap();
    }
    if let Some(last) = commits.last() {
        ColdStore::flush(&store, thread_id, last.sequence)
            .await
            .unwrap();
    }
    store.shutdown().await.unwrap();
}

/// Applies every commit through the projector and persists each delta through the write adapter.
pub(super) async fn project_and_persist(
    path: &Path,
    thread_id: &str,
    parent: Option<&str>,
    commits: &[ThreadCommit],
) -> ProjectionState {
    let writer = TimelineWriter::open(path).await.unwrap();
    let mut state = ProjectionState::new(thread_id, parent);
    for commit in commits {
        let delta = state.apply(commit).unwrap();
        writer.persist_commit(commit, &state, &delta).await.unwrap();
    }
    writer.close().await.unwrap();
    state
}

/// Seeds the core journal and builds the Studio index in one step, returning the live projection.
pub(super) async fn indexed(
    path: &Path,
    thread_id: &str,
    commits: &[ThreadCommit],
) -> ProjectionState {
    seed(path, thread_id, commits).await;
    project_and_persist(path, thread_id, None, commits).await
}

/// Persists one further commit against an already-indexed session through the bounded resume path.
pub(super) async fn resume_and_persist(
    path: &Path,
    thread_id: &str,
    parent: Option<&str>,
    commit: &ThreadCommit,
) -> ProjectionState {
    let reader = TimelineReader::open(path).await.unwrap();
    let source = reader.restore_source(thread_id, parent).await.unwrap();
    reader.close().await.unwrap();
    let mut state = ProjectionState::restore(source.head, source.facts, source.panel, source.slots);
    let writer = TimelineWriter::open(path).await.unwrap();
    let delta = state.apply(commit).unwrap();
    writer.persist_commit(commit, &state, &delta).await.unwrap();
    writer.close().await.unwrap();
    state
}

pub(super) fn preview_bytes(entries: &[crate::studio::timeline_store::TimelineEntry]) -> usize {
    entries.iter().map(|entry| entry.preview.byte_len()).sum()
}

pub(super) fn answer_texts(
    entries: &[crate::studio::timeline_store::TimelineEntry],
) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| entry.preview.decode_item())
        .filter_map(|item| item.text().map(|text| text.text().to_owned()))
        .collect()
}
