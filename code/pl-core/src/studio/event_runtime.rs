use std::sync::Arc;

use anyhow::Result;
use pl_protocol::{
    AgentStatus, InteractionChangedEvent, StudioAgentPart, StudioAgentSnapshot,
    StudioAgentTimelineEvent, StudioAgentTimelineEventKind, StudioAttachment, StudioEventEnvelope,
    StudioEventKind, StudioInferencePart, StudioMessage, StudioMessageRole, StudioMessageStatus,
    StudioPart, StudioPartDelta, StudioPartDeltaField, StudioPartStatus, StudioPartType,
    StudioPlanPart, StudioSessionHandoff, StudioSessionSummary, StudioTextChannel, StudioToolPart,
    StudioTurn, StudioTurnStatus,
};
use pl_trace::{
    AgentEvent, TraceAgentPart, TraceDelta, TraceInferencePart, TracePart, TracePartDeltaEvent,
    TracePartKind, TracePartSource, TracePartStatus, TraceTextChannel, TraceToolPart,
};
use tokio::sync::{Mutex, broadcast};

use crate::studio::ids::{new_studio_event_id, unix_seconds};
use crate::studio::timeline_actor::{
    TimelineDeltaDecision, TurnTimelineActor, is_terminal_studio_part_status,
};
use crate::studio::{
    SessionHandoffRecord, StudioEventFilter, StudioFilteredEventReceiver, StudioStore,
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

    pub async fn emit_handoff(
        &self,
        handoff: &SessionHandoffRecord,
    ) -> Result<StudioEventEnvelope> {
        let target_session = self
            .store
            .read_session(&handoff.target_session_id)
            .await?
            .map(studio_session_summary);
        self.emit(
            Some(handoff.project_id.clone()),
            Some(handoff.target_session_id.clone()),
            None,
            StudioEventKind::SessionHandoffChanged {
                handoff: StudioSessionHandoff {
                    origin_session_id: handoff.origin_session_id.clone(),
                    target_session_id: handoff.target_session_id.clone(),
                    target_session,
                    kind: handoff.kind.as_str().to_string(),
                    status: handoff.status.as_str().to_string(),
                    plan_id: Some(handoff.plan_id.clone()),
                    updated_at: handoff.updated_at,
                },
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
        let mut part = studio_part_from_trace_part(session_id, item);
        let existing = self.store.read_message_part(&part.part_id).await?;
        let next_order = self.store.next_message_part_order(&part.message_id).await?;
        self.prepare_trace_part_snapshot(&mut part, existing.as_ref(), next_order)
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
        if !self.accept_trace_part_delta(session_id, &event).await? {
            return Ok(None);
        }
        let turn_id = event.turn_id.clone();
        let delta = studio_part_delta(session_id, event);
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
    ) -> Result<bool> {
        let existing = self.store.read_message_part(&event.item_id).await?;
        let decision = self
            .timeline_actor
            .lock()
            .await
            .prepare_delta(event, existing.as_ref());
        if decision == TimelineDeltaDecision::Stale {
            self.emit_stale(session_id, 1).await?;
            return Ok(false);
        }
        Ok(true)
    }

    async fn prepare_trace_part_snapshot(
        &self,
        part: &mut StudioPart,
        existing: Option<&crate::studio::records::StudioPartRecord>,
        next_order: u64,
    ) {
        let mut actor = self.timeline_actor.lock().await;
        actor.prepare_snapshot_order(part, existing, next_order);
        actor.prepare_snapshot(part);
    }

    async fn record_trace_part_snapshot(&self, part: &StudioPart) {
        self.timeline_actor.lock().await.record_snapshot(part);
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

fn studio_session_summary(session: crate::studio::SessionRecord) -> StudioSessionSummary {
    StudioSessionSummary {
        id: session.id,
        project_id: session.project_id,
        title: session.title,
        mode: session.mode,
        updated_at: session.updated_at,
        visibility: session.visibility.as_str().to_string(),
        parent_session_id: session.parent_session_id,
    }
}

fn trace_part_is_user_text(item: &TracePart) -> bool {
    item.kind == TracePartKind::Text && item.text_channel == Some(TraceTextChannel::User)
}

fn trace_part_delta_is_user_text(event: &TracePartDeltaEvent) -> bool {
    matches!(
        (&event.kind, &event.delta),
        (
            TracePartKind::Text,
            TraceDelta::Text {
                text_channel: TraceTextChannel::User,
                ..
            }
        )
    )
}

fn studio_part_delta(session_id: &str, event: TracePartDeltaEvent) -> StudioPartDelta {
    let field = match &event.delta {
        TraceDelta::Text { .. } => StudioPartDeltaField::Text,
        TraceDelta::Thinking { .. } => StudioPartDeltaField::ReasoningSummary,
        TraceDelta::ToolArguments { .. } => StudioPartDeltaField::ToolArguments,
        TraceDelta::ToolResult { .. } => StudioPartDeltaField::ToolResult,
        TraceDelta::Plan { .. } => StudioPartDeltaField::PlanContent,
    };
    let chunk_index = match &event.delta {
        TraceDelta::Thinking { chunk_index, .. } => Some(*chunk_index),
        TraceDelta::Text { .. }
        | TraceDelta::ToolArguments { .. }
        | TraceDelta::ToolResult { .. }
        | TraceDelta::Plan { .. } => None,
    };
    StudioPartDelta {
        session_id: session_id.to_string(),
        message_id: message_id_for_trace_delta(&event),
        part_id: event.item_id,
        revision: event.revision,
        field,
        delta: trace_delta_text(event.delta),
        chunk_index,
    }
}

fn trace_delta_text(delta: TraceDelta) -> String {
    match delta {
        TraceDelta::Text { delta, .. }
        | TraceDelta::Thinking { delta, .. }
        | TraceDelta::ToolArguments { delta }
        | TraceDelta::ToolResult { delta }
        | TraceDelta::Plan { delta } => delta,
    }
}

fn studio_agent_timeline_event(session_id: &str, event: AgentEvent) -> StudioAgentTimelineEvent {
    let kind = match event {
        AgentEvent::CollabAgentSpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
            ..
        } => StudioAgentTimelineEventKind::SpawnBegin {
            call_id,
            sender_path,
            task_name,
            prompt,
            role,
            model,
            reasoning_effort,
        },
        AgentEvent::CollabAgentSpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
            ..
        } => StudioAgentTimelineEventKind::SpawnEnd {
            call_id,
            sender_path,
            agent_id,
            path,
            role,
            status,
            prompt,
            error,
        },
        AgentEvent::CollabAgentInteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
            ..
        } => StudioAgentTimelineEventKind::InteractionBegin {
            call_id,
            sender_path,
            receiver_path,
            prompt,
        },
        AgentEvent::CollabAgentInteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
            ..
        } => StudioAgentTimelineEventKind::InteractionEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            prompt,
            error,
        },
        AgentEvent::CollabWaitingBegin {
            call_id,
            sender_path,
            ..
        } => StudioAgentTimelineEventKind::WaitingBegin {
            call_id,
            sender_path,
        },
        AgentEvent::CollabWaitingEnd {
            call_id,
            sender_path,
            timed_out,
            ..
        } => StudioAgentTimelineEventKind::WaitingEnd {
            call_id,
            sender_path,
            timed_out,
        },
        AgentEvent::CollabCloseBegin {
            call_id,
            sender_path,
            receiver_path,
            ..
        } => StudioAgentTimelineEventKind::CloseBegin {
            call_id,
            sender_path,
            receiver_path,
        },
        AgentEvent::CollabCloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
            ..
        } => StudioAgentTimelineEventKind::CloseEnd {
            call_id,
            sender_path,
            receiver_path,
            status,
            error,
        },
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => {
            unreachable!("non agent trace events are filtered before mapping")
        }
    };
    StudioAgentTimelineEvent {
        event_id: String::new(),
        session_id: session_id.to_string(),
        sequence: 0,
        created_at: 0,
        kind,
    }
}

