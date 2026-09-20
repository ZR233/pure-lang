//! Index build/rebuild/verify driver over the Studio timeline index.
//!
//! The driver is called by the write-behind worker and by the migration coordinator, one Thread at a
//! time with that Thread's durable journal already in hand; it never reads `session_entries` itself
//! and never opens an always-on writer. Per-Thread memory stays bounded: a fresh build folds one
//! Thread's journal, and a catch-up applies only the commits missing from an existing index through
//! the projector's bounded working set.
//!
//! Idempotency and resume semantics of [`build_index`] with an existing index:
//! - watermark equal to the journal end: [`IndexBuildOutcome::Current`], no work.
//! - watermark behind the journal end: [`IndexBuildOutcome::Extended`] applying only the new commits
//!   (`apply_plan` + `load_facts`/`load_slots`), never a full rebuild.
//! - watermark ahead of the durable journal: a typed [`TimelineStoreError::IndexAhead`], never a
//!   silent truncation.
//!
//! [`rebuild_index`] is the explicit repair entry for a missing, corrupt or wrong-version index; it
//! clears this Thread's derived rows and rewrites them from the journal. It is never invoked from a
//! read path.

use super::TimelineStoreError;
use super::read::TimelineReader;
use super::write::TimelineWriter;
use crate::studio::thread_projection::engine::{Facts, ProjectionHead, ProjectionState};
use pl_core::thread::ThreadSnapshot;
use pl_core::thread::journal::ThreadCommit;
use std::sync::Arc;

/// What one [`build_index`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexBuildOutcome {
    /// No index existed; the whole journal was written.
    Built { watermark: u64 },
    /// An existing index behind the journal was caught up by exactly `applied` commits.
    Extended { from: u64, to: u64, applied: u64 },
    /// The index already matched the journal end.
    Current { watermark: u64 },
}

/// The measured state of an index that matches its journal, without any repair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexVerification {
    pub watermark: u64,
    pub slots: u64,
}

/// Builds or catches up one Thread's Studio index from its durable journal.
///
/// The caller supplies one Thread's commits in sequence order; the driver never touches other
/// Threads or the core format.
///
/// # Errors
/// Fails when the session database is missing or its index declares an unknown schema version, when
/// the index leads the journal, or on any storage failure.
pub(crate) async fn build_index(
    path: impl AsRef<std::path::Path>,
    thread_id: &str,
    parent_id: Option<&str>,
    commits: &[Arc<ThreadCommit>],
) -> Result<IndexBuildOutcome, TimelineStoreError> {
    let path = path.as_ref();
    validate_journal(thread_id, commits)?;
    let journal_end = journal_end(commits);
    match TimelineReader::open(path).await {
        Ok(reader) => match reader.read_head(thread_id).await {
            Ok(head) => {
                return catch_up(
                    path,
                    reader,
                    thread_id,
                    parent_id,
                    head,
                    commits,
                    journal_end,
                )
                .await;
            }
            Err(TimelineStoreError::ThreadNotIndexed { .. }) => {
                reader.close().await?;
            }
            Err(error) => return Err(error),
        },
        Err(TimelineStoreError::IndexNotInitialized { .. }) => {}
        Err(error) => return Err(error),
    }
    let writer = TimelineWriter::open(path).await?;
    let state = full_state(thread_id, parent_id, commits)?;
    writer.write_state(thread_id, &state).await?;
    let watermark = state.watermark();
    writer.close().await?;
    Ok(IndexBuildOutcome::Built { watermark })
}

