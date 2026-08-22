use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope, ThreadSnapshot};

use super::projector::{ThreadProjectionBatch, interaction_completion_turn};

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
    let mut active_turn = current.active_turn.clone();
    let mut notifications = Vec::new();
    for mut fact in facts {
        rebind_thread(&mut fact.notification, thread_id);
        if let ThreadNotification::InteractionChanged { interaction } = &fact.notification
            && let Some(turn) = interaction_completion_turn(
                thread_id,
                active_turn.as_ref(),
                interaction,
                fact.emitted_at,
            )
        {
            revision = revision.saturating_add(1);
            notifications.push(ThreadNotificationEnvelope {
                thread_id: thread_id.to_string(),
                revision,
                emitted_at: fact.emitted_at,
                notification: ThreadNotification::TurnCompleted { turn },
            });
            active_turn = None;
        }
        match &fact.notification {
            ThreadNotification::TurnStarted { turn } | ThreadNotification::TurnUpdated { turn } => {
                active_turn = Some(turn.clone());
            }
            ThreadNotification::TurnCompleted { turn }
                if active_turn
                    .as_ref()
                    .is_some_and(|active| active.id == turn.id) =>
            {
                active_turn = None;
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
        CompletedTurnState, InteractionCommand, InteractionRequest, InteractionScope,
        InteractionStatus, ResolveUserInput, RunningTurnState, THREAD_SCHEMA_VERSION, Thread,
        ThreadNotification, ThreadSnapshot, Turn, TurnCompletion, TurnPhase, TurnState,
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
    fn pending_interaction_completes_its_active_origin_turn() {
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

        // origin Turn 落 completed，随后只下发 InteractionChanged。
        assert_eq!(projected.notifications.len(), 2);
        let ThreadNotification::TurnCompleted { turn } = &projected.notifications[0].notification
        else {
            panic!("active origin turn must complete on pending interaction");
        };
        assert_eq!(turn.id, "turn-1");
        assert_eq!(
            turn.state,
            TurnState::Completed(CompletedTurnState::new(
                Some(1),
                2,
                TurnCompletion::InteractionRequested,
            ))
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
            revision: 0,
            state: TurnState::Running(RunningTurnState::new(1, TurnPhase::Thinking)),
            updated_at: 1,
        }
    }

    fn interaction(status: InteractionStatus) -> InteractionRequest {
        let mut interaction = InteractionRequest::user_input(
            "ask-1",
            InteractionScope {
                thread_id: String::new(),
                turn_id: "turn-1".to_string(),
                item_id: None,
                tool_id: None,
                agent_path: None,
            },
            Vec::new(),
            1,
        );
        if status == InteractionStatus::Resolved {
            let decision = interaction
                .decide(InteractionCommand::ResolveUserInput(ResolveUserInput {
                    interaction_id: interaction.interaction_id.clone(),
                    expected_revision: interaction.revision,
                    operation_id: "resolve-1".to_string(),
                    resolved_at: 2,
                    answers: Default::default(),
                }))
                .unwrap();
            interaction.apply(decision, 2);
        }
        interaction
    }
}