fn studio_part_from_trace_part(session_id: &str, item: TracePart) -> StudioPart {
    let source = item.source;
    let part_type = part_type_for_trace_kind(item.kind);
    let status = part_status_for_trace_status(item.status);
    let completed_at = is_terminal_studio_part_status(status).then_some(item.updated_at);
    let text = part_text(&item);
    let message_id = message_id_for_trace_part(&item);
    let part_id = part_id_for_trace_part(&item);
    let error = error_for_part_status(status, &item.content);
    let tool = item.tool.map(studio_tool_part);
    let agent = item.agent.map(studio_agent_part);
    let inference = item.inference.map(studio_inference_part);
    let plan = matches!(part_type, StudioPartType::Plan).then(|| StudioPlanPart {
        content: item.content.clone(),
    });
    StudioPart {
        part_id,
        message_id,
        session_id: session_id.to_string(),
        turn_id: item.turn_id,
        part_type,
        order: item.started_sequence,
        revision: item.revision,
        status,
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at,
        error,
        text_channel: item.text_channel.map(studio_text_channel),
        text,
        attachments: item
            .attachments
            .into_iter()
            .map(|attachment| StudioAttachment {
                id: attachment.id,
                media_type: attachment.media_type,
                filename: attachment.filename,
                width: attachment.width,
                height: attachment.height,
                byte_size: attachment.byte_size,
                data_url: attachment.data_url,
            })
            .collect(),
        tool,
        agent,
        inference,
        plan,
        file: None,
        usage: item.usage,
        synthetic: matches!(source, TracePartSource::Runtime)
            || matches!(part_type, StudioPartType::Turn | StudioPartType::Inference),
        ignored: false,
    }
}

