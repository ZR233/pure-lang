//! 把一个 write-behind 批次应用进单个 SQLite 事务，并对持久化错误分类。

use sea_orm::{EntityTrait, TransactionTrait};
use std::collections::BTreeMap;

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::runtime::MODEL_PERFORMANCE_OWNER_ID;
use crate::studio::store::directory::apply_directory_delta;
use crate::studio::store::object::put_object;

use super::super::{ApplyCommitOutcome, apply_state_commit, store_error};
use super::queue::{
    PendingBatch, QueueEntry, QueuedMutation, StudioDirectoryMutation, StudioMutation,
};
use super::worker::{BatchError, PersistenceDisposition};

pub(super) async fn apply_batch(
    store: &StudioStore,
    batch: &PendingBatch,
    confirmed: &mut super::super::context::TranscriptCache,
) -> Result<(), BatchError> {
    let mut cache = confirmed.clone();
    let tx = store.database().begin().await.map_err(classify_db_error)?;
    let applied = applied_thread_batches(&tx, batch, &mut cache).await?;
    for entry in &batch.entries {
        match entry {
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Thread(commit),
                ..
            }) => match if applied
                .get(&commit.agent_id)
                .is_some_and(|revision| commit.facts.revision <= *revision)
            {
                Ok(ApplyCommitOutcome::AlreadyApplied)
            } else {
                apply_thread_fact(&tx, commit, &mut cache).await
            } {
                Ok(ApplyCommitOutcome::Applied | ApplyCommitOutcome::AlreadyApplied) => {}
                Ok(ApplyCommitOutcome::RevisionConflict { actual_revision }) => {
                    let _ = tx.rollback().await;
                    return Err(BatchError::Conflict { actual_revision });
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(classify_store_error(error));
                }
            },
            QueueEntry::Mutation(QueuedMutation {
                mutation: StudioMutation::Directory(directory),
                ..
            }) => match directory.as_ref() {
                StudioDirectoryMutation::Delta(delta) => {
                    if let Err(error) = apply_directory_delta(&tx, delta).await {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
                StudioDirectoryMutation::Attachments(records) => {
                    if let Err(error) =
                        crate::studio::store::attachment::persist_attachment_records(&tx, records)
                            .await
                    {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
                StudioDirectoryMutation::WorktreeLease(lease) => {
                    if let Err(error) =
                        put_object(&tx, &lease.child_id, lease, crate::studio::unix_seconds()).await
                    {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
                StudioDirectoryMutation::ModelPerformance(commit) => {
                    if let Err(error) = put_object(
                        &tx,
                        MODEL_PERFORMANCE_OWNER_ID,
                        &commit.value,
                        commit.value.updated_at(),
                    )
                    .await
                    {
                        let _ = tx.rollback().await;
                        return Err(classify_store_error(store_error(error)));
                    }
                }
            },
            QueueEntry::Barrier(_) => {}
        }
    }
    tx.commit().await.map_err(classify_db_error)?;
    *confirmed = cache;
    Ok(())
}

// 一个事务可能已提交，而 worker 尚未推进内存水位就退出。
// 恢复后的批次可能还包含新事实；只跳过由相同 receipt 确认的已保存前缀。
async fn applied_thread_batches(
    tx: &sea_orm::DatabaseTransaction,
    batch: &PendingBatch,
    cache: &mut super::super::context::TranscriptCache,
) -> Result<BTreeMap<pl_core::ThreadId, u64>, BatchError> {
    let mut owners = BTreeMap::<_, Vec<_>>::new();
    for entry in &batch.entries {
        if let QueueEntry::Mutation(QueuedMutation {
            mutation: StudioMutation::Thread(fact),
            ..
        }) = entry
        {
            owners.entry(fact.agent_id.clone()).or_default().push(fact);
        }
    }
    let mut applied = BTreeMap::new();
    for (id, facts) in owners {
        let revision = crate::studio::entity::thread::Entity::find_by_id(id.to_string())
            .one(tx)
            .await
            .map_err(classify_db_error)?
            .and_then(|row| row.runtime_revision)
            .and_then(|revision| u64::try_from(revision).ok());
        let Some(fact) = facts
            .into_iter()
            .find(|fact| Some(fact.facts.revision) == revision)
        else {
            continue;
        };
        match apply_thread_fact(tx, fact, cache)
            .await
            .map_err(classify_store_error)?
        {
            ApplyCommitOutcome::AlreadyApplied => {
                applied.insert(id, fact.facts.revision);
            }
            ApplyCommitOutcome::RevisionConflict { actual_revision } => {
                return Err(BatchError::Conflict { actual_revision });
            }
            ApplyCommitOutcome::Applied => {}
        }
    }
    Ok(applied)
}

async fn apply_thread_fact(
    tx: &sea_orm::DatabaseTransaction,
    fact: &super::thread_fact::ThreadFact,
    cache: &mut super::super::context::TranscriptCache,
) -> Result<ApplyCommitOutcome, PureError> {
    let commit = fact.materialize(tx, cache).await?;
    apply_state_commit(tx, &commit, cache).await
}

fn classify_db_error(error: sea_orm::DbErr) -> BatchError {
    let disposition = db_error_disposition(&error);
    classified_store_error(disposition, store_error(error))
}

fn classify_store_error(error: PureError) -> BatchError {
    let message = error.to_string().to_ascii_lowercase();
    let disposition = if contains_retryable_sqlite_error(&message) {
        PersistenceDisposition::Retryable
    } else {
        PersistenceDisposition::Blocked
    };
    classified_store_error(disposition, error)
}

fn classified_store_error(disposition: PersistenceDisposition, error: PureError) -> BatchError {
    match disposition {
        PersistenceDisposition::Retryable => BatchError::RetryableStore(error),
        PersistenceDisposition::Blocked => BatchError::BlockedStore(error),
    }
}

fn db_error_disposition(error: &sea_orm::DbErr) -> PersistenceDisposition {
    use sea_orm::{ConnAcquireErr, DbErr, RuntimeErr, SqlxError};

    match error {
        DbErr::ConnectionAcquire(ConnAcquireErr::Timeout) => PersistenceDisposition::Retryable,
        DbErr::Conn(RuntimeErr::SqlxError(error))
        | DbErr::Exec(RuntimeErr::SqlxError(error))
        | DbErr::Query(RuntimeErr::SqlxError(error)) => match error.as_ref() {
            SqlxError::Database(error) => error
                .code()
                .as_deref()
                .and_then(|code| code.parse::<i32>().ok())
                .map_or(PersistenceDisposition::Blocked, sqlite_code_disposition),
            SqlxError::Io(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                ) =>
            {
                PersistenceDisposition::Retryable
            }
            SqlxError::PoolTimedOut => PersistenceDisposition::Retryable,
            _ => PersistenceDisposition::Blocked,
        },
        DbErr::Conn(RuntimeErr::Internal(message))
        | DbErr::Exec(RuntimeErr::Internal(message))
        | DbErr::Query(RuntimeErr::Internal(message))
            if contains_retryable_sqlite_error(&message.to_ascii_lowercase()) =>
        {
            PersistenceDisposition::Retryable
        }
        _ => PersistenceDisposition::Blocked,
    }
}

fn sqlite_code_disposition(extended_code: i32) -> PersistenceDisposition {
    match extended_code & 0xff {
        // SQLITE_BUSY、SQLITE_LOCKED 与 SQLITE_IOERR 允许自动重试。
        5 | 6 | 10 => PersistenceDisposition::Retryable,
        // 损坏、只读、容量耗尽、结构/约束错误等均需要人工处置。
        _ => PersistenceDisposition::Blocked,
    }
}

fn contains_retryable_sqlite_error(message: &str) -> bool {
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database is busy")
        || message.contains("disk i/o error")
        || message.contains("sqlite_busy")
        || message.contains("sqlite_locked")
        || message.contains("sqlite_ioerr")
}
