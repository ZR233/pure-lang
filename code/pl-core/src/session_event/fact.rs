use pl_protocol::{SessionEventEnvelope, SessionEventKind, SessionEventPosition};

use super::projector::SessionEventProjectionBatch;

/// 已由框架或产品适配层确认的会话事实。
///
/// 产品只能向当前 agent 拥有的 session 记录事实，不能分配 durable sequence、构造事件
/// envelope 或直接广播。所有排序与投影仍由 `pl-core` 完成。
#[derive(Debug, Clone)]
pub struct SessionEventFact {
    pub source_event_id: String,
    pub source_agent_id: Option<String>,
    pub turn_id: Option<String>,
    pub emitted_at: i64,
    pub position: SessionEventFactPosition,
    pub kind: SessionEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEventFactPosition {
    Durable,
    Transient { revision: u64 },
}

impl SessionEventFact {
    pub fn durable(
        source_agent_id: Option<String>,
        turn_id: Option<String>,
        emitted_at: i64,
        kind: SessionEventKind,
    ) -> Self {
        Self {
            source_event_id: format!("fact-{emitted_at}"),
            source_agent_id,
            turn_id,
            emitted_at,
            position: SessionEventFactPosition::Durable,
            kind,
        }
    }
}

pub(crate) fn project_session_facts(
    session_id: &str,
    through_sequence: u64,
    facts: Vec<SessionEventFact>,
) -> SessionEventProjectionBatch {
    let mut sequence = through_sequence;
    let mut events = Vec::with_capacity(facts.len());
    for fact in facts {
        let kind = rebind_kind(fact.kind, session_id);
        let position = match fact.position {
            SessionEventFactPosition::Durable => {
                sequence = sequence.saturating_add(1);
                SessionEventPosition::Durable { sequence }
            }
            SessionEventFactPosition::Transient { revision } => {
                SessionEventPosition::Transient { revision }
            }
        };
        let event_id = match position {
            SessionEventPosition::Durable { sequence } => format!("{session_id}:{sequence}"),
            SessionEventPosition::Transient { revision } => {
                format!("{session_id}:transient:{}:{revision}", fact.source_event_id)
            }
        };
        events.push(SessionEventEnvelope {
            event_id,
            session_id: session_id.to_string(),
            source_agent_id: fact.source_agent_id,
            turn_id: fact.turn_id,
            emitted_at: fact.emitted_at,
            position,
            kind,
        });
    }
    SessionEventProjectionBatch {
        events,
        through_sequence: sequence,
    }
}

fn rebind_kind(mut kind: SessionEventKind, session_id: &str) -> SessionEventKind {
    match &mut kind {
        SessionEventKind::TurnChanged { turn } => turn.session_id = session_id.to_string(),
        SessionEventKind::MessageChanged { message } => message.session_id = session_id.to_string(),
        SessionEventKind::PartChanged { part } => part.session_id = session_id.to_string(),
        SessionEventKind::InteractionChanged { event } => {
            event.interaction.scope.session_id = session_id.to_string()
        }
        SessionEventKind::AgentChanged { agent } => agent.session_id = session_id.to_string(),
        SessionEventKind::TimelineEventAppended { event } => {
            event.session_id = session_id.to_string()
        }
        SessionEventKind::RuntimeChanged { runtime } => runtime.session_id = session_id.to_string(),
        SessionEventKind::MessageRemoved { .. }
        | SessionEventKind::PartRemoved { .. }
        | SessionEventKind::PartDelta { .. }
        | SessionEventKind::SkillActivated { .. }
        | SessionEventKind::PlanChanged { .. }
        | SessionEventKind::ContextCompacted { .. }
        | SessionEventKind::ErrorOccurred { .. } => {}
    }
    kind
}