fn part_id_for_trace_part(item: &TracePart) -> String {
    match (item.kind, item.text_channel) {
        (TracePartKind::Text, Some(TraceTextChannel::User)) => {
            format!("{}:user-text", item.turn_id)
        }
        (TracePartKind::Text, Some(TraceTextChannel::Commentary))
        | (TracePartKind::Text, Some(TraceTextChannel::Final))
        | (TracePartKind::Text, None)
        | (TracePartKind::Thinking, _)
        | (TracePartKind::Tool, _)
        | (TracePartKind::Agent, _)
        | (TracePartKind::Turn, _)
        | (TracePartKind::Inference, _)
        | (TracePartKind::Plan, _) => item.item_id.clone(),
    }
}

fn message_id_for_trace_part(item: &TracePart) -> String {
    let suffix = match message_role_for_trace_part(item) {
        StudioMessageRole::User => "user",
        StudioMessageRole::Assistant => "assistant",
        StudioMessageRole::System => "system",
    };
    format!("{}:{suffix}", item.turn_id)
}

fn message_id_for_trace_delta(event: &TracePartDeltaEvent) -> String {
    let suffix = match event.kind {
        TracePartKind::Text => match &event.delta {
            TraceDelta::Text {
                text_channel: TraceTextChannel::User,
                ..
            } => "user",
            TraceDelta::Text {
                text_channel: TraceTextChannel::Commentary | TraceTextChannel::Final,
                ..
            } => "assistant",
            TraceDelta::Thinking { .. }
            | TraceDelta::ToolArguments { .. }
            | TraceDelta::ToolResult { .. }
            | TraceDelta::Plan { .. } => "assistant",
        },
        TracePartKind::Thinking
        | TracePartKind::Tool
        | TracePartKind::Agent
        | TracePartKind::Turn
        | TracePartKind::Inference
        | TracePartKind::Plan => "assistant",
    };
    format!("{}:{suffix}", event.turn_id)
}

fn message_role_for_trace_part(item: &TracePart) -> StudioMessageRole {
    match item.kind {
        TracePartKind::Text => match item.text_channel {
            Some(TraceTextChannel::User) => StudioMessageRole::User,
            Some(TraceTextChannel::Commentary | TraceTextChannel::Final) | None => {
                StudioMessageRole::Assistant
            }
        },
        TracePartKind::Thinking
        | TracePartKind::Tool
        | TracePartKind::Agent
        | TracePartKind::Turn
        | TracePartKind::Inference
        | TracePartKind::Plan => StudioMessageRole::Assistant,
    }
}

