//! Parent dialogue is projected from durable inbox admission and consumption, never model context.
use super::{ProjectionError, order};
use pl_core::thread::journal::ThreadCommit;
use pl_protocol::{ThreadItem, ThreadItemState, ThreadRawItem};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn project_messages(
    thread_id: &str,
    parent_id: Option<&str>,
    journal: &[Arc<ThreadCommit>],
    through: u64,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let Some(parent_id) = parent_id else {
        return Ok(Vec::new());
    };
    let source_id = format!("agent:{parent_id}");
    let mut messages = BTreeMap::<u64, ThreadItem>::new();
    for commit in journal.iter().filter(|commit| commit.sequence <= through) {
        for record in commit
            .inbox
            .iter()
            .filter(|record| record.message.source_id == source_id)
        {
            let payload = &record.message.payload;
            let state = if payload.format() == "text/plain" && payload.version() == 1 {
                ThreadItem::completed_parent_agent_message(
                    order::message_id(&record.message.id),
                    thread_id.into(),
                    String::new(),
                    payload.content().into(),
                    Vec::new(),
                    commit.committed_at,
                )
                .state()
                .clone()
            } else {
                ThreadItemState::Raw(ThreadRawItem {
                    payloads: vec![super::raw_payload(payload)],
                    notice: "Unsupported saved parent message encoding".into(),
                    recorded_at: commit.committed_at,
                })
            };
            messages.insert(
                record.sequence,
                ThreadItem::new(
                    order::message_id(&record.message.id),
                    thread_id.into(),
                    String::new(),
                    0,
                    commit.sequence,
                    commit.committed_at,
                    commit.committed_at,
                    state,
                ),
            );
        }
        if let Some(through) = commit.consumed_messages {
            for (_, item) in messages
                .range_mut(..=through)
                .filter(|(_, item)| item.turn_id.is_empty())
            {
                let attempt = commit
                    .attempt
                    .as_ref()
                    .ok_or_else(|| ProjectionError::MissingMessage(item.id.clone()))?;
                item.turn_id = attempt.turn_id.clone();
                item.revision = commit.sequence;
                item.updated_at = commit.committed_at;
            }
        }
    }
    Ok(messages.into_values().collect())
}
