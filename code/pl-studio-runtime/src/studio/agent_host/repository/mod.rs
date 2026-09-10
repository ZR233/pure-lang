//! Studio product directory persistence, separate from Thread journal storage.
mod conversion;
pub(super) mod labels;
mod write_behind;
pub(in crate::studio) use write_behind::ThreadWriteBehindWriter;

pub(super) fn store_error(error: impl std::fmt::Display) -> crate::PureError {
    crate::PureError::MemoryError(error.to_string())
}