/// Explicitly rebuilds one Thread's index from its journal, deleting any existing derived rows.
///
/// Use for a missing, corrupt or wrong-version index; the read path never calls this.
///
/// # Errors
/// Fails when the session database is missing or the durability probe or a write fails; the
/// previous rows survive a failed transaction.
pub(crate) async fn rebuild_index(
    path: impl AsRef<std::path::Path>,
    thread_id: &str,
    parent_id: Option<&str>,
    commits: &[Arc<ThreadCommit>],
) -> Result<IndexBuildOutcome, TimelineStoreError> {
    validate_journal(thread_id, commits)?;
    let writer = TimelineWriter::open(path).await?;
    let state = full_state(thread_id, parent_id, commits)?;
    writer.write_state(thread_id, &state).await?;
    let watermark = state.watermark();
    writer.close().await?;
    Ok(IndexBuildOutcome::Built { watermark })
}

/// Verifies one Thread's index against its durable journal without repairing anything.
///
/// # Errors
/// Fails when the Thread has no index, when the index watermark differs from the journal end, or
/// when the persisted slot ordinals are not strictly increasing.
pub(crate) async fn verify_index(
    path: impl AsRef<std::path::Path>,
    thread_id: &str,
    commits: &[Arc<ThreadCommit>],
) -> Result<IndexVerification, TimelineStoreError> {
    validate_journal(thread_id, commits)?;
    let expected = journal_end(commits);
    let reader = TimelineReader::open(path).await?;
    let head = reader.read_head(thread_id).await?;
    if head.watermark != expected {
        reader.close().await?;
        return Err(TimelineStoreError::IndexMismatch {
            expected,
            found: head.watermark,
        });
    }
    let ordinals = reader.slot_ordinals(thread_id).await?;
    if ordinals.windows(2).any(|pair| pair[0] >= pair[1]) {
        reader.close().await?;
        return Err(TimelineStoreError::Corrupt(format!(
            "Thread {thread_id} slot ordinals are not strictly increasing"
        )));
    }
    reader.close().await?;
    Ok(IndexVerification {
        watermark: head.watermark,
        slots: ordinals.len() as u64,
    })
}

#[allow(clippy::too_many_arguments)]
async fn catch_up(
    path: &std::path::Path,
    reader: TimelineReader,
    thread_id: &str,
    parent_id: Option<&str>,
    head: ProjectionHead,
    commits: &[Arc<ThreadCommit>],
    journal_end: u64,
) -> Result<IndexBuildOutcome, TimelineStoreError> {
    if head.watermark == journal_end {
        reader.close().await?;
        return Ok(IndexBuildOutcome::Current {
            watermark: journal_end,
        });
    }
    if head.watermark > journal_end {
        reader.close().await?;
        return Err(TimelineStoreError::IndexAhead {
            index: head.watermark,
            journal: journal_end,
        });
    }

    let from = head.watermark;
    let panel = reader.read_panel(thread_id).await?;
    let facts = Facts {
        message_source: message_source(parent_id),
        ..Default::default()
    };
    // The live projection starts from the persisted head; the facts and positions of each new
    // commit are then loaded through the projector's bounded working set, never the whole history.
    let mut state = ProjectionState::restore(head, facts, panel, Vec::new());
    let writer = TimelineWriter::open(path).await?;
    let mut applied = 0u64;
    for commit in commits.iter().filter(|commit| commit.sequence > from) {
        // Two-phase plan: the requirements are fact-independent, but `slot_keys` also names the
        // slots of messages this commit consumes, which the projector reads from the loaded facts.
        // The plan is therefore recomputed once the facts are in the working set.
        let requirements = state.apply_plan(commit.as_ref()).requirements;
        let facts = reader.load_requirements(thread_id, &requirements).await?;
        state.load_facts(facts.rows)?;
        let slot_keys = state.apply_plan(commit.as_ref()).slot_keys;
        let slots = reader.read_slots(thread_id, &slot_keys).await?;
        state.load_slots(slots)?;
        let delta = state.apply(commit.as_ref())?;
        writer
            .persist_commit(commit.as_ref(), &state, &delta)
            .await?;
        applied += 1;
    }
    let to = state.watermark();
    reader.close().await?;
    writer.close().await?;
    Ok(IndexBuildOutcome::Extended { from, to, applied })
}

