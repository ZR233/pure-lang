//! Tool timeline facts use original call bytes and the context actually delivered by core.
use super::{ProjectionError, content::text_content};
use pl_core::thread::{
    AttemptOutcome, ThreadSnapshot, ToolDelivery, ToolOutcome, journal::ThreadCommit,
    permissions::PermissionState, task::TaskStatus,
};
use pl_protocol::{
    ThreadItem, ThreadItemState, ThreadToolInvocation, ThreadToolItem, ThreadToolOutput,
    ThreadToolState,
};
use std::{collections::BTreeMap, sync::Arc};

struct Call<'a> {
    turn_id: &'a str,
    call: &'a pl_core::model::ModelToolCall,
    sequence: u64,
    at: i64,
}

pub(in crate::studio) fn project_tools(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    journal: &[Arc<ThreadCommit>],
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut calls = BTreeMap::new();
    let mut updates = BTreeMap::new();
    for commit in journal
        .iter()
        .filter(|commit| commit.sequence <= snapshot.commit_sequence)
    {
        if let Some(attempt) = &commit.attempt
            && let AttemptOutcome::Committed(output) = &attempt.outcome
        {
            for call in &output.tool_calls {
                if calls
                    .insert(
                        call.call_id.as_str(),
                        Call {
                            turn_id: &attempt.turn_id,
                            call,
                            sequence: commit.sequence,
                            at: commit.committed_at,
                        },
                    )
                    .is_some()
                {
                    return Err(ProjectionError::DuplicateCall(call.call_id.clone()));
                }
            }
        }
        for task in commit.tasks.iter() {
            updates.insert(
                task.call_id.as_str(),
                (commit.sequence, commit.committed_at),
            );
        }
        for permission in commit.permissions.iter() {
            updates.insert(
                permission.call_id.as_str(),
                (commit.sequence, commit.committed_at),
            );
        }
        for delivery in commit.deliveries.iter() {
            updates.insert(
                delivery.call_id.as_str(),
                (commit.sequence, commit.committed_at),
            );
        }
    }
    let mut items = Vec::new();
    for (id, saved) in calls {
        let task = snapshot.tasks.values().find(|task| task.call_id == id);
        let delivery = snapshot
            .deliveries
            .iter()
            .find(|delivery| delivery.call_id == id);
        let (revision, at) = updates
            .get(id)
            .copied()
            .unwrap_or((saved.sequence, saved.at));
        let state = if let Some(delivery) = delivery {
            if delivery.tool_id != saved.call.tool_id {
                return Err(ProjectionError::DuplicateCall(id.into()));
            }
            terminal(delivery, at)?
        } else if let Some(task) = task {
            if task.status != TaskStatus::Running {
                return Err(ProjectionError::MissingToolResult(id.into()));
            }
            let progress = snapshot
                .tool_progress
                .get(&task.id)
                .map_or_else(String::new, |content| text_content(content));
            if task.cancel_requested {
                ThreadToolState::Cancelling(pl_protocol::CancellingThreadTool::new(progress))
            } else if snapshot.permissions.values().any(|permission| {
                permission.call_id == id && permission.state == PermissionState::Pending
            }) {
                ThreadToolState::AwaitingApproval(pl_protocol::AwaitingApprovalThreadTool)
            } else {
                ThreadToolState::Running(pl_protocol::RunningThreadTool::new(progress))
            }
        } else {
            ThreadToolState::Queued(pl_protocol::QueuedThreadTool)
        };
        if let Some(delivery) = delivery
            && delivery.tool_id == "skill_view"
            && matches!(delivery.outcome, ToolOutcome::Succeeded)
            && let Ok(Some(mut activation)) = pl_tool::skill::saved_skill_activation(
                delivery.output.payload(),
                saved.turn_id.into(),
                pl_protocol::SkillActivationCause::Tool {
                    tool_call_id: id.into(),
                },
            )
        {
            activation.activated_at = at;
            items.push(ThreadItem::new(
                super::order::skill_id(id),
                thread_id.into(),
                saved.turn_id.into(),
                revision,
                revision,
                at,
                at,
                ThreadItemState::Skill(pl_protocol::ThreadSkillItem::new(activation)),
            ));
        }
        let mut invocation = ThreadToolInvocation::new(
            id.into(),
            saved.call.tool_id.clone(),
            saved.call.arguments.content().into(),
        )
        .with_provider_identity(Some(id.into()), None);
        if let Some(task) = task {
            invocation = invocation.with_task_id(task.id.clone());
        }
        items.push(ThreadItem::new(
            super::order::tool_id(id),
            thread_id.into(),
            saved.turn_id.into(),
            saved.sequence,
            revision,
            saved.at,
            at,
            ThreadItemState::Tool(ThreadToolItem::new(invocation, state)),
        ));
    }
    Ok(items)
}

