use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{
    InteractionChangedEvent, StudioAgentSnapshot, StudioEventEnvelope, StudioEventKind,
    StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart, StudioTurn,
    StudioTurnStatus,
};
use pl_trace::{AgentEvent, TracePart, TracePartDeltaEvent};
use tokio::sync::{Mutex, broadcast};

use crate::studio::ids::{new_studio_event_id, unix_seconds};
use crate::studio::timeline_actor::{TimelineDeltaDecision, TracePartScope, TurnTimelineActor};
use crate::studio::{StudioEventFilter, StudioFilteredEventReceiver, StudioStore};

mod mapper;
use mapper::{
    assistant_message_status_for_turn, message_id_for_trace_part, message_role_for_trace_part,
    studio_agent_timeline_event, studio_part_delta, studio_part_from_trace_part,
    studio_session_summary, trace_delta_matches_part, trace_part_delta_is_user_text,
    trace_part_is_user_text,
};

#[derive(Clone)]
pub struct StudioEventRuntime {
    store: StudioStore,
    tx: broadcast::Sender<StudioEventEnvelope>,
    timeline_actor: Arc<Mutex<TurnTimelineActor>>,
}

impl StudioEventRuntime {
    pub fn new(store: StudioStore) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            store,
            tx,
            timeline_actor: Arc::new(Mutex::new(TurnTimelineActor::default())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StudioEventEnvelope> {
        self.tx.subscribe()
    }

    pub fn subscribe_filtered(&self, filter: StudioEventFilter) -> StudioFilteredEventReceiver {
        StudioFilteredEventReceiver::new(self.tx.subscribe(), filter)
    }

    pub fn subscribe_session(&self, session_id: impl Into<String>) -> StudioFilteredEventReceiver {
        self.subscribe_filtered(StudioEventFilter::session(session_id))
    }

    pub fn subscribe_global(&self) -> StudioFilteredEventReceiver {
        self.subscribe_filtered(StudioEventFilter::global())
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

    pub async fn emit_live(
        &self,
        project_id: Option<String>,
        session_id: Option<String>,
        turn_id: Option<String>,
        kind: StudioEventKind,
    ) -> Result<StudioEventEnvelope> {
        if !matches!(
            kind,
            StudioEventKind::MessagePartDelta { .. } | StudioEventKind::Stale { .. }
        ) {
            anyhow::bail!("emit_live only accepts live-only studio events");
        }
        let sequence = if let Some(session_id) = session_id.as_deref() {
            self.store.next_studio_event_sequence(session_id).await? as u64
        } else {
            0
        };
        let envelope = StudioEventEnvelope {
            event_id: new_studio_event_id(),
            project_id,
            session_id,
            turn_id,
            sequence,
            created_at: unix_seconds(),
            kind,
        };
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
        if let Some(message_status) = assistant_message_status_for_turn(status) {
            self.finish_assistant_message(session_id, turn_id, message_status, reason.clone())
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

    pub async fn emit_session_list(&self, project_id: &str) -> Result<StudioEventEnvelope> {
        let sessions = self
            .store
            .list_sessions(project_id)
            .await?
            .into_iter()
            .map(studio_session_summary)
            .collect();
        self.emit(
            Some(project_id.to_string()),
            None,
            None,
            StudioEventKind::SessionListChanged {
                project_id: project_id.to_string(),
                sessions,
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
            AgentEvent::TracePartStarted { item } => {
                if trace_part_is_user_text(&item) {
                    return Ok(None);
                }
                return self
                    .emit_trace_part_snapshot(session_id, item)
                    .await
                    .map(Some);
            }
            AgentEvent::TracePartDelta { event } => {
                if trace_part_delta_is_user_text(&event) {
                    return Ok(None);
                }
                return self.emit_trace_part_delta(session_id, event).await;
            }
            AgentEvent::TracePartCompleted { item } => {
                if trace_part_is_user_text(&item) {
                    return Ok(None);
                }
                return self
                    .emit_trace_part_snapshot(session_id, item)
                    .await
                    .map(Some);
            }
            AgentEvent::TracePartFailed { item, error } => {
                if trace_part_is_user_text(&item) {
                    return Ok(None);
                }
                let mut item = item;
                if item.content.trim().is_empty() {
                    item.content = error;
                }
                return self
                    .emit_trace_part_snapshot(session_id, item)
                    .await
                    .map(Some);
            }
            AgentEvent::InteractionChanged { event } => {
                return self.emit_interaction(session_id, event).await.map(Some);
            }
            AgentEvent::AgentRuntimeUpdated { .. } => return Ok(None),
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
            AgentEvent::AgentStateChanged {
                id,
                path,
                parent_path,
                role,
                task,
                status,
                summary,
                depth,
                error,
                reason,
                budget_limit_kind,
                budget_usage,
                updated_at,
            } => StudioEventKind::AgentChanged {
                agent: StudioAgentSnapshot {
                    id,
                    session_id: session_id.to_string(),
                    path,
                    parent_path,
                    role,
                    task,
                    status,
                    summary,
                    depth,
                    error,
                    reason,
                    budget_limit_kind,
                    budget_usage,
                    runtime_usage: None,
                    updated_at,
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
                event: studio_agent_timeline_event(session_id, event),
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
            .emit_live(
                None,
                Some(session_id.to_string()),
                None,
                StudioEventKind::Stale { lagged_events },
            )
            .await?;
        Ok(())
    }

    async fn emit_trace_part_snapshot(
        &self,
        session_id: &str,
        item: TracePart,
    ) -> Result<StudioEventEnvelope> {
        self.ensure_assistant_message_started(session_id, &item)
            .await?;
        let trace_part_id = item.item_id.clone();
        let trace_scope = TracePartScope::new(session_id, &item.turn_id, &trace_part_id);
        let mut part = studio_part_from_trace_part(session_id, item);
        let existing_part_id = self.resolve_trace_part_id(&trace_scope).await;
        let existing = self
            .store
            .read_message_part(existing_part_id.as_deref().unwrap_or(&part.part_id))
            .await?;
        let next_order = self.store.next_message_part_order(&part.message_id).await?;
        self.prepare_trace_part_snapshot(
            &mut part,
            Some(&trace_scope),
            existing.as_ref(),
            next_order,
        )
        .await;
        let envelope_part = part.clone();
        let envelope = self
            .emit(
                None,
                Some(session_id.to_string()),
                Some(part.turn_id.clone()),
                StudioEventKind::MessagePartUpdated {
                    part: Box::new(envelope_part),
                },
            )
            .await?;
        self.record_trace_part_snapshot(&part).await;
        Ok(envelope)
    }

    async fn emit_trace_part_delta(
        &self,
        session_id: &str,
        event: TracePartDeltaEvent,
    ) -> Result<Option<StudioEventEnvelope>> {
        let Some((part_id, revision)) = self.accept_trace_part_delta(session_id, &event).await?
        else {
            return Ok(None);
        };
        let turn_id = event.turn_id.clone();
        let delta = studio_part_delta(event, part_id, revision);
        self.emit_live(
            None,
            Some(session_id.to_string()),
            Some(turn_id),
            StudioEventKind::MessagePartDelta { delta },
        )
        .await
        .map(Some)
    }

    async fn accept_trace_part_delta(
        &self,
        session_id: &str,
        event: &TracePartDeltaEvent,
    ) -> Result<Option<(String, u64)>> {
        let trace_scope = TracePartScope::new(session_id, &event.turn_id, &event.item_id);
        let Some(part_id) = self.resolve_trace_part_id(&trace_scope).await else {
            self.emit_stale(session_id, 1).await?;
            return Ok(None);
        };
        let existing = self.store.read_message_part(&part_id).await?;
        let Some(existing) = existing.as_ref() else {
            self.emit_stale(session_id, 1).await?;
            return Ok(None);
        };
        if !trace_delta_matches_part(session_id, event, existing) {
            self.emit_stale(session_id, 1).await?;
            return Ok(None);
        }
        let decision = self.timeline_actor.lock().await.prepare_delta(
            &part_id,
            event.revision,
            Some(existing),
        );
        match decision {
            TimelineDeltaDecision::Accept { revision } => Ok(Some((part_id, revision))),
            TimelineDeltaDecision::Stale => {
                self.emit_stale(session_id, 1).await?;
                Ok(None)
            }
        }
    }

    async fn prepare_trace_part_snapshot(
        &self,
        part: &mut StudioPart,
        trace_scope: Option<&TracePartScope>,
        existing: Option<&crate::studio::records::StudioPartRecord>,
        next_order: u64,
    ) {
        let mut actor = self.timeline_actor.lock().await;
        actor.prepare_snapshot_order(part, trace_scope, existing, next_order);
        actor.prepare_activity_group(part, existing);
        actor.prepare_snapshot(part);
    }

    async fn record_trace_part_snapshot(&self, part: &StudioPart) {
        self.timeline_actor.lock().await.record_snapshot(part);
    }

    async fn resolve_trace_part_id(&self, trace_scope: &TracePartScope) -> Option<String> {
        self.timeline_actor
            .lock()
            .await
            .resolve_trace_part_id(trace_scope, None)
    }

    async fn ensure_assistant_message_started(
        &self,
        session_id: &str,
        item: &TracePart,
    ) -> Result<()> {
        if message_role_for_trace_part(item) != StudioMessageRole::Assistant {
            return Ok(());
        }
        let message_id = message_id_for_trace_part(item);
        if self.store.read_studio_message(&message_id).await?.is_some() {
            return Ok(());
        }
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(item.turn_id.clone()),
            StudioEventKind::MessageUpdated {
                message: Box::new(StudioMessage {
                    message_id,
                    session_id: session_id.to_string(),
                    turn_id: item.turn_id.clone(),
                    role: StudioMessageRole::Assistant,
                    status: StudioMessageStatus::Streaming,
                    created_at: item.created_at,
                    updated_at: item.created_at,
                    completed_at: None,
                    error: None,
                    metadata: serde_json::json!({}),
                }),
            },
        )
        .await?;
        Ok(())
    }

    async fn finish_assistant_message(
        &self,
        session_id: &str,
        turn_id: &str,
        status: StudioMessageStatus,
        error: Option<String>,
    ) -> Result<()> {
        let message_id = format!("{turn_id}:assistant");
        let Some(current) = self.store.read_studio_message(&message_id).await? else {
            return Ok(());
        };
        let now = unix_seconds();
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(turn_id.to_string()),
            StudioEventKind::MessageUpdated {
                message: Box::new(StudioMessage {
                    status,
                    updated_at: now,
                    completed_at: Some(now),
                    error,
                    ..current.message
                }),
            },
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
