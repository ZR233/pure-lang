use anyhow::Result;
use pl_protocol::{
    AgentStatus, InteractionChangedEvent, StudioAgentPart, StudioAgentSnapshot,
    StudioAgentTimelineEvent, StudioAgentTimelineEventKind, StudioAttachment, StudioEventEnvelope,
    StudioEventKind, StudioInferencePart, StudioMessage, StudioMessageRole, StudioMessageStatus,
    StudioPart, StudioPartDelta, StudioPartDeltaField, StudioPartStatus, StudioPartType,
    StudioPlanPart, StudioSessionHandoff, StudioTextChannel, StudioToolPart, StudioTurn,
    StudioTurnStatus,
};
use pl_trace::{
    AgentEvent, TraceAgentPart, TraceDelta, TraceInferencePart, TracePart, TracePartDeltaEvent,
    TracePartKind, TracePartStatus, TraceTextChannel, TraceToolPart,
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
                let turn_id = event.turn_id.clone();
                let delta = studio_part_delta(session_id, event);
                return self
                    .emit_live(
                        None,
                        Some(session_id.to_string()),
                        Some(turn_id),
                        StudioEventKind::MessagePartDelta { delta },
                    )
                    .await
                    .map(Some);
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
        let message = studio_message_from_trace_part(session_id, &item);
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(item.turn_id.clone()),
            StudioEventKind::MessageUpdated {
                message: Box::new(message),
            },
        )
        .await?;
        let part = studio_part_from_trace_part(session_id, item);
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(part.turn_id.clone()),
            StudioEventKind::MessagePartUpdated {
                part: Box::new(part),
            },
        )
        .await
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
        TraceDelta::Thinking { .. } => StudioPartDeltaField::ReasoningText,
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

fn studio_message_from_trace_part(session_id: &str, item: &TracePart) -> StudioMessage {
    let role = message_role_for_trace_part(item);
    let status = message_status_for_trace_part(item);
    let completed_at = matches!(
        status,
        StudioMessageStatus::Completed
            | StudioMessageStatus::Failed
            | StudioMessageStatus::Cancelled
    )
    .then_some(item.updated_at);
    StudioMessage {
        message_id: message_id_for_trace_part(item),
        session_id: session_id.to_string(),
        turn_id: item.turn_id.clone(),
        role,
        status,
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at,
        error: error_for_trace_part(item),
        metadata: serde_json::json!({}),
    }
}

fn studio_part_from_trace_part(session_id: &str, item: TracePart) -> StudioPart {
    let part_type = part_type_for_trace_kind(item.kind);
    let status = part_status_for_trace_status(item.status);
    let completed_at = matches!(
        status,
        StudioPartStatus::Completed
            | StudioPartStatus::Failed
            | StudioPartStatus::Interrupted
            | StudioPartStatus::BudgetLimited
    )
    .then_some(item.updated_at);
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
        synthetic: matches!(part_type, StudioPartType::Turn | StudioPartType::Inference),
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

fn message_status_for_trace_part(item: &TracePart) -> StudioMessageStatus {
    match item.status {
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Running => StudioMessageStatus::Streaming,
        TracePartStatus::Completed | TracePartStatus::Denied => StudioMessageStatus::Completed,
        TracePartStatus::Failed | TracePartStatus::BudgetLimited => StudioMessageStatus::Failed,
        TracePartStatus::Interrupted => StudioMessageStatus::Cancelled,
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

fn error_for_trace_part(item: &TracePart) -> Option<String> {
    match item.status {
        TracePartStatus::Failed | TracePartStatus::Interrupted | TracePartStatus::BudgetLimited => {
            (!item.content.trim().is_empty()).then(|| item.content.clone())
        }
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Denied
        | TracePartStatus::Running
        | TracePartStatus::Completed => None,
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
