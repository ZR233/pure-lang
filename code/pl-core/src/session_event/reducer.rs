use pl_protocol::{
    InteractionStatus, SessionEventEnvelope, SessionEventKind, SessionEventPosition,
    SessionPartContent, SessionPartDeltaField, SessionViewSnapshot,
};

use super::SessionEventError;

pub(super) fn apply_session_event(
    snapshot: &mut SessionViewSnapshot,
    event: &SessionEventEnvelope,
) -> Result<(), SessionEventError> {
    if event.session_id != snapshot.session_id {
        return Err(SessionEventError::SessionMismatch {
            expected: snapshot.session_id.clone(),
            actual: event.session_id.clone(),
        });
    }
    match &event.kind {
        SessionEventKind::TurnChanged { turn } => snapshot.turn = Some(turn.clone()),
        SessionEventKind::MessageChanged { message } => {
            upsert_by(&mut snapshot.messages, (**message).clone(), |candidate| {
                candidate.message_id.as_str()
            })
        }
        SessionEventKind::MessageRemoved { message_id } => {
            snapshot
                .messages
                .retain(|message| message.message_id != *message_id);
            snapshot.parts.retain(|part| part.message_id != *message_id);
        }
        SessionEventKind::PartChanged { part } => {
            upsert_by(&mut snapshot.parts, (**part).clone(), |candidate| {
                candidate.part_id.as_str()
            })
        }
        SessionEventKind::PartRemoved {
            message_id,
            part_id,
        } => snapshot
            .parts
            .retain(|part| part.message_id != *message_id || part.part_id != *part_id),
        SessionEventKind::PartDelta { delta } => {
            let Some(part) = snapshot
                .parts
                .iter_mut()
                .find(|part| part.part_id == delta.part_id)
            else {
                return Err(SessionEventError::ProjectionInvariant(format!(
                    "delta targets missing part {}",
                    delta.part_id
                )));
            };
            let expected = part.revision.saturating_add(1);
            if delta.revision != expected {
                return Err(SessionEventError::RevisionGap {
                    part_id: delta.part_id.clone(),
                    expected,
                    actual: delta.revision,
                });
            }
            apply_delta(part, delta.field, &delta.delta)?;
            part.revision = delta.revision;
            if let SessionEventPosition::Transient { revision } = event.position
                && revision != delta.revision
            {
                return Err(SessionEventError::RevisionGap {
                    part_id: delta.part_id.clone(),
                    expected: delta.revision,
                    actual: revision,
                });
            }
        }
        SessionEventKind::InteractionChanged { event } => {
            snapshot.interactions.retain(|interaction| {
                interaction.interaction_id != event.interaction.interaction_id
            });
            if event.interaction.status == InteractionStatus::Pending {
                snapshot.interactions.push(event.interaction.clone());
            }
        }
        SessionEventKind::AgentChanged { agent } => {
            upsert_by(&mut snapshot.agents, agent.clone(), |candidate| {
                candidate.id.as_str()
            });
            if let Some(runtime) = &mut snapshot.runtime {
                runtime.agent_count = snapshot.agents.len().try_into().unwrap_or(u32::MAX);
            }
        }
        SessionEventKind::TimelineEventAppended { event } => {
            upsert_by(&mut snapshot.timeline_events, event.clone(), |candidate| {
                candidate.event_id.as_str()
            })
        }
        SessionEventKind::RuntimeChanged { runtime } => {
            snapshot.runtime = Some((**runtime).clone())
        }
        SessionEventKind::SkillActivated { activation } => {
            upsert_by(
                &mut snapshot.activated_skills,
                activation.clone(),
                |candidate| candidate.name.as_str(),
            );
            if let Some(runtime) = &mut snapshot.runtime
                && !runtime.active_skills.contains(&activation.name)
            {
                runtime.active_skills.push(activation.name.clone());
            }
        }
        SessionEventKind::PlanChanged { event } => snapshot.plan_events.push(event.clone()),
        SessionEventKind::ContextCompacted { .. } | SessionEventKind::ErrorOccurred { .. } => {}
    }
    Ok(())
}

fn apply_delta(
    part: &mut pl_protocol::SessionPart,
    field: SessionPartDeltaField,
    delta: &str,
) -> Result<(), SessionEventError> {
    match (&mut part.content, field) {
        (SessionPartContent::Text { text, .. }, SessionPartDeltaField::Text)
        | (SessionPartContent::Reasoning { text }, SessionPartDeltaField::ReasoningSummary) => {
            text.push_str(delta)
        }
        (SessionPartContent::Plan { content }, SessionPartDeltaField::PlanContent) => {
            content.push_str(delta)
        }
        (SessionPartContent::Tool { tool }, SessionPartDeltaField::ToolArguments) => {
            tool.arguments.push_str(delta)
        }
        (SessionPartContent::Tool { tool }, SessionPartDeltaField::ToolResult) => {
            tool.result.get_or_insert_default().push_str(delta)
        }
        (content, field) => {
            return Err(SessionEventError::ProjectionInvariant(format!(
                "delta field {field:?} is incompatible with part content {content:?}"
            )));
        }
    }
    Ok(())
}

fn upsert_by<T, F>(items: &mut Vec<T>, replacement: T, key: F)
where
    F: Fn(&T) -> &str,
{
    let replacement_key = key(&replacement).to_string();
    if let Some(existing) = items
        .iter_mut()
        .find(|candidate| key(candidate) == replacement_key)
    {
        *existing = replacement;
    } else {
        items.push(replacement);
    }
}
