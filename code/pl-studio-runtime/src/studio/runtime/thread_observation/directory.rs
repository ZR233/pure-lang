//! Product ancestry and state projections; notifications never replay old child execution.
use super::ObservationServices;
use anyhow::Result;
use pl_core::thread::{ThreadLifecycle, ThreadSnapshot, TurnState};
use pl_protocol::{AgentState, Thread};

use crate::studio::records::DirectoryState;
use crate::studio::store::directory::ThreadStateUpdate;
use crate::studio::thread_projection::engine::{ProjectionDelta, ProjectionState};

pub(super) async fn publish(
    projector: &ObservationServices,
    thread: Thread,
    snapshot: &ThreadSnapshot,
    state: &ProjectionState,
    delta: &ProjectionDelta,
) -> Result<()> {
    let agent_state = agent_state(snapshot)?;
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
            let text = state
                .materialize()
            .into_iter()
            .filter(|item| item.turn_id == turn.turn_id)
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
    // The runtime usage/panel summary owns the newest product update time; a commit that did not
    // advance it keeps the committed timestamp already folded into `thread.updated_at`.
    let panel_updated_at = if delta.panel_changed {
        state.read_panel()?.updated_at
    } else {
        0
    };
    let updated_at = thread.updated_at.max(panel_updated_at);
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
        state: agent_state,
        updated_at,
    };
    projector.events.update_agent_directory(entry).await;
    // Durable, rebuildable directory summary: only `status`/`error` and the update time, so a cold
    // directory read (no journal replay) sees the observed state. A late observation never
    // re-inserts an archived entry: `patch_thread_runtime` already returned `None` for it.
    projector.events.record_directory_state(ThreadStateUpdate {
        thread_id: thread.id.clone(),
        state: DirectoryState {
            kind: thread.status,
            error: None,
        },
        updated_at,
    });
    Ok(())
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
