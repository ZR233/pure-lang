use pl_protocol::session_runtime::ToolTaskOutputFact;
use pl_protocol::session_runtime::{ToolTaskResultPage, ToolTaskResultReference};
use serde::{Deserialize, Serialize};

use super::{SessionWaitResult, SessionWakeEvent, ToolTaskSnapshot};

/// Model projection is a copy; the original offer is retained for atomic acknowledgement.
pub(super) fn wait_result(
    result: &SessionWaitResult,
) -> Result<SessionWaitResult, ResultReadError> {
    let mut result = result.clone();
    if let SessionWaitResult::Events(batch) = &mut result {
        for envelope in &mut batch.events {
            match &mut envelope.event {
                SessionWakeEvent::ToolFinished(value) => *value = task(value)?,
                SessionWakeEvent::Message(_)
                | SessionWakeEvent::TimerElapsed(_)
                | SessionWakeEvent::AgentChanged(_)
                | SessionWakeEvent::SourceFailed(_) => {}
            }
        }
    }
    Ok(result)
}

pub(crate) fn task(task: &ToolTaskSnapshot) -> Result<ToolTaskSnapshot, ResultReadError> {
    let mut task = task.clone();
    hide_host_facts(&mut task);
    if task.result_reference.is_some() || task.result.is_none() {
        return Ok(task);
    }
    let encoded = encode_result(&task)?;
    let content_hash = crate::canonical_content_hash(encoded.as_bytes());
    task.result_reference = Some(ToolTaskResultReference {
        encoded_bytes: encoded.len() as u64,
        preview_complete: true,
        cursor: encode_cursor(&task.receipt.task_id, &content_hash, 0)?,
        content_hash,
    });
    if serde_json::to_vec(&task)?.len() > 12 * 1024
        && let Some(result) = &mut task.result
    {
        result.output =
            crate::tool::model_visible_tool_output_with_budget(&result.output, 1024, 4096);
        result.structured_content = None;
        result.attachments.clear();
        result.facts.clear();
        result.skill_activations.clear();
        result.output_file = None;
        if let Some(reference) = &mut task.result_reference {
            reference.preview_complete = false;
        }
    }
    Ok(task)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResultReadError {
    #[error("task has no completed result")]
    NotReady,
    #[error("result cursor is invalid, stale, or belongs to another task")]
    InvalidCursor,
    #[error("cannot encode result readback")]
    Encoding(#[from] serde_json::Error),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResultCursor {
    task_id: String,
    content_hash: String,
    offset: usize,
}

pub(super) fn result_page(
    task: &ToolTaskSnapshot,
    cursor: &str,
) -> Result<ToolTaskResultPage, ResultReadError> {
    if cursor.len() > 4096 {
        return Err(ResultReadError::InvalidCursor);
    }
    let cursor: ResultCursor =
        serde_json::from_str(cursor).map_err(|_| ResultReadError::InvalidCursor)?;
    let mut task = task.clone();
    hide_host_facts(&mut task);
    let encoded = encode_result(&task)?;
    let content_hash = crate::canonical_content_hash(encoded.as_bytes());
    if cursor.task_id != task.receipt.task_id
        || cursor.content_hash != content_hash
        || cursor.offset >= encoded.len()
        || !encoded.is_char_boundary(cursor.offset)
    {
        return Err(ResultReadError::InvalidCursor);
    }
    let mut end = cursor.offset.saturating_add(16 * 1024).min(encoded.len());
    while !encoded.is_char_boundary(end) {
        end -= 1;
    }
    Ok(ToolTaskResultPage {
        task_id: cursor.task_id.clone(),
        content_hash: content_hash.clone(),
        offset: cursor.offset as u64,
        encoded_bytes: encoded.len() as u64,
        text: encoded[cursor.offset..end].to_owned(),
        next_cursor: if end < encoded.len() {
            Some(encode_cursor(&cursor.task_id, &content_hash, end)?)
        } else {
            None
        },
    })
}

fn encode_result(task: &ToolTaskSnapshot) -> Result<String, ResultReadError> {
    let result = task.result.as_ref().ok_or(ResultReadError::NotReady)?;
    Ok(serde_json::to_string(result)?)
}

fn encode_cursor(
    task_id: &str,
    content_hash: &str,
    offset: usize,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ResultCursor {
        task_id: task_id.to_owned(),
        content_hash: content_hash.to_owned(),
        offset,
    })
}

fn hide_host_facts(task: &mut ToolTaskSnapshot) {
    if let Some(result) = &mut task.result {
        result.facts.retain(|fact| match fact {
            ToolTaskOutputFact::AuditMetadata { .. } => false,
            ToolTaskOutputFact::OutputArtifacts { .. }
            | ToolTaskOutputFact::CacheHit { .. }
            | ToolTaskOutputFact::OutputMetrics { .. }
            | ToolTaskOutputFact::OutputBudget { .. }
            | ToolTaskOutputFact::RejectedControl { .. } => true,
        });
    }
}
