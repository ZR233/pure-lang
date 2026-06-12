use anyhow::Result;
use pl_protocol::{
    AgentEvent, AgentStatus, InteractionChangedEvent, StudioAgentSnapshot,
    StudioAgentTimelineEvent, StudioEventEnvelope, StudioEventKind, StudioSessionHandoff,
    StudioSessionRuntime, StudioTimelineChange, StudioTurn, StudioTurnStatus,
};
use tokio::sync::broadcast;

use crate::studio::ids::{new_studio_event_id, unix_seconds};
use crate::studio::{SessionHandoffRecord, StudioStore};

#[derive(Clone)]
pub struct StudioEventRuntime {
    store: StudioStore,
    tx: broadcast::Sender<StudioEventEnvelope>,
}

impl StudioEventRuntime {
    pub fn new(store: StudioStore) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { store, tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StudioEventEnvelope> {
        self.tx.subscribe()
    }

    pub async fn emit(
        &self,
        project_id: Option<String>,
        session_id: Option<String>,
        turn_id: Option<String>,
        kind: StudioEventKind,
    ) -> Result<StudioEventEnvelope> {
        let envelope = StudioEventEnvelope {
            event_id: new_studio_event_id(),
            project_id,
            session_id,
            turn_id,
            sequence: 0,
            created_at: unix_seconds(),
            kind,
        };
        let envelope = self.store.append_studio_event(envelope).await?;
        let _ = self.tx.send(envelope.clone());
        Ok(envelope)
    }

    pub async fn emit_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        status: StudioTurnStatus,
        reason: Option<String>,
    ) -> Result<StudioEventEnvelope> {
        let now = unix_seconds();
        if matches!(status, StudioTurnStatus::Queued) {
            let _ = self
                .store
                .create_turn(session_id, turn_id, StudioTurnStatus::Queued, now)
                .await?;
        } else {
            let _ = self
                .store
                .set_turn_status(turn_id, status, reason.clone(), now)
                .await?;
        }
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(turn_id.to_string()),
            StudioEventKind::TurnChanged {
                turn: StudioTurn {
                    turn_id: turn_id.to_string(),
                    session_id: session_id.to_string(),
                    status,
                    reason,
                    updated_at: now,
                },
            },
        )
        .await
    }

    pub async fn emit_interaction(
        &self,
        session_id: &str,
        event: InteractionChangedEvent,
    ) -> Result<StudioEventEnvelope> {
        let turn_id = Some(event.interaction.scope.turn_id.clone());
        self.emit(
            None,
            Some(session_id.to_string()),
            turn_id,
            StudioEventKind::InteractionChanged {
                event: Box::new(event),
            },
        )
        .await
    }

    pub async fn emit_handoff(
        &self,
        handoff: &SessionHandoffRecord,
    ) -> Result<StudioEventEnvelope> {
        self.emit(
            Some(handoff.project_id.clone()),
            Some(handoff.target_session_id.clone()),
            None,
            StudioEventKind::SessionHandoffChanged {
                handoff: StudioSessionHandoff {
                    origin_session_id: handoff.origin_session_id.clone(),
                    target_session_id: handoff.target_session_id.clone(),
                    kind: handoff.kind.as_str().to_string(),
                    status: handoff.status.as_str().to_string(),
                    plan_id: Some(handoff.plan_id.clone()),
                    updated_at: handoff.updated_at,
                },
            },
        )
        .await
    }

    pub async fn emit_agent_event(
        &self,
        session_id: &str,
        event: AgentEvent,
    ) -> Result<Option<StudioEventEnvelope>> {
        let kind = match event {
            AgentEvent::TimelineItemStarted { item } => {
                let turn_id = item.turn_id.clone();
                return self
                    .emit(
                        None,
                        Some(session_id.to_string()),
                        Some(turn_id),
                        StudioEventKind::TimelineChanged {
                            change: Box::new(StudioTimelineChange::Started { item }),
                        },
                    )
                    .await
                    .map(Some);
            }
            AgentEvent::TimelineItemDelta { event } => {
                let turn_id = event.turn_id.clone();
                return self
                    .emit(
                        None,
                        Some(session_id.to_string()),
                        Some(turn_id),
                        StudioEventKind::TimelineChanged {
                            change: Box::new(StudioTimelineChange::Delta { event }),
                        },
                    )
                    .await
                    .map(Some);
            }
            AgentEvent::TimelineItemCompleted { sequence, item } => {
                let turn_id = item.turn_id.clone();
                return self
                    .emit(
                        None,
                        Some(session_id.to_string()),
                        Some(turn_id),
                        StudioEventKind::TimelineChanged {
                            change: Box::new(StudioTimelineChange::Completed { sequence, item }),
                        },
                    )
                    .await
                    .map(Some);
            }
            AgentEvent::TimelineItemFailed {
                sequence,
                item,
                error,
            } => {
                let turn_id = item.turn_id.clone();
                return self
                    .emit(
                        None,
                        Some(session_id.to_string()),
                        Some(turn_id),
                        StudioEventKind::TimelineChanged {
                            change: Box::new(StudioTimelineChange::Failed {
                                sequence,
                                item,
                                error,
                            }),
                        },
                    )
                    .await
                    .map(Some);
            }
            AgentEvent::InteractionChanged { event } => {
                return self.emit_interaction(session_id, event).await.map(Some);
            }
            AgentEvent::AgentRuntimeUpdated { .. } => StudioEventKind::SessionRuntimeChanged {
                runtime: StudioSessionRuntime {
                    payload: serde_json::to_value(event_payload(&event))?,
                },
            },
            AgentEvent::SkillActivated { activation } => {
                let turn_id = Some(activation.turn_id.clone());
                return self
                    .emit(
                        None,
                        Some(session_id.to_string()),
                        turn_id,
                        StudioEventKind::SkillActivated { activation },
                    )
                    .await
                    .map(Some);
            }
            AgentEvent::AgentStateChanged { .. } => StudioEventKind::AgentChanged {
                agent: StudioAgentSnapshot {
                    payload: serde_json::to_value(event_payload(&event))?,
                },
            },
            AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. } => StudioEventKind::AgentTimelineChanged {
                event: StudioAgentTimelineEvent {
                    payload: serde_json::to_value(event_payload(&event))?,
                },
            },
            AgentEvent::TurnInterrupted { reason } => StudioEventKind::TurnChanged {
                turn: StudioTurn {
                    turn_id: String::new(),
                    session_id: session_id.to_string(),
                    status: StudioTurnStatus::Cancelled,
                    reason: Some(reason),
                    updated_at: unix_seconds(),
                },
            },
            AgentEvent::TurnBudgetLimited { reason, .. } => StudioEventKind::TurnChanged {
                turn: StudioTurn {
                    turn_id: String::new(),
                    session_id: session_id.to_string(),
                    status: StudioTurnStatus::Failed,
                    reason: Some(reason),
                    updated_at: unix_seconds(),
                },
            },
            AgentEvent::Done => return Ok(None),
            AgentEvent::Error { message, .. } => StudioEventKind::TurnChanged {
                turn: StudioTurn {
                    turn_id: String::new(),
                    session_id: session_id.to_string(),
                    status: StudioTurnStatus::Failed,
                    reason: Some(message),
                    updated_at: unix_seconds(),
                },
            },
        };
        self.emit(None, Some(session_id.to_string()), None, kind)
            .await
            .map(Some)
    }

    pub async fn emit_stale(&self, session_id: &str, lagged_events: u64) -> Result<()> {
        let _ = self
            .emit(
                None,
                Some(session_id.to_string()),
                None,
                StudioEventKind::Stale { lagged_events },
            )
            .await?;
        Ok(())
    }
}

fn event_payload(event: &AgentEvent) -> serde_json::Value {
    serde_json::to_value(event).unwrap_or(serde_json::Value::Null)
}

#[allow(dead_code)]
fn _assert_agent_status_is_used(status: AgentStatus) -> AgentStatus {
    status
}