fn full_state(
    thread_id: &str,
    parent_id: Option<&str>,
    commits: &[Arc<ThreadCommit>],
) -> Result<ProjectionState, TimelineStoreError> {
    match commits.last() {
        None => Ok(ProjectionState::new(thread_id, parent_id)),
        Some(last) => {
            let snapshot = ThreadSnapshot {
                commit_sequence: last.sequence,
                ..Default::default()
            };
            Ok(ProjectionState::rebuild(
                thread_id, parent_id, &snapshot, commits,
            )?)
        }
    }
}

fn journal_end(commits: &[Arc<ThreadCommit>]) -> u64 {
    commits.last().map(|commit| commit.sequence).unwrap_or(0)
}

/// Rejects a journal that is not one contiguous, single-owner sequence starting at 1.
///
/// A caller always hands over one Thread's complete durable journal, so a gap is a typed refusal
/// and nothing is written: the derived index must never be built from a partial or mixed journal.
fn validate_journal(
    thread_id: &str,
    commits: &[Arc<ThreadCommit>],
) -> Result<(), TimelineStoreError> {
    let mut expected = 1u64;
    for commit in commits {
        if commit.thread_id != thread_id {
            return Err(TimelineStoreError::InvalidRequest(format!(
                "commit {} belongs to Thread {} not {thread_id}",
                commit.sequence, commit.thread_id
            )));
        }
        if commit.sequence != expected {
            return Err(TimelineStoreError::JournalGap {
                expected,
                found: commit.sequence,
            });
        }
        expected = expected.saturating_add(1);
    }
    Ok(())
}

