use sea_orm::{DbErr, RuntimeErr, SqlxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioStartupErrorKind {
    PersistentData,
    Environment,
    Cancelled,
    Internal,
}

/// A startup step failed before the runtime was published. Its source retains the original cause.
#[derive(Debug, thiserror::Error)]
#[error("Studio startup step {step} failed ({kind:?})")]
pub struct StudioStartupError {
    pub step: &'static str,
    pub kind: StudioStartupErrorKind,
    #[source]
    source: anyhow::Error,
}

impl StudioStartupError {
    pub(crate) fn new(
        step: &'static str,
        kind: StudioStartupErrorKind,
        source: impl Into<anyhow::Error>,
    ) -> Self {
        Self {
            step,
            kind,
            source: source.into(),
        }
    }

    pub(crate) fn input(step: &'static str, source: impl Into<anyhow::Error>) -> Self {
        let source = source.into();
        let kind = if is_data_error(&source) {
            StudioStartupErrorKind::PersistentData
        } else if has_environment_error(&source) {
            StudioStartupErrorKind::Environment
        } else {
            StudioStartupErrorKind::Internal
        };
        Self::new(step, kind, source)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid persistent data: {0}")]
struct PersistentDataError(String);

pub(crate) fn data_error(error: impl Into<anyhow::Error>) -> anyhow::Error {
    let error = error.into();
    let message = error.to_string();
    error.context(PersistentDataError(message))
}

#[derive(Debug, thiserror::Error)]
#[error("startup resource cleanup failed")]
struct CleanupFailure;

pub(crate) fn cleanup_error(error: impl Into<anyhow::Error>) -> anyhow::Error {
    error.into().context(CleanupFailure)
}

/// Mark errors originating in a decoder/validator, while preserving access failures.
pub(crate) fn input_error(error: impl Into<anyhow::Error>) -> anyhow::Error {
    let error = error.into();
    if has_environment_error(&error) {
        error
    } else {
        data_error(error)
    }
}

fn is_data_error(error: &anyhow::Error) -> bool {
    if has_environment_error(error) {
        return false;
    }
    error.is::<PersistentDataError>()
        || error.chain().any(|cause| {
            cause.is::<crate::StudioDatabaseError>()
                || cause.is::<toml::de::Error>()
                || cause.is::<serde_json::Error>()
                || cause.is::<std::str::Utf8Error>()
                || cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::InvalidData)
                || cause.downcast_ref::<DbErr>().is_some_and(sqlite_data_error)
        })
}

fn has_environment_error(error: &anyhow::Error) -> bool {
    error.is::<CleanupFailure>() || error.chain().any(|cause| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() != std::io::ErrorKind::InvalidData)
            || cause.downcast_ref::<crate::PureError>().is_some_and(|error| matches!(error, crate::PureError::Io(io) if io.kind() != std::io::ErrorKind::InvalidData))
            || cause.downcast_ref::<DbErr>().is_some_and(|error| !sqlite_data_error(error))
    })
}

fn sqlite_data_error(error: &DbErr) -> bool {
    match error {
        DbErr::Conn(RuntimeErr::SqlxError(error))
        | DbErr::Exec(RuntimeErr::SqlxError(error))
        | DbErr::Query(RuntimeErr::SqlxError(error)) => match error.as_ref() {
            SqlxError::Database(error) => error
                .code()
                .as_deref()
                .and_then(|code| code.parse::<i32>().ok())
                .is_some_and(|code| matches!(code & 0xff, 11 | 26)),
            SqlxError::ColumnDecode { .. } | SqlxError::Decode(_) => true,
            _ => false,
        },
        DbErr::Type(_) | DbErr::Json(_) => true,
        _ => false,
    }
}
