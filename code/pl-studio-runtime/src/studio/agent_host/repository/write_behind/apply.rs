//! Product-only transactions; session facts are persisted independently by pl-core.
use super::super::store_error;
use super::queue::{PendingBatch, QueueEntry, StudioDirectoryMutation, StudioMutation};
use super::worker::{BatchError, PersistenceDisposition};
use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::runtime::MODEL_PERFORMANCE_OWNER_ID;
use crate::studio::store::directory::apply_directory_delta;
use crate::studio::store::object::put_object;
use sea_orm::TransactionTrait;

pub(super) async fn apply_batch(
    store: &StudioStore,
    batch: &PendingBatch,
) -> Result<(), BatchError> {
    let tx = store.database().begin().await.map_err(classify_db_error)?;
    for entry in &batch.entries {
        let QueueEntry::Mutation(commit) = entry else {
            continue;
        };
        let StudioMutation::Directory(directory) = &commit.mutation;
        match directory.as_ref() {
            StudioDirectoryMutation::Delta(delta) => apply_directory_delta(&tx, delta)
                .await
                .map_err(|error| classify_store_error(store_error(error)))?,
            StudioDirectoryMutation::WorktreeLease(lease) => {
                put_object(&tx, &lease.child_id, lease, crate::studio::unix_seconds())
                    .await
                    .map_err(|error| classify_store_error(store_error(error)))?
            }
            StudioDirectoryMutation::ModelPerformance(commit) => put_object(
                &tx,
                MODEL_PERFORMANCE_OWNER_ID,
                &commit.value,
                commit.value.updated_at(),
            )
            .await
            .map_err(|error| classify_store_error(store_error(error)))?,
        }
    }
    tx.commit().await.map_err(classify_db_error)?;
    Ok(())
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
