//! Committed context reductions are distinct from auxiliary inference accounting.
use super::ProjectionError;

pub(super) fn receipt(
    payload: &pl_core::context::OpaquePayload,
) -> Result<Option<crate::compaction::CompactionReceipt>, ProjectionError> {
    if payload.format() != "pl.studio.compaction" {
        return Ok(None);
    }
    if payload.version() != 1 {
        return Err(ProjectionError::UnsupportedOutput(
            "unsupported compaction receipt version".into(),
        ));
    }
    Ok(Some(serde_json::from_str(payload.content())?))
}