fn message_source(parent_id: Option<&str>) -> Option<String> {
    parent_id.map(|parent| format!("agent:{parent}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::fixture;
    use crate::studio::timeline_store::{TimelineBudget, TimelinePageQuery, TimelineReader};
    use pl_protocol::ThreadItem;
    use pretty_assertions::assert_eq;

    fn journal_with_parent() -> Vec<Arc<ThreadCommit>> {
        let mut first = fixture::commit(1);
        first.inbox = vec![fixture::inbox("m1", 1, "agent:parent")].into();
        first.turn = Some(fixture::turn(None));
        let mut second = fixture::commit(2);
        second.attempt = Some(fixture::running_attempt("a1"));
        let mut third = fixture::commit(3);
        third.attempt = Some(fixture::committed("a1", "answer"));
        [first, second, third].into_iter().map(Arc::new).collect()
    }

    fn expected_items(commits: &[Arc<ThreadCommit>]) -> Vec<ThreadItem> {
        let snapshot = ThreadSnapshot {
            commit_sequence: commits.last().unwrap().sequence,
            ..Default::default()
        };
        ProjectionState::rebuild("thread", Some("parent"), &snapshot, commits)
            .unwrap()
            .materialize()
    }

    async fn page_items(path: &std::path::Path) -> Vec<ThreadItem> {
        let reader = TimelineReader::open(path).await.unwrap();
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        reader.close().await.unwrap();
        window
            .entries
            .iter()
            .map(|entry| {
                entry
                    .preview
                    .decode_item()
                    .expect("small fixture items decode completely")
            })
            .collect()
    }

    #[tokio::test]
    async fn build_index_writes_a_pageable_index_from_the_journal() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;

        let outcome = build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        assert_eq!(outcome, IndexBuildOutcome::Built { watermark: 3 });

        let reader = TimelineReader::open(&path).await.unwrap();
        assert_eq!(reader.read_head("thread").await.unwrap().watermark, 3);
        reader.close().await.unwrap();
        assert_eq!(page_items(&path).await, expected_items(&commits));
    }

    #[tokio::test]
    async fn build_index_catches_up_only_the_new_commits() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;
        build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();

        // Two more commits that only add items, so the bounded catch-up stays exact.
        let mut fourth = fixture::commit(4);
        fourth.attempt = Some(fixture::running_attempt("a2"));
        let mut fifth = fixture::commit(5);
        fifth.attempt = Some(fixture::committed("a2", "second answer"));
        commits.push(Arc::new(fourth));
        commits.push(Arc::new(fifth));
        fixture::seed_arc(&path, "thread", &commits).await;

        let outcome = build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        assert_eq!(
            outcome,
            IndexBuildOutcome::Extended {
                from: 3,
                to: 5,
                applied: 2
            },
            "catch-up must apply exactly the missing commits"
        );
        assert_eq!(page_items(&path).await, expected_items(&commits));
    }

    #[tokio::test]
    async fn build_index_catches_up_a_consumed_message_without_a_full_rebuild() {
        // `apply_plan` now names a consumed inbox message's slot key, so the bounded catch-up must
        // update that item and still match a full rebuild exactly.
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;
        build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();

        let mut fourth = fixture::commit(4);
        fourth.attempt = Some(fixture::running_attempt("a2"));
        let mut fifth = fixture::commit(5);
        fifth.attempt = Some(fixture::committed("a2", "second answer"));
        fifth.consumed_messages = Some(1);
        commits.push(Arc::new(fourth));
        commits.push(Arc::new(fifth));
        fixture::seed_arc(&path, "thread", &commits).await;

        let outcome = build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        assert_eq!(
            outcome,
            IndexBuildOutcome::Extended {
                from: 3,
                to: 5,
                applied: 2
            },
            "a consumed message must not force a full rebuild"
        );
        assert_eq!(page_items(&path).await, expected_items(&commits));
    }

    #[tokio::test]
    async fn rebuild_index_is_idempotent_and_matches_a_fresh_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;
        let first = rebuild_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        let second = rebuild_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(page_items(&path).await, expected_items(&commits));
    }

    #[tokio::test]
    async fn build_index_is_a_no_op_when_current_and_rejects_an_index_that_leads() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;
        build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();

        let outcome = build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();
        assert_eq!(outcome, IndexBuildOutcome::Current { watermark: 3 });

        let error = build_index(&path, "thread", Some("parent"), &commits[..2])
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                TimelineStoreError::IndexAhead {
                    index: 3,
                    journal: 2
                }
            ),
            "unexpected {error:?}"
        );
    }

    #[tokio::test]
    async fn verify_index_reports_a_watermark_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let commits = journal_with_parent();
        fixture::seed_arc(&path, "thread", &commits).await;
        build_index(&path, "thread", Some("parent"), &commits)
            .await
            .unwrap();

        let report = verify_index(&path, "thread", &commits).await.unwrap();
        assert_eq!(report.watermark, 3);
        assert!(report.slots >= 3, "slots {}", report.slots);

        let error = verify_index(&path, "thread", &commits[..2])
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                TimelineStoreError::IndexMismatch {
                    expected: 2,
                    found: 3
                }
            ),
            "unexpected {error:?}"
        );
    }

    #[tokio::test]
    async fn build_index_rejects_a_non_contiguous_journal_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let journal = journal_with_parent();
        let mut fourth = fixture::commit(4);
        fourth.attempt = Some(fixture::committed("a1", "answer"));
        // A gap: sequences 1, 2, 4 with commit 3 missing.
        let gapped: Vec<Arc<ThreadCommit>> =
            vec![journal[0].clone(), journal[1].clone(), Arc::new(fourth)];
        fixture::seed_arc(&path, "thread", &gapped).await;

        let error = build_index(&path, "thread", Some("parent"), &gapped)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                TimelineStoreError::JournalGap {
                    expected: 3,
                    found: 4
                }
            ),
            "unexpected {error:?}"
        );

        // The refusal must leave the derived index unwritten.
        let error = TimelineReader::open(&path).await.unwrap_err();
        assert!(
            matches!(error, TimelineStoreError::IndexNotInitialized { .. }),
            "a rejected journal must not create an index: {error:?}"
        );
    }
}
