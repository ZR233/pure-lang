use anyhow::Result;
use pl_protocol::{
    AgentEvent, AgentStatus, InteractionChangedEvent, StudioAgentPart, StudioAgentSnapshot,
    StudioAgentTimelineEvent, StudioAttachment, StudioEventEnvelope, StudioEventKind,
    StudioInferencePart, StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart,
    StudioPartDelta, StudioPartDeltaField, StudioPartStatus, StudioPartType, StudioPlanPart,
    StudioSessionHandoff, StudioTextChannel, StudioToolPart, StudioTurn, StudioTurnStatus,
    TimelineAgentItem, TimelineDelta, TimelineInferenceItem, TimelineItem, TimelineItemDeltaEvent,
    TimelineItemKind, TimelineItemStatus, TimelineTextChannel, TimelineToolItem,
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
            AgentEvent::TimelineItemStarted { item } => {
                return self
                    .emit_timeline_item_snapshot(session_id, item)
                    .await
                    .map(Some);
            }
            AgentEvent::TimelineItemDelta { event } => {
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
            AgentEvent::TimelineItemCompleted { item } => {
                return self
                    .emit_timeline_item_snapshot(session_id, item)
                    .await
                    .map(Some);
            }
            AgentEvent::TimelineItemFailed { item, error } => {
                let mut item = item;
                if item.content.trim().is_empty() {
                    item.content = error;
                }
                return self
                    .emit_timeline_item_snapshot(session_id, item)
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
                event: StudioAgentTimelineEvent {
                    payload: serde_json::to_value(&event)?,
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
            .emit_live(
                None,
                Some(session_id.to_string()),
                None,
                StudioEventKind::Stale { lagged_events },
            )
            .await?;
        Ok(())
    }

    async fn emit_timeline_item_snapshot(
        &self,
        session_id: &str,
        item: TimelineItem,
    ) -> Result<StudioEventEnvelope> {
        let message = studio_message_from_timeline_item(session_id, &item);
        self.emit(
            None,
            Some(session_id.to_string()),
            Some(item.turn_id.clone()),
            StudioEventKind::MessageUpdated {
                message: Box::new(message),
            },
        )
        .await?;
        let part = studio_part_from_timeline_item(session_id, item);
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

fn studio_part_delta(session_id: &str, event: TimelineItemDeltaEvent) -> StudioPartDelta {
    let field = match &event.delta {
        TimelineDelta::Text { .. } => StudioPartDeltaField::Text,
        TimelineDelta::Thinking { .. } => StudioPartDeltaField::ReasoningText,
        TimelineDelta::ToolArguments { .. } => StudioPartDeltaField::ToolArguments,
        TimelineDelta::ToolResult { .. } => StudioPartDeltaField::ToolResult,
        TimelineDelta::Plan { .. } => StudioPartDeltaField::PlanContent,
    };
    let chunk_index = match &event.delta {
        TimelineDelta::Thinking { chunk_index, .. } => Some(*chunk_index),
        TimelineDelta::Text { .. }
        | TimelineDelta::ToolArguments { .. }
        | TimelineDelta::ToolResult { .. }
        | TimelineDelta::Plan { .. } => None,
    };
    StudioPartDelta {
        session_id: session_id.to_string(),
        message_id: message_id_for_timeline_event(&event),
        part_id: event.item_id,
        field,
        delta: timeline_delta_text(event.delta),
        chunk_index,
    }
}

fn timeline_delta_text(delta: TimelineDelta) -> String {
    match delta {
        TimelineDelta::Text { delta, .. }
        | TimelineDelta::Thinking { delta, .. }
        | TimelineDelta::ToolArguments { delta }
        | TimelineDelta::ToolResult { delta }
        | TimelineDelta::Plan { delta } => delta,
    }
}

fn studio_message_from_timeline_item(session_id: &str, item: &TimelineItem) -> StudioMessage {
    let role = message_role_for_timeline_item(item);
    let status = message_status_for_timeline_item(item);
    let completed_at = matches!(
        status,
        StudioMessageStatus::Completed
            | StudioMessageStatus::Failed
            | StudioMessageStatus::Cancelled
    )
    .then_some(item.updated_at);
    StudioMessage {
        message_id: message_id_for_timeline_item(item),
        session_id: session_id.to_string(),
        turn_id: item.turn_id.clone(),
        role,
        status,
        created_at: item.created_at,
        updated_at: item.updated_at,
        completed_at,
        error: error_for_timeline_item(item),
        metadata: serde_json::json!({}),
    }
}

fn studio_part_from_timeline_item(session_id: &str, item: TimelineItem) -> StudioPart {
    let part_type = part_type_for_timeline_kind(item.kind);
    let status = part_status_for_timeline_status(item.status);
    let completed_at = matches!(
        status,
        StudioPartStatus::Completed
            | StudioPartStatus::Failed
            | StudioPartStatus::Interrupted
            | StudioPartStatus::BudgetLimited
    )
    .then_some(item.updated_at);
    let text = part_text(&item);
    let message_id = message_id_for_timeline_item(&item);
    let error = error_for_part_status(status, &item.content);
    let tool = item.tool.map(studio_tool_part);
    let agent = item.agent.map(studio_agent_part);
    let inference = item.inference.map(studio_inference_part);
    let plan = matches!(part_type, StudioPartType::Plan).then(|| StudioPlanPart {
        content: item.content.clone(),
    });
    StudioPart {
        part_id: item.item_id,
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

fn message_id_for_timeline_item(item: &TimelineItem) -> String {
    let suffix = match message_role_for_timeline_item(item) {
        StudioMessageRole::User => "user",
        StudioMessageRole::Assistant => "assistant",
        StudioMessageRole::System => "system",
    };
    format!("{}:{suffix}", item.turn_id)
}

fn message_id_for_timeline_event(event: &TimelineItemDeltaEvent) -> String {
    let suffix = match event.kind {
        TimelineItemKind::Text => match &event.delta {
            TimelineDelta::Text {
                text_channel: TimelineTextChannel::User,
                ..
            } => "user",
            TimelineDelta::Text {
                text_channel: TimelineTextChannel::Commentary | TimelineTextChannel::Final,
                ..
            } => "assistant",
            TimelineDelta::Thinking { .. }
            | TimelineDelta::ToolArguments { .. }
            | TimelineDelta::ToolResult { .. }
            | TimelineDelta::Plan { .. } => "assistant",
        },
        TimelineItemKind::Thinking
        | TimelineItemKind::Tool
        | TimelineItemKind::Agent
        | TimelineItemKind::Turn
        | TimelineItemKind::Inference
        | TimelineItemKind::Plan => "assistant",
    };
    format!("{}:{suffix}", event.turn_id)
}

fn message_role_for_timeline_item(item: &TimelineItem) -> StudioMessageRole {
    match item.kind {
        TimelineItemKind::Text => match item.text_channel {
            Some(TimelineTextChannel::User) => StudioMessageRole::User,
            Some(TimelineTextChannel::Commentary | TimelineTextChannel::Final) | None => {
                StudioMessageRole::Assistant
            }
        },
        TimelineItemKind::Thinking
        | TimelineItemKind::Tool
        | TimelineItemKind::Agent
        | TimelineItemKind::Turn
        | TimelineItemKind::Inference
        | TimelineItemKind::Plan => StudioMessageRole::Assistant,
    }
}

fn message_status_for_timeline_item(item: &TimelineItem) -> StudioMessageStatus {
    match item.status {
        TimelineItemStatus::Started
        | TimelineItemStatus::Streaming
        | TimelineItemStatus::AwaitingApproval
        | TimelineItemStatus::Approved
        | TimelineItemStatus::Running => StudioMessageStatus::Streaming,
        TimelineItemStatus::Completed | TimelineItemStatus::Denied => {
            StudioMessageStatus::Completed
        }
        TimelineItemStatus::Failed | TimelineItemStatus::BudgetLimited => {
            StudioMessageStatus::Failed
        }
        TimelineItemStatus::Interrupted => StudioMessageStatus::Cancelled,
    }
}

fn part_type_for_timeline_kind(kind: TimelineItemKind) -> StudioPartType {
    match kind {
        TimelineItemKind::Text => StudioPartType::Text,
        TimelineItemKind::Thinking => StudioPartType::Reasoning,
        TimelineItemKind::Tool => StudioPartType::Tool,
        TimelineItemKind::Agent => StudioPartType::Agent,
        TimelineItemKind::Turn => StudioPartType::Turn,
        TimelineItemKind::Inference => StudioPartType::Inference,
        TimelineItemKind::Plan => StudioPartType::Plan,
    }
}

fn part_status_for_timeline_status(status: TimelineItemStatus) -> StudioPartStatus {
    match status {
        TimelineItemStatus::Started => StudioPartStatus::Started,
        TimelineItemStatus::Streaming => StudioPartStatus::Streaming,
        TimelineItemStatus::AwaitingApproval => StudioPartStatus::AwaitingApproval,
        TimelineItemStatus::Approved => StudioPartStatus::Approved,
        TimelineItemStatus::Denied => StudioPartStatus::Denied,
        TimelineItemStatus::Running => StudioPartStatus::Running,
        TimelineItemStatus::Completed => StudioPartStatus::Completed,
        TimelineItemStatus::Failed => StudioPartStatus::Failed,
        TimelineItemStatus::Interrupted => StudioPartStatus::Interrupted,
        TimelineItemStatus::BudgetLimited => StudioPartStatus::BudgetLimited,
    }
}

fn studio_text_channel(channel: TimelineTextChannel) -> StudioTextChannel {
    match channel {
        TimelineTextChannel::User => StudioTextChannel::User,
        TimelineTextChannel::Commentary => StudioTextChannel::Commentary,
        TimelineTextChannel::Final => StudioTextChannel::Final,
    }
}

fn part_text(item: &TimelineItem) -> String {
    match item.kind {
        TimelineItemKind::Text | TimelineItemKind::Plan | TimelineItemKind::Turn => {
            item.content.clone()
        }
        TimelineItemKind::Thinking => item
            .thinking_chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join(""),
        TimelineItemKind::Tool | TimelineItemKind::Agent | TimelineItemKind::Inference => {
            item.content.clone()
        }
    }
}

fn studio_tool_part(tool: TimelineToolItem) -> StudioToolPart {
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

fn studio_agent_part(agent: TimelineAgentItem) -> StudioAgentPart {
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

fn studio_inference_part(inference: TimelineInferenceItem) -> StudioInferencePart {
    StudioInferencePart {
        inference_id: inference.inference_id,
        model: inference.model,
    }
}

fn error_for_timeline_item(item: &TimelineItem) -> Option<String> {
    match item.status {
        TimelineItemStatus::Failed
        | TimelineItemStatus::Interrupted
        | TimelineItemStatus::BudgetLimited => {
            (!item.content.trim().is_empty()).then(|| item.content.clone())
        }
        TimelineItemStatus::Started
        | TimelineItemStatus::Streaming
        | TimelineItemStatus::AwaitingApproval
        | TimelineItemStatus::Approved
        | TimelineItemStatus::Denied
        | TimelineItemStatus::Running
        | TimelineItemStatus::Completed => None,
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
