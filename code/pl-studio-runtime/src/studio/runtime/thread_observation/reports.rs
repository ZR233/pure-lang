//! One immutable projection per terminal Turn; no separate delivery state or mutable report store.
use super::ObservationServices;
use anyhow::Result;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{ThreadSnapshot, TurnRecord},
};
use pl_protocol::{Thread, ThreadTextChannel};

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

pub(super) async fn publish_terminal(
    services: &ObservationServices,
    thread: &Thread,
    snapshot: &ThreadSnapshot,
    turn: &TurnRecord,
    sequence: u64,
    wake: bool,
) -> Result<()> {
    let Some(parent) = thread.parent_thread_id.as_deref() else {
        return Ok(());
    };
    let items = services
        .store
        .history(&thread.id)
        .await?
        .items_for_turn(&turn.turn_id)
        .await?;
    let report = report(thread, turn, snapshot, &items)?;
    let content = serde_json::to_string(&report)?;
    let message_id = format!(
        "studio.child.{}:{}:{}",
        thread.id.len(),
        thread.id,
        sequence
    );
    services
        .threads
        .notify_parent(
            parent,
            pl_core::thread::inbox::ThreadMessage {
                // Preserve the sequence-derived identity used by historical terminal notifications.
                id: message_id,
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
    items: &[pl_protocol::ThreadItem],
) -> Result<TurnReport<'a>> {
    let mut finals = Vec::new();
    let mut commentary = Vec::new();
    for item in items {
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
        items.iter().rev().find_map(|item| item.tool().cloned())
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
    let message_item_ids = items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let through = snapshot
        .wake_messages_through
        .max(snapshot.consumed_messages);
    let messages = snapshot
        .inbox
        .iter()
        .filter(|record| {
            let item_id = crate::studio::thread_projection::order::message_id(&record.message.id);
            record.sequence <= through && message_item_ids.contains(item_id.as_str())
        })
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
