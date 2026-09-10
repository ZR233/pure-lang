//! Product enum labels at the database boundary.
use super::store_error;
use crate::PureError;
use pl_protocol::ThreadModeId;
/// 从 thread 表的 `mode` 列值恢复 [`ThreadModeId`]。
pub(super) fn thread_mode_from_label(label: &str) -> Result<ThreadModeId, PureError> {
    ThreadModeId::from_label(label).map_err(map_label_error)
}

fn map_label_error(error: pl_protocol::UnknownLabelError) -> PureError {
    store_error(error.to_string())
}
