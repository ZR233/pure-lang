use std::collections::BTreeSet;
use std::future::Future;

use crate::session_runtime::{
    SessionAgentChange, SessionAgentEvent, SessionBuildContext, SessionEventSource,
    SessionEventSubscription, SessionMessageSender, SessionSourceError, SessionWakeEvent,
};

use super::super::{AgentRuntimeHandle, AgentTargetSelector, ThreadId};
use super::support::filter_visible;

pub(super) struct AgentEventSource {
    pub(super) runtime: AgentRuntimeHandle,
    pub(super) caller: ThreadId,
    pub(super) selector: AgentTargetSelector,
}

impl SessionEventSource for AgentEventSource {
    fn initialize(
        &self,
        context: SessionBuildContext,
    ) -> impl Future<Output = Result<SessionEventSubscription, SessionSourceError>> + Send {
        let runtime = self.runtime.clone();
        let caller = self.caller.clone();
        let selector = self.selector.clone();
        async move {
            let mut updates = runtime.subscribe_directory();
            let history = runtime
                .directory
                .retain_history(caller.clone(), selector.clone())
                .map_err(|error| SessionSourceError::new("retain agent history", error))?;
            let messages = context
                .messages()
                .map_err(|error| SessionSourceError::new("bind agent publisher", error))?;
            let cancellation = context.cancellation_token();
            Ok(SessionEventSubscription::new(async move {
                let mut seen = BTreeSet::new();
                let mut closed = BTreeSet::new();
                loop {
                    let scan = async {
                        let directory = runtime.directory_snapshot();
                        let visible = filter_visible(&directory.agents, &caller, &selector);
                        for snapshot in visible
                            .into_iter()
                            .filter(|snapshot| snapshot.identity.id != caller)
                        {
                            let id = &snapshot.identity.id;
                            if matches!(snapshot.state, pl_protocol::AgentState::Closed(_))
                                && closed.contains(id)
                            {
                                continue;
                            }
                            if !matches!(snapshot.state, pl_protocol::AgentState::Closed(_)) {
                                closed.remove(id);
                            }
                            let turns =
                                runtime.thread_events.turns(id.as_str()).map_err(|error| {
                                    SessionSourceError::new("read agent turns", error)
                                })?;
                            for turn in turns
                                .iter()
                                .filter_map(crate::session_runtime::SessionAgentTurn::from_turn)
                            {
                                publish(
                                    &messages,
                                    &mut seen,
                                    &turn.turn_id.clone(),
                                    SessionAgentEvent {
                                        identity: snapshot.identity.clone(),
                                        change: SessionAgentChange::TurnCompleted(turn),
                                    },
                                )
                                .await?;
                            }
                            let state =
                                runtime
                                    .read_thread_context(id.clone())
                                    .await
                                    .map_err(|error| {
                                        SessionSourceError::new("read agent reports", error)
                                    })?;
                            for (index, report) in state.submissions.iter().enumerate() {
                                publish(
                                    &messages,
                                    &mut seen,
                                    &format!("report:{index}"),
                                    SessionAgentEvent {
                                        identity: snapshot.identity.clone(),
                                        change: SessionAgentChange::Progress(
                                            pl_protocol::AgentProgressCheckpoint {
                                                report: report.report.clone(),
                                                updated_at: report.created_at,
                                            },
                                        ),
                                    },
                                )
                                .await?;
                            }
                            let thread = runtime.thread_snapshot(id).map_err(|error| {
                                SessionSourceError::new("read agent interactions", error)
                            })?;
                            for interaction in
                                thread.interactions.into_iter().filter(|interaction| {
                                    interaction.status() == pl_protocol::InteractionStatus::Pending
                                })
                            {
                                publish(
                                    &messages,
                                    &mut seen,
                                    &interaction.interaction_id.clone(),
                                    SessionAgentEvent {
                                        identity: snapshot.identity.clone(),
                                        change: SessionAgentChange::InteractionPending(
                                            crate::session_runtime::SessionAgentInteraction {
                                                interaction_id: interaction.interaction_id.clone(),
                                                scope: interaction.scope.clone(),
                                                kind: interaction.kind(),
                                            },
                                        ),
                                    },
                                )
                                .await?;
                            }
                            let terminal = match &snapshot.state {
                                pl_protocol::AgentState::Closed(_) => {
                                    Some(SessionAgentChange::Closed)
                                }
                                pl_protocol::AgentState::Faulted(_) => {
                                    Some(SessionAgentChange::Faulted)
                                }
                                pl_protocol::AgentState::Closing(state) => state
                                    .error()
                                    .cloned()
                                    .map(SessionAgentChange::CleanupFailed),
                                pl_protocol::AgentState::Idle(_)
                                | pl_protocol::AgentState::Queued(_)
                                | pl_protocol::AgentState::Running(_)
                                | pl_protocol::AgentState::WaitingTool(_)
                                | pl_protocol::AgentState::WaitingInteraction(_)
                                | pl_protocol::AgentState::Cancelling(_) => None,
                            };
                            if let Some(change) = terminal {
                                publish(
                                    &messages,
                                    &mut seen,
                                    &format!("state:{}", snapshot.event_sequence),
                                    SessionAgentEvent {
                                        identity: snapshot.identity.clone(),
                                        change,
                                    },
                                )
                                .await?;
                            }
                            if matches!(snapshot.state, pl_protocol::AgentState::Closed(_)) {
                                closed.insert(id.clone());
                                history.acknowledge_closed(id);
                            }
                        }
                        Ok::<(), SessionSourceError>(())
                    };
                    tokio::select! {
                        result = scan => result?,
                        _ = cancellation.cancelled() => return Ok(()),
                    }
                    tokio::select! {
                        result = updates.changed() => { result.map_err(|error| SessionSourceError::new("wait for agent changes", error))?; },
                        _ = cancellation.cancelled() => return Ok(()),
                    }
                }
            }))
        }
    }
}

async fn publish(
    sender: &SessionMessageSender,
    seen: &mut BTreeSet<String>,
    key: &str,
    event: SessionAgentEvent,
) -> Result<(), SessionSourceError> {
    let value = serde_json::to_value(&event)
        .map_err(|error| SessionSourceError::new("encode agent event", error))?;
    let id = crate::canonical_json_hash(&serde_json::json!([key, value]));
    if seen.contains(&id) {
        return Ok(());
    }
    sender
        .publish_event_wait(id.clone(), SessionWakeEvent::AgentChanged(event))
        .await
        .map_err(|error| SessionSourceError::new("publish agent event", error))?;
    seen.insert(id);
    Ok(())
}