fn assistant_message_status_for_turn(status: StudioTurnStatus) -> Option<StudioMessageStatus> {
    match status {
        StudioTurnStatus::Completed => Some(StudioMessageStatus::Completed),
        StudioTurnStatus::Failed => Some(StudioMessageStatus::Failed),
        StudioTurnStatus::Cancelled => Some(StudioMessageStatus::Cancelled),
        StudioTurnStatus::Queued
        | StudioTurnStatus::ContextLoading
        | StudioTurnStatus::WaitingForModel
        | StudioTurnStatus::Streaming
        | StudioTurnStatus::RunningTool
        | StudioTurnStatus::WaitingForInteraction
        | StudioTurnStatus::Persisting => None,
    }
}

fn part_type_for_trace_kind(kind: TracePartKind) -> StudioPartType {
    match kind {
        TracePartKind::Text => StudioPartType::Text,
        TracePartKind::Thinking => StudioPartType::Reasoning,
        TracePartKind::Tool => StudioPartType::Tool,
        TracePartKind::Agent => StudioPartType::Agent,
        TracePartKind::Turn => StudioPartType::Turn,
        TracePartKind::Inference => StudioPartType::Inference,
        TracePartKind::Plan => StudioPartType::Plan,
    }
}

fn part_status_for_trace_status(status: TracePartStatus) -> StudioPartStatus {
    match status {
        TracePartStatus::Started => StudioPartStatus::Started,
        TracePartStatus::Streaming => StudioPartStatus::Streaming,
        TracePartStatus::AwaitingApproval => StudioPartStatus::AwaitingApproval,
        TracePartStatus::Approved => StudioPartStatus::Approved,
        TracePartStatus::Denied => StudioPartStatus::Denied,
        TracePartStatus::Running => StudioPartStatus::Running,
        TracePartStatus::Completed => StudioPartStatus::Completed,
        TracePartStatus::Failed => StudioPartStatus::Failed,
        TracePartStatus::Interrupted => StudioPartStatus::Interrupted,
        TracePartStatus::BudgetLimited => StudioPartStatus::BudgetLimited,
    }
}

fn studio_text_channel(channel: TraceTextChannel) -> StudioTextChannel {
    match channel {
        TraceTextChannel::User => StudioTextChannel::User,
        TraceTextChannel::Commentary => StudioTextChannel::Commentary,
        TraceTextChannel::Final => StudioTextChannel::Final,
    }
}

fn part_text(item: &TracePart) -> String {
    match item.kind {
        TracePartKind::Text | TracePartKind::Plan | TracePartKind::Turn => item.content.clone(),
        TracePartKind::Thinking => item
            .thinking_chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join(""),
        TracePartKind::Tool | TracePartKind::Agent | TracePartKind::Inference => {
            item.content.clone()
        }
    }
}

fn studio_tool_part(tool: TraceToolPart) -> StudioToolPart {
    StudioToolPart {
        tool_call_id: tool.tool_call_id,
        call_id: tool.call_id,
        provider_item_id: tool.provider_item_id,
        name: tool.name,
        arguments: tool.arguments,
        result: tool.result,
        exit_code: tool.exit_code,
        timed_out: tool.timed_out,
        working_directory: tool.working_directory,
        denial_reason: tool.denial_reason,
    }
}

fn studio_agent_part(agent: TraceAgentPart) -> StudioAgentPart {
    StudioAgentPart {
        id: agent.id,
        path: agent.path,
        parent_path: agent.parent_path,
        role: agent.role,
        task: agent.task,
        status: agent.status,
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
    }
}

fn studio_inference_part(inference: TraceInferencePart) -> StudioInferencePart {
    StudioInferencePart {
        inference_id: inference.inference_id,
        model: inference.model,
    }
}

fn error_for_part_status(status: StudioPartStatus, content: &str) -> Option<String> {
    match status {
        StudioPartStatus::Failed
        | StudioPartStatus::Interrupted
        | StudioPartStatus::BudgetLimited => {
            (!content.trim().is_empty()).then(|| content.to_string())
        }
        StudioPartStatus::Started
        | StudioPartStatus::Streaming
        | StudioPartStatus::AwaitingApproval
        | StudioPartStatus::Approved
        | StudioPartStatus::Denied
        | StudioPartStatus::Running
        | StudioPartStatus::Completed => None,
    }
}