fn terminal(delivery: &ToolDelivery, at: i64) -> Result<ThreadToolState, ProjectionError> {
    let mut resources = Vec::new();
    for content in &delivery.delivered_context {
        match content {
            pl_core::context::ContextContent::Text { .. } => {}
            pl_core::context::ContextContent::Resource { reference } => {
                resources.push(serde_json::to_value(reference)?)
            }
            pl_core::context::ContextContent::Opaque { payload } => {
                resources.push(serde_json::to_value(payload)?)
            }
        }
    }
    let output = ThreadToolOutput::new(
        text_content(&delivery.delivered_context),
        Vec::new(),
        resources,
        command_exit_code(delivery),
    );
    Ok(match &delivery.outcome {
        ToolOutcome::Succeeded => {
            ThreadToolState::Succeeded(pl_protocol::SucceededThreadTool::new(at, output))
        }
        ToolOutcome::Failed(error) => ThreadToolState::Failed(pl_protocol::FailedThreadTool::new(
            at,
            pl_protocol::ThreadToolFailure::new(
                pl_protocol::ThreadToolFailureKind::Execution,
                error.to_string(),
            ),
            Some(output),
        )),
        ToolOutcome::Cancelled => {
            ThreadToolState::Cancelled(pl_protocol::CancelledThreadTool::new(
                at,
                "Tool execution was cancelled; observed facts remain in the journal.".into(),
            ))
        }
        ToolOutcome::Interrupted => {
            ThreadToolState::Interrupted(pl_protocol::InterruptedThreadTool::new(
                at,
                "Tool execution was interrupted; no side effect was replayed.".into(),
            ))
        }
    })
}

// Decode only the producer-owned versioned command receipt. The core outcome remains
// authoritative even when a process receipt reports a different exit status.
fn command_exit_code(delivery: &ToolDelivery) -> Option<i32> {
    let payload = delivery.output.payload();
    if !matches!(delivery.tool_id.as_str(), "exec" | "write_stdin")
        || payload.format() != "pl.tool.exec"
        || payload.version() != 1
    {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct CommandReceipt {
        state: pl_tool::command::CommandProcessLifecycle,
    }
    serde_json::from_str::<CommandReceipt>(payload.content())
        .ok()?
        .state
        .exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::{ContextContent, OpaquePayload},
        tool::ToolOutput,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn command_receipt_projects_exit_code_without_overriding_core_outcome() {
        let mut delivery = ToolDelivery {
            target: Default::default(), call_id: "command".into(), tool_id: "exec".into(),
            output: ToolOutput::new(OpaquePayload::new("pl.tool.exec", 1,
                r#"{"state":{"kind":"final","data":{"result":{"kind":"succeeded","data":{"exit_code":0}}}}}"#).unwrap(), Vec::new()),
            delivered_context: Vec::new(), outcome: ToolOutcome::Cancelled,
        };
        assert_eq!(command_exit_code(&delivery), Some(0));
        assert!(matches!(
            terminal(&delivery, 1).unwrap(),
            ThreadToolState::Cancelled(_)
        ));
        delivery.output = ToolOutput::new(
            OpaquePayload::new("pl.tool.exec", 1, "invalid receipt").unwrap(),
            Vec::new(),
        );
        assert_eq!(command_exit_code(&delivery), None);
        assert!(matches!(
            terminal(&delivery, 1).unwrap(),
            ThreadToolState::Cancelled(_)
        ));
    }

    #[test]
    fn tool_history_uses_saved_delivery_not_business_payload_or_new_renderer() {
        let delivery = ToolDelivery {
            target: Default::default(),
            call_id: "call".into(),
            tool_id: "custom".into(),
            output: ToolOutput::new(
                OpaquePayload::new("custom.result", 88, "{approved:true,exitCode:0}").unwrap(),
                vec![ContextContent::Text {
                    text: "original producer context".into(),
                }],
            ),
            delivered_context: vec![ContextContent::Text {
                text: "  actually delivered\r\n".into(),
            }],
            outcome: ToolOutcome::Succeeded,
        };
        let ThreadToolState::Succeeded(state) = terminal(&delivery, 42).unwrap() else {
            panic!("expected committed success")
        };
        assert_eq!(state.output().result(), "  actually delivered\r\n");
        assert_eq!(state.output().exit_code(), None);
        assert_eq!(
            delivery.output.payload().content(),
            "{approved:true,exitCode:0}"
        );
    }
}
