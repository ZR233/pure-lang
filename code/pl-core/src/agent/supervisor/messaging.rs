use pl_protocol::{PureError, SubAgentActivityKind};

use super::snapshot::clear_for_reactivation;
use super::{
    AgentMessage, AgentMessageMode, AgentMessageRequest, AgentRecord, AgentStatus, AgentSupervisor,
};

impl AgentSupervisor {
    pub async fn send_message(
        &self,
        request: AgentMessageRequest<'_>,
    ) -> Result<AgentRecord, PureError> {
        let AgentMessageRequest {
            current_path,
            target,
            message,
            mode,
            run_spec,
            event_tx,
            call_id,
        } = request;
        if message.trim().is_empty() {
            return Err(PureError::ToolExecutionFailed {
                tool: "send_message".to_string(),
                error: "message must not be empty".to_string(),
            });
        }
        let activity_message = message.clone();
        let mut trigger_run_spec = if mode == AgentMessageMode::TriggerTurn {
            Some(run_spec.ok_or_else(|| PureError::ToolExecutionFailed {
                tool: "followup_task".to_string(),
                error: "missing follow-up run configuration".to_string(),
            })?)
        } else {
            None
        };
        let agent_id = self.resolve_agent(current_path, target).await?;
        let execution_guard = if mode == AgentMessageMode::TriggerTurn {
            Some(self.reserve_agent_execution()?)
        } else {
            None
        };
        let (record, followup_messages) = {
            let mut state = self.state.lock().await;
            let entry =
                state
                    .agents
                    .get_mut(&agent_id)
                    .ok_or_else(|| PureError::ToolExecutionFailed {
                        tool: "send_message".to_string(),
                        error: format!("target agent not found: {target}"),
                    })?;
            if entry.record.path == super::AgentPath::ROOT && mode == AgentMessageMode::TriggerTurn
            {
                return Err(PureError::ToolExecutionFailed {
                    tool: "followup_task".to_string(),
                    error: "tasks cannot be assigned to the root agent".to_string(),
                });
            }
            if entry.record.status.is_final() {
                let tool = if mode == AgentMessageMode::TriggerTurn {
                    "followup_task"
                } else {
                    "send_message"
                };
                return Err(PureError::ToolExecutionFailed {
                    tool: tool.to_string(),
                    error: format!(
                        "target agent {} is already {}",
                        entry.record.path,
                        entry.record.status.as_str()
                    ),
                });
            }
            if mode == AgentMessageMode::TriggerTurn
                && matches!(
                    entry.record.status,
                    AgentStatus::Queued | AgentStatus::Running
                )
            {
                return Err(PureError::ToolExecutionFailed {
                    tool: "followup_task".to_string(),
                    error: format!(
                        "target agent {} is already {}",
                        entry.record.path,
                        entry.record.status.as_str()
                    ),
                });
            }
            entry.mailbox.push_back(AgentMessage {
                sender_path: current_path.to_string(),
                message,
                trigger_turn: mode.trigger_turn(),
            });
            let followup_messages = if mode.trigger_turn() {
                entry.record.status = AgentStatus::Queued;
                clear_for_reactivation(&mut entry.record);
                entry.mailbox.drain(..).collect()
            } else {
                if !matches!(
                    entry.record.status,
                    AgentStatus::Queued | AgentStatus::Running
                ) {
                    entry.record.status = AgentStatus::Waiting;
                    entry.record.updated_at = super::snapshot::unix_seconds();
                }
                Vec::new()
            };
            let record = entry.record.clone();
            state.mark_activity();
            (record, followup_messages)
        };
        self.notify_activity();

        if mode == AgentMessageMode::TriggerTurn {
            let mut run_spec = trigger_run_spec
                .take()
                .expect("trigger run spec is reserved");
            let followup_message = followup_prompt(followup_messages);
            run_spec.message = followup_message.clone();
            run_spec.call_id = call_id.clone();
            self.start_agent_turn_with_guard(
                agent_id,
                run_spec,
                execution_guard.expect("trigger execution guard is reserved"),
            )
            .await;
            super::events::emit_agent_record(event_tx, &record);
            super::events::emit_subagent_activity(
                event_tx,
                call_id,
                Some(&record),
                SubAgentActivityKind::FollowupStarted,
                Some(followup_message),
                None,
                None,
            );
            return Ok(record);
        }

        super::events::emit_agent_record(event_tx, &record);
        super::events::emit_subagent_activity(
            event_tx,
            call_id,
            Some(&record),
            SubAgentActivityKind::MessageQueued,
            Some(activity_message),
            None,
            None,
        );
        Ok(record)
    }
}

fn followup_prompt(messages: Vec<AgentMessage>) -> String {
    let multiple = messages.len() > 1;
    let mut prompt = String::new();
    for (index, message) in messages.into_iter().enumerate() {
        if index > 0 {
            prompt.push_str("\n\n");
        }
        if multiple {
            if message.trigger_turn {
                prompt.push_str("Follow-up task");
            } else {
                prompt.push_str("Queued message from ");
                prompt.push_str(&message.sender_path);
            }
            prompt.push_str(":\n");
        }
        prompt.push_str(&message.message);
    }
    prompt
}
