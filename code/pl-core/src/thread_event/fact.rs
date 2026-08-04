use pl_protocol::{
    InteractionStatus, ThreadNotification, ThreadNotificationEnvelope, Turn, TurnPhase, TurnState,
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
    mut revision: u64,
    facts: Vec<ThreadNotificationFact>,
) -> ThreadProjectionBatch {
    let mut notifications = Vec::new();
    for mut fact in facts {
        rebind_thread(&mut fact.notification, thread_id);
        if let ThreadNotification::InteractionChanged { interaction } = &fact.notification {
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
                        started_at: None,
                        updated_at: fact.emitted_at,
                        completed_at: None,
                    },
                },
            });
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
