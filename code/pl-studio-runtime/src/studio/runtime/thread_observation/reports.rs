//! One immutable projection per terminal Turn; no separate delivery state or mutable report store.
use super::ObservationServices;
use anyhow::Result;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{ThreadSnapshot, TurnRecord, TurnState, journal::ThreadCommit},
};
use pl_protocol::{Thread, ThreadTextChannel};
use std::sync::Arc;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnReport<'a> {
    child_id: &'a str,
    commit_sequence: u64,
    turn: &'a TurnRecord,
    message: String,
    messages: Vec<MessageIdentity<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_tool: Option<pl_protocol::ThreadToolItem>,
    running_tasks: Vec<&'a str>,
    pending_interactions: Vec<&'a str>,
    permissions: Vec<&'a pl_core::thread::permissions::PermissionRecord>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageIdentity<'a> {
    id: &'a str,
    source_id: &'a str,
    sequence: u64,
    consumed: bool,
}

pub(super) async fn publish(
    services: &ObservationServices,
    thread: &Thread,
    history: &[Arc<ThreadCommit>],
    wake: bool,
) -> Result<()> {
    let Some(parent) = thread.parent_thread_id.as_deref() else {
        return Ok(());
    };
    let Some(commit) = history.last() else {
        return Ok(());
    };
    let Some(turn) = commit
        .turn
        .as_ref()
        .filter(|turn| turn.state != TurnState::Running)
    else {
        return Ok(());
    };
    if history[..history.len() - 1].iter().any(|commit| {
        commit.turn.as_ref().is_some_and(|previous| {
            previous.turn_id == turn.turn_id && previous.state != TurnState::Running
        })
    }) {
        return Ok(());
    }
    let snapshot = pl_core::thread::journal::replay(history)?;
    let report = report(thread, turn, &snapshot, history)?;
    let content = serde_json::to_string(&report)?;
    services
        .threads
        .notify_parent(
            parent,
            pl_core::thread::inbox::ThreadMessage {
                // Preserve the journal-derived identity used by historical terminal notifications.
                id: format!(
                    "studio.child.{}:{}:{}",
                    thread.id.len(),
                    thread.id,
                    commit.sequence
                ),
                source_id: format!("studio.child:{}", thread.id),
                payload: OpaquePayload::new("pl.studio.turn-report", 1, content.clone())?,
                context: vec![ContextContent::Text {
                    text: content.into(),
                }],
            },
            wake,
        )
        .await?;
    Ok(())
}

fn report<'a>(
    thread: &'a Thread,
    turn: &'a TurnRecord,
    snapshot: &'a ThreadSnapshot,
    history: &[Arc<ThreadCommit>],
) -> Result<TurnReport<'a>> {
    let items = crate::studio::thread_projection::project_items(
        &thread.id,
        thread.parent_thread_id.as_deref(),
        snapshot,
        history,
    )?;
    let mut finals = Vec::new();
    let mut commentary = Vec::new();
    for item in &items {
        if item.turn_id != turn.turn_id {
            continue;
        }
        if let Some(text) = item.text() {
            match text.channel() {
                ThreadTextChannel::Final => finals.push(text.text()),
                ThreadTextChannel::Commentary => commentary.push(text.text()),
                _ => {}
            }
        }
    }
    let mut message = finals.join("\n\n");
    let last_tool = if message.is_empty() {
        items
            .iter()
            .rev()
            .filter(|item| item.turn_id == turn.turn_id)
            .find_map(|item| item.tool().cloned())
    } else {
        None
    };
    if message.is_empty() {
        message = format!(
            "本轮已停止，未提交最终总结。实际状态：{:?}。\n\n本轮已有可见输出：\n{}",
            turn.state,
            commentary.join("\n\n")
        );
    }
    // Capture the opening inbox watermark before this Turn, even if model preparation failed
    // before consuming its triggering messages. Include later messages only when actually consumed.
    let opening = history
        .iter()
        .position(|commit| {
            commit
                .turn
                .as_ref()
                .is_some_and(|record| record.turn_id == turn.turn_id)
        })
        .unwrap_or(0);
    let before = &history[..opening];
    let consumed_before = before
        .iter()
        .filter_map(|commit| commit.consumed_messages)
        .next_back()
        .unwrap_or(0);
    let wake_before = before
        .iter()
        .filter_map(|commit| commit.wake_messages_through)
        .next_back()
        .unwrap_or(0);
    let through = wake_before.max(snapshot.consumed_messages);
    let messages = snapshot
        .inbox
        .iter()
        .filter(|record| record.sequence > consumed_before && record.sequence <= through)
        .map(|record| MessageIdentity {
            id: &record.message.id,
            source_id: &record.message.source_id,
            sequence: record.sequence,
            consumed: record.sequence <= snapshot.consumed_messages,
        })
        .collect();
    Ok(TurnReport {
        child_id: &thread.id,
        commit_sequence: snapshot.commit_sequence,
        turn,
        message,
        messages,
        last_tool,
        running_tasks: snapshot
            .tasks
            .values()
            .filter(|task| task.status == pl_core::thread::task::TaskStatus::Running)
            .map(|task| task.id.as_str())
            .collect(),
        pending_interactions: snapshot
            .interactions
            .values()
            .filter(|item| item.state == pl_core::thread::interactions::InteractionState::Pending)
            .map(|item| item.request.id.as_str())
            .collect(),
        permissions: snapshot
            .permissions
            .values()
            .filter(|item| {
                item.turn_id == turn.turn_id
                    || item.state == pl_core::thread::permissions::PermissionState::Pending
            })
            .collect(),
    })
}
