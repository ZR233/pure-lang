use pl_protocol::{
    InteractionStatus, ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot, Turn,
    TurnPhase, TurnState,
};

use super::projector::ThreadProjectionBatch;

#[derive(Debug, Clone)]
pub struct ThreadNotificationFact {
    pub emitted_at: i64,
    pub notification: ThreadNotification,
}

impl ThreadNotificationFact {
    pub fn durable(emitted_at: i64, notification: ThreadNotification) -> Self {
        Self {
            emitted_at,
            notification,
        }
    }
}

pub(crate) fn project_thread_facts(
    thread_id: &str,
    current: &ThreadSnapshot,
    facts: Vec<ThreadNotificationFact>,
) -> ThreadProjectionBatch {
    let mut revision = current.revision;
    let mut active_turn_id = current.active_turn.as_ref().map(|turn| turn.id.clone());
    let mut notifications = Vec::new();
    for mut fact in facts {
        rebind_thread(&mut fact.notification, thread_id);
        if let ThreadNotification::InteractionChanged { interaction } = &fact.notification
            && active_turn_id.as_deref() == Some(interaction.scope.turn_id.as_str())
        {
            revision = revision.saturating_add(1);
            notifications.push(ThreadNotificationEnvelope {
                thread_id: thread_id.to_string(),
                revision,
                emitted_at: fact.emitted_at,
                notification: ThreadNotification::TurnUpdated {
                    turn: Turn {
                        id: interaction.scope.turn_id.clone(),
                        thread_id: thread_id.to_string(),
                        state: TurnState::InProgress {
                            phase: if interaction.status == InteractionStatus::Pending {
                                TurnPhase::WaitingInteraction
                            } else {
                                TurnPhase::Thinking
                            },
                        },
                        failure: None,
                        started_at: None,
                        updated_at: fact.emitted_at,
                        completed_at: None,
                    },
                },
            });
        }
        match &fact.notification {
            ThreadNotification::TurnStarted { turn } | ThreadNotification::TurnUpdated { turn } => {
                active_turn_id = Some(turn.id.clone());
            }
            ThreadNotification::TurnCompleted { turn }
                if active_turn_id.as_deref() == Some(turn.id.as_str()) =>
            {
                active_turn_id = None;
            }
            ThreadNotification::TurnCompleted { .. }
            | ThreadNotification::ItemStarted { .. }
            | ThreadNotification::ItemDelta { .. }
            | ThreadNotification::ItemCompleted { .. }
            | ThreadNotification::InteractionChanged { .. }
            | ThreadNotification::ThreadRuntimeUpdated { .. }
            | ThreadNotification::Lagged { .. } => {}
        }
        revision = revision.saturating_add(1);
        notifications.push(ThreadNotificationEnvelope {
            thread_id: thread_id.to_string(),
            revision,
            emitted_at: fact.emitted_at,
            notification: fact.notification,
        });
    }
    ThreadProjectionBatch {
        notifications,
        through_revision: revision,
    }
}

fn rebind_thread(notification: &mut ThreadNotification, thread_id: &str) {
    match notification {
        ThreadNotification::TurnStarted { turn }
        | ThreadNotification::TurnUpdated { turn }
        | ThreadNotification::TurnCompleted { turn } => turn.thread_id = thread_id.to_string(),
        ThreadNotification::ItemStarted { item } | ThreadNotification::ItemCompleted { item } => {
            item.thread_id = thread_id.to_string()
        }
        ThreadNotification::InteractionChanged { interaction } => {
            interaction.scope.thread_id = thread_id.to_string();
        }
        ThreadNotification::ThreadRuntimeUpdated { runtime } => {
            runtime.thread_id = thread_id.to_string();
        }
        ThreadNotification::ItemDelta { .. } | ThreadNotification::Lagged { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        InteractionKind, InteractionPayload, InteractionRequest, InteractionScope,
        InteractionStatus, THREAD_SCHEMA_VERSION, Thread, ThreadNotification, ThreadSnapshot, Turn,
        TurnPhase, TurnState,
    };

    use super::*;

    #[test]
    fn resolved_interaction_does_not_revive_terminal_origin_turn() {
        let current = snapshot(None);
        let projected = project_thread_facts(
            "thread-1",
            &current,
            vec![ThreadNotificationFact::durable(
                2,
                ThreadNotification::InteractionChanged {
                    interaction: Box::new(interaction(InteractionStatus::Resolved)),
                },
            )],
        );

        assert_eq!(projected.notifications.len(), 1);
        assert!(matches!(
            projected.notifications[0].notification,
            ThreadNotification::InteractionChanged { .. }
        ));
    }

    #[test]
    fn pending_interaction_updates_only_its_active_origin_turn() {
        let current = snapshot(Some(turn("turn-1")));
        let projected = project_thread_facts(
            "thread-1",
            &current,
            vec![ThreadNotificationFact::durable(
                2,
                ThreadNotification::InteractionChanged {
                    interaction: Box::new(interaction(InteractionStatus::Pending)),
                },
            )],
        );

        assert_eq!(projected.notifications.len(), 2);
        let ThreadNotification::TurnUpdated { turn } = &projected.notifications[0].notification
        else {
            panic!("active origin turn must receive a phase update");
        };
        assert_eq!(turn.id, "turn-1");
        assert_eq!(
            turn.state,
            TurnState::InProgress {
                phase: TurnPhase::WaitingInteraction
            }
        );
    }

    fn snapshot(active_turn: Option<Turn>) -> ThreadSnapshot {
        ThreadSnapshot {
            schema_version: THREAD_SCHEMA_VERSION,
            revision: 7,
            thread: Thread::placeholder("thread-1"),
            active_turn,
            items: Vec::new(),
            interactions: Vec::new(),
            runtime: None,
        }
    }

    fn turn(id: &str) -> Turn {
        Turn {
            id: id.to_string(),
            thread_id: "thread-1".to_string(),
            state: TurnState::InProgress {
                phase: TurnPhase::Thinking,
            },
            failure: None,
            started_at: Some(1),
            updated_at: 1,
            completed_at: None,
        }
    }

    fn interaction(status: InteractionStatus) -> InteractionRequest {
        InteractionRequest {
            interaction_id: "ask-1".to_string(),
            kind: InteractionKind::UserInput,
            status,
            scope: InteractionScope {
                thread_id: String::new(),
                turn_id: "turn-1".to_string(),
                item_id: None,
                tool_id: None,
                agent_path: None,
            },
            payload: InteractionPayload::UserInput {
                questions: Vec::new(),
            },
            created_at: 1,
            updated_at: 2,
            resolved_at: Some(2),
            resolution: None,
        }
    }
}
