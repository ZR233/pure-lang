//! Side-effect-free restoration; execution resumes only after the host installs fresh resources.
use super::*;

pub(super) fn settle(mut state: ThreadSnapshot) -> Result<ThreadSnapshot, ThreadError> {
    let calls = state.context.pending_calls()?;
    if !calls.is_empty() {
        let revision = state
            .context
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        let mut records = state.context.records.to_vec();
        let mut identities = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut deliveries = state.deliveries.to_vec();
        for call in calls {
            let turn_id = state
                .context
                .records
                .iter()
                .find(|record| {
                    record
                        .tool_calls
                        .iter()
                        .any(|original| original.call_id == call.call_id)
                })
                .and_then(|record| record.turn_id.clone());
            let content = vec![ContextContent::Text {
                text: Arc::from(
                    "Tool execution was interrupted by runtime restart. The operation was not re-executed during recovery.",
                ),
            }];
            let output = crate::tool::ToolOutput::new(
                OpaquePayload::text("interrupted during recovery"),
                content.clone(),
            );
            let mut suffix = 0_u64;
            let id = loop {
                let id = format!("recovery:{revision}:{}:{suffix}", call.call_id);
                if identities.insert(id.clone()) {
                    break id;
                }
                suffix += 1;
            };
            records.push(ContextRecord {
                id,
                turn_id,
                source: ContextSource::ToolResult {
                    call_id: call.call_id.clone(),
                    tool_id: call.tool_id.clone(),
                },
                content: content.clone(),
                tool_calls: Vec::new(),
            });
            deliveries.push(ToolDelivery {
                target: ToolDeliveryTarget::CallResult,
                call_id: call.call_id,
                tool_id: call.tool_id,
                output,
                delivered_context: content,
                outcome: ToolOutcome::Interrupted,
            });
        }
        state.context = ContextSnapshot {
            revision,
            records: records.into(),
        };
        state.deliveries = deliveries.into();
    }
    let mut attempts = state.attempts.to_vec();
    if let Some(attempt) = attempts.last_mut()
        && matches!(attempt.outcome, AttemptOutcome::Running)
    {
        attempt.outcome = AttemptOutcome::Interrupted;
        state.attempts = attempts.into();
    }
    let mut turns = state.turns.to_vec();
    if let Some(turn) = turns.last_mut()
        && turn.state == TurnState::Running
    {
        turn.state = TurnState::Interrupted;
        state.turns = turns.into();
    }
    let interrupted = state
        .tasks
        .values()
        .filter(|task| task.status == task::TaskStatus::Running)
        .cloned()
        .collect::<Vec<_>>();
    for mut record in interrupted {
        if record.acknowledgement.is_some() {
            let output = crate::tool::ToolOutput::new(
                OpaquePayload::text("Task interrupted by runtime restart."),
                vec![ContextContent::Text {
                    text: Arc::from("Task interrupted by runtime restart; it was not re-executed."),
                }],
            );
            let context = super::background::result_context(
                &record,
                task::TaskStatus::Interrupted,
                output.context().to_vec(),
            );
            let message_id = super::background::append_result_message(
                &mut state,
                &record,
                &output,
                context.clone(),
            )?;
            let mut deliveries = state.deliveries.to_vec();
            deliveries.push(ToolDelivery {
                target: ToolDeliveryTarget::Inbox { message_id },
                call_id: record.call_id.clone(),
                tool_id: record.tool_id.clone(),
                output,
                delivered_context: context,
                outcome: ToolOutcome::Interrupted,
            });
            state.deliveries = deliveries.into();
        }
        record.revision = record
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        record.status = task::TaskStatus::Interrupted;
        task::record_change(&mut state, record);
    }
    let permissions = state.permissions.keys().cloned().collect::<Vec<_>>();
    for id in permissions {
        permissions::cancel_pending(&mut state, &id)?;
    }
    state.private_context = None;
    state.lifecycle = ThreadLifecycle::Open;
    state.persistence = Default::default();
    state.context.validate_complete()?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_marks_an_unfinished_turn_interrupted_without_creating_an_attempt() {
        let mut state = ThreadSnapshot::default();
        state.turns = vec![TurnRecord {
            elapsed_ms: None,
            input_id: None,
            turn_id: "turn".into(),
            state: TurnState::Running,
            model_steps: 0,
        }]
        .into();
        let restored = settle(state).unwrap();
        assert_eq!(restored.turns[0].state, TurnState::Interrupted);
        assert!(restored.attempts.is_empty());
        assert!(restored.deliveries.is_empty());
    }
}
