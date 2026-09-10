//! Product ancestry and state projections; notifications never replay old child execution.
use super::ObservationServices;
use anyhow::Result;
use pl_core::thread::{ThreadLifecycle, ThreadSnapshot, TurnState, journal::ThreadCommit};
use pl_protocol::{AgentState, Thread};
use std::sync::Arc;

pub(super) async fn publish(
    projector: &ObservationServices,
    thread: Thread,
    snapshot: &ThreadSnapshot,
) -> Result<()> {
    let state = agent_state(snapshot)?;
    let progress = snapshot
        .extensions
        .get(crate::thread_assembler::progress::EXTENSION)
        .map(|record| crate::thread_assembler::progress::decode(&record.payload))
        .transpose()?
        .map(|record| pl_protocol::AgentProgressCheckpoint {
            report: record.report,
            updated_at: record.created_at,
        });
    let summary = progress
        .as_ref()
        .map(|progress| progress.report.summary.clone());
    let summary_age_seconds = progress.as_ref().map_or(0, |progress| {
        u64::try_from(
            crate::studio::ids::unix_seconds()
                .saturating_sub(progress.updated_at)
                .max(0),
        )
        .unwrap_or(0)
    });
    let Some(thread) =
        projector
            .events
            .patch_thread_runtime(&thread.id, thread.status, thread.updated_at)
    else {
        return Ok(());
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
        progress,
        updated_at: thread.updated_at,
        summary_age_seconds,
    };
    projector.events.update_agent_directory(entry).await;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ChildNotification<'a> {
    child_id: &'a str,
    commit_sequence: u64,
    turn: Option<&'a pl_core::thread::TurnRecord>,
    lifecycle: Option<ThreadLifecycle>,
    progress: Option<pl_protocol::AgentSubmissionRecord>,
}
pub(super) async fn notify_parent(
    projector: &ObservationServices,
    thread: &Thread,
    commit: &ThreadCommit,
) -> Result<()> {
    let Some(parent) = thread.parent_thread_id.as_deref() else {
        return Ok(());
    };
    let finished = commit
        .turn
        .as_ref()
        .filter(|turn| turn.state != TurnState::Running);
    let lifecycle = commit
        .lifecycle
        .filter(|state| *state == ThreadLifecycle::Closed);
    let progress = commit
        .extensions
        .iter()
        .find_map(|change| match change {
            pl_core::thread::extensions::ExtensionChange::Put { id, record }
                if id == crate::thread_assembler::progress::EXTENSION =>
            {
                Some(crate::thread_assembler::progress::decode(&record.payload))
            }
            pl_core::thread::extensions::ExtensionChange::Put { .. }
            | pl_core::thread::extensions::ExtensionChange::Delete { .. } => None,
        })
        .transpose()?;
    if finished.is_none() && lifecycle.is_none() && progress.is_none() {
        return Ok(());
    }
    let notification = ChildNotification {
        child_id: &thread.id,
        commit_sequence: commit.sequence,
        turn: finished,
        lifecycle,
        progress,
    };
    let content = serde_json::to_string(&notification)?;
    projector
        .threads
        .notify_parent(
            parent,
            pl_core::thread::inbox::ThreadMessage {
                id: format!(
                    "studio.child.{}:{}:{}",
                    thread.id.len(),
                    thread.id,
                    commit.sequence
                ),
                source_id: format!("studio.child:{}", thread.id),
                payload: pl_core::context::OpaquePayload::new(
                    "pl.studio.child-notification",
                    1,
                    content.clone(),
                )?,
                context: vec![pl_core::context::ContextContent::Text {
                    text: Arc::from(content),
                }],
            },
        )
        .await?;
    Ok(())
}
