//! Product ancestry and state projections; notifications never replay old child execution.
use super::ObservationServices;
use anyhow::Result;
use pl_core::thread::{ThreadLifecycle, ThreadSnapshot, TurnState};
use pl_protocol::{AgentState, Thread};

pub(super) async fn publish(
    projector: &ObservationServices,
    thread: Thread,
    snapshot: &ThreadSnapshot,
) -> Result<()> {
    let state = agent_state(snapshot)?;
    let Some(thread) =
        projector
            .events
            .patch_thread_runtime(&thread.id, thread.status, thread.updated_at)
    else {
        return Ok(());
    };
    let summary = match snapshot
        .turns
        .last()
        .filter(|turn| turn.state != TurnState::Running)
    {
        Some(turn) => {
            let text = projector
                .store
                .history(&thread.id)
                .await?
                .items_for_turn(&turn.turn_id)
                .await?
                .into_iter()
                .filter_map(|item| {
                    item.text()
                        .filter(|text| text.channel() == pl_protocol::ThreadTextChannel::Final)
                        .map(|text| text.text().to_owned())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.is_empty()).then_some(text)
        }
        None => None,
    };
    let entry = crate::StudioAgentDirectoryEntry {
        id: thread.id.clone(),
        thread_id: thread.id.clone(),
        root_thread_id: thread.root_thread_id.clone(),
        path: thread.agent_path.clone(),
        parent_path: thread.parent_thread_id.clone(),
        role: thread.role.clone(),
        task: thread.title.clone(),
        summary,
        depth: u32::from(thread.parent_thread_id.is_some()),
        state,
        updated_at: thread.updated_at,
    };
    projector.events.update_agent_directory(entry).await;
    Ok(())
}

/// Repairs a Thread whose terminal Turn left the effect window before product observation saw it.
///
/// A root Thread has no parent to wake, but its product directory and terminal-derived state must
/// still reflect the durable terminal Turn rather than a pruned or stale snapshot. The terminal
/// record and the final text both come from bounded durable history, so the repair never depends on
/// a resident snapshot that may already have dropped the Turn.
pub(super) async fn publish_recovered_terminal(
    projector: &ObservationServices,
    mut thread: Thread,
    snapshot: &ThreadSnapshot,
    turn: &pl_core::thread::TurnRecord,
    updated_at: i64,
) -> Result<()> {
    let mut repaired = snapshot.clone();
    let mut turns = repaired
        .turns
        .iter()
        .filter(|record| record.turn_id != turn.turn_id)
        .cloned()
        .collect::<Vec<_>>();
    turns.push(turn.clone());
    repaired.turns = turns.into();
    thread.status = crate::studio::thread_projection::status(&repaired);
    thread.updated_at = thread.updated_at.max(updated_at);
    publish(projector, thread, &repaired).await
}

fn agent_state(snapshot: &ThreadSnapshot) -> Result<AgentState> {
    match snapshot.lifecycle {
        ThreadLifecycle::Closed => {
            return Ok(AgentState::Closed(pl_protocol::ClosedAgentState::new()));
        }
        ThreadLifecycle::Closing => {
            return Ok(AgentState::Closing(pl_protocol::ClosingAgentState::new()));
        }
        ThreadLifecycle::Open => {}
    }
    if let Some(interaction) = snapshot.interactions.values().find(|interaction| {
        interaction.state == pl_core::thread::interactions::InteractionState::Pending
    }) {
        return Ok(AgentState::WaitingInteraction(
            pl_protocol::WaitingInteractionAgentState::new(
                pl_protocol::TurnId::new(interaction.request.turn_id.clone())?,
                interaction.request.id.clone(),
            ),
        ));
    }
    if let Some(permission) = snapshot.permissions.values().find(|permission| {
        permission.state == pl_core::thread::permissions::PermissionState::Pending
    }) {
        return Ok(AgentState::WaitingInteraction(
            pl_protocol::WaitingInteractionAgentState::new(
                pl_protocol::TurnId::new(permission.turn_id.clone())?,
                permission.id.clone(),
            ),
        ));
    }
    if let Some(turn) = snapshot
        .turns
        .iter()
        .rev()
        .find(|turn| turn.state == TurnState::Running)
    {
        return Ok(AgentState::Running(pl_protocol::RunningAgentState::new(
            pl_protocol::TurnId::new(turn.turn_id.clone())?,
        )));
    }
    // Accepted inputs have no Turn identity until execution starts; do not fabricate a queued Turn.
    Ok(AgentState::idle())
}
