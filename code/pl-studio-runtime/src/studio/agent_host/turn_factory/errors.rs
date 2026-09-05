//! turn 准备阶段的错误包装。

use crate::PureError;

pub(super) fn turn_error(error: impl Into<String>) -> PureError {
    PureError::MemoryError(error.into())
}

pub(super) fn anyhow_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}