#[allow(dead_code)]
fn _assert_agent_status_is_used(status: AgentStatus) -> AgentStatus {
    status
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::CompileMode;

    #[tokio::test]
    async fn assistant_message_lifecycle_follows_turn_not_part_status() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Visible progress", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store.clone());

        let commentary = TracePart::text(
            "turn-1",
            "commentary-1",
            10,
            TraceTextChannel::Commentary,
            "working",
            TracePartStatus::Completed,
            100,
        );
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartCompleted { item: commentary },
            )
            .await
            .unwrap();

        let messages = store.load_studio_messages(&session.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.status, StudioMessageStatus::Streaming);
        assert_eq!(messages[0].message.created_at, 100);
        assert_eq!(messages[0].message.completed_at, None);

        let final_answer = TracePart::text(
            "turn-1",
            "final-1",
            11,
            TraceTextChannel::Final,
            "done",
            TracePartStatus::Started,
            200,
        );
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartStarted { item: final_answer },
            )
            .await
            .unwrap();

        let messages = store.load_studio_messages(&session.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.status, StudioMessageStatus::Streaming);
        assert_eq!(messages[0].message.created_at, 100);
        assert_eq!(messages[0].message.completed_at, None);

        runtime
            .emit_turn(&session.id, "turn-1", StudioTurnStatus::Completed, None)
            .await
            .unwrap();

        let messages = store.load_studio_messages(&session.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message.status, StudioMessageStatus::Completed);
        assert_eq!(messages[0].message.created_at, 100);
        assert!(messages[0].message.completed_at.is_some());
    }

    #[tokio::test]
    async fn trace_part_order_is_allocated_by_runtime_not_trace_sequence() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Part order", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store.clone());

        let first = TracePart::text(
            "turn-order",
            "first-final",
            999,
            TraceTextChannel::Final,
            "first",
            TracePartStatus::Completed,
            100,
        );
        runtime
            .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item: first })
            .await
            .unwrap();

        let second = TracePart::text(
            "turn-order",
            "second-final",
            10,
            TraceTextChannel::Final,
            "second",
            TracePartStatus::Completed,
            101,
        );
        runtime
            .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item: second })
            .await
            .unwrap();

        let parts = store.load_message_parts(&session.id).await.unwrap();
        let compact = parts
            .into_iter()
            .map(|record| (record.part.part_id, record.part.order, record.part.text))
            .collect::<Vec<_>>();

        assert_eq!(
            compact,
            vec![
                ("first-final".to_string(), 0, "first".to_string()),
                ("second-final".to_string(), 1, "second".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn runtime_commentary_is_projected_as_synthetic() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Runtime commentary", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store.clone());

        let runtime_commentary =
            TracePart::runtime_commentary("turn-1", "progress-1", 1, "正在准备上下文。", 100);
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartCompleted {
                    item: runtime_commentary,
                },
            )
            .await
            .unwrap();

        let model_commentary = TracePart::text(
            "turn-1",
            "commentary-1",
            2,
            TraceTextChannel::Commentary,
            "模型进展",
            TracePartStatus::Completed,
            101,
        );
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartCompleted {
                    item: model_commentary,
                },
            )
            .await
            .unwrap();

        let parts = store.load_message_parts(&session.id).await.unwrap();
        let compact = parts
            .into_iter()
            .map(|record| {
                (
                    record.part.part_id,
                    record.part.text_channel,
                    record.part.synthetic,
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            compact,
            vec![
                (
                    "progress-1".to_string(),
                    Some(StudioTextChannel::Commentary),
                    true,
                ),
                (
                    "commentary-1".to_string(),
                    Some(StudioTextChannel::Commentary),
                    false,
                ),
            ]
        );
    }

    #[tokio::test]
    async fn trace_part_delta_requires_existing_part() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Delta guard", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store);
        let mut rx = runtime.subscribe_session(session.id.clone());

        let result = runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartDelta {
                    event: text_delta_event("turn-delta", "missing-part", 1, "hello"),
                },
            )
            .await
            .unwrap();

        assert!(result.is_none());
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event.kind,
            StudioEventKind::Stale { lagged_events: 1 }
        ));
    }

    #[tokio::test]
    async fn trace_part_delta_requires_contiguous_revision() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Delta revision", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store);
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartStarted {
                    item: streaming_text_part("turn-delta", "part-delta"),
                },
            )
            .await
            .unwrap();

        let first = runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartDelta {
                    event: text_delta_event("turn-delta", "part-delta", 1, "hel"),
                },
            )
            .await
            .unwrap()
            .unwrap();
        let StudioEventKind::MessagePartDelta { delta } = first.kind else {
            panic!("expected messagePartDelta");
        };
        assert_eq!(delta.revision, 1);
        assert_eq!(delta.delta, "hel");

        let mut rx = runtime.subscribe_session(session.id.clone());
        let duplicate = runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartDelta {
                    event: text_delta_event("turn-delta", "part-delta", 1, "lo"),
                },
            )
            .await
            .unwrap();

        assert!(duplicate.is_none());
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event.kind,
            StudioEventKind::Stale { lagged_events: 1 }
        ));
    }

    #[tokio::test]
    async fn trace_part_delta_after_terminal_snapshot_emits_stale() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Terminal delta", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store);
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartCompleted {
                    item: TracePart::text(
                        "turn-terminal",
                        "part-terminal",
                        0,
                        TraceTextChannel::Final,
                        "done",
                        TracePartStatus::Completed,
                        100,
                    ),
                },
            )
            .await
            .unwrap();
        let mut rx = runtime.subscribe_session(session.id.clone());

        let result = runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartDelta {
                    event: text_delta_event("turn-terminal", "part-terminal", 1, "late"),
                },
            )
            .await
            .unwrap();

        assert!(result.is_none());
        let event = rx.recv().await.unwrap();
        assert!(matches!(
            event.kind,
            StudioEventKind::Stale { lagged_events: 1 }
        ));
    }

    #[tokio::test]
    async fn terminal_snapshot_carries_latest_live_revision() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/studio").await.unwrap();
        let session = store
            .create_session(&project.id, "Terminal revision", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store.clone());
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartStarted {
                    item: streaming_text_part("turn-terminal-revision", "part-terminal-revision"),
                },
            )
            .await
            .unwrap();
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartDelta {
                    event: text_delta_event(
                        "turn-terminal-revision",
                        "part-terminal-revision",
                        1,
                        "done",
                    ),
                },
            )
            .await
            .unwrap();
        let mut completed = TracePart::text(
            "turn-terminal-revision",
            "part-terminal-revision",
            0,
            TraceTextChannel::Final,
            "done",
            TracePartStatus::Completed,
            100,
        );
        completed.updated_at = 101;

        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartCompleted { item: completed },
            )
            .await
            .unwrap();

        let part = store
            .read_message_part("part-terminal-revision")
            .await
            .unwrap()
            .unwrap()
            .part;
        assert_eq!(part.status, StudioPartStatus::Completed);
        assert_eq!(part.revision, 1);
    }

    fn streaming_text_part(turn_id: &str, item_id: &str) -> TracePart {
        TracePart::text(
            turn_id,
            item_id,
            0,
            TraceTextChannel::Final,
            "",
            TracePartStatus::Streaming,
            100,
        )
    }

    fn text_delta_event(
        turn_id: &str,
        item_id: &str,
        revision: u64,
        delta: &str,
    ) -> TracePartDeltaEvent {
        TracePartDeltaEvent {
            turn_id: turn_id.to_string(),
            item_id: item_id.to_string(),
            started_sequence: 0,
            revision,
            kind: TracePartKind::Text,
            status: TracePartStatus::Streaming,
            created_at: 100,
            updated_at: 100,
            delta: TraceDelta::Text {
                text_channel: TraceTextChannel::Final,
                delta: delta.to_string(),
            },
        }
    }
}
