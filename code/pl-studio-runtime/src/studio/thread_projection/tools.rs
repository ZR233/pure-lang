//! Tool timeline facts use original call bytes and the context actually delivered by core.
use super::{ProjectionError, content::text_content};
use pl_core::thread::{
    ThreadSnapshot, ToolDelivery, ToolOutcome, permissions::PermissionState, task::TaskStatus,
};
use pl_protocol::{
    ThreadItem, ThreadItemState, ThreadToolInvocation, ThreadToolItem, ThreadToolOutput,
    ThreadToolState,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn project_tool_call(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    turn_id: &str,
    call: &pl_core::model::ModelToolCall,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let id = call.call_id.as_str();
    let invocation = ThreadToolInvocation::new(
        id.into(),
        call.tool_id.clone(),
        call.arguments.content().into(),
    )
    .with_provider_identity(Some(id.into()), None);
    project_invocation(
        thread_id, snapshot, turn_id, invocation, ordinal, created_at, revision, updated_at,
    )
}

pub(super) fn project_saved_tool_call(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    saved: &ThreadItem,
    revision: u64,
    updated_at: i64,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let ThreadItemState::Tool(tool) = saved.state() else {
        return Err(ProjectionError::DuplicateCall(saved.id.clone()));
    };
    let invocation = tool.invocation().clone();
    let id = invocation.tool_call_id();
    if saved.id != super::order::tool_id(id) || saved.thread_id != thread_id {
        return Err(ProjectionError::DuplicateCall(id.into()));
    }
    project_invocation(
        thread_id,
        snapshot,
        &saved.turn_id,
        invocation,
        saved.ordinal,
        saved.created_at,
        revision,
        updated_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn project_invocation(
    thread_id: &str,
    snapshot: &ThreadSnapshot,
    turn_id: &str,
    mut invocation: ThreadToolInvocation,
    ordinal: u64,
    created_at: i64,
    revision: u64,
    updated_at: i64,
) -> Result<Vec<ThreadItem>, ProjectionError> {
    let mut items = Vec::new();
    let id = invocation.tool_call_id().to_owned();
    let task = snapshot.tasks.values().find(|task| task.call_id == id);
    let delivery = snapshot
        .deliveries
        .iter()
        .find(|delivery| delivery.call_id == id);
    let state = if let Some(delivery) = delivery {
        if delivery.tool_id != invocation.name() {
            return Err(ProjectionError::DuplicateCall(id));
        }
        terminal(delivery, updated_at)?
    } else if let Some(task) = task {
        if task.status != TaskStatus::Running {
            return Err(ProjectionError::MissingToolResult(id));
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
            turn_id.into(),
            pl_protocol::SkillActivationCause::Tool {
                tool_call_id: id.clone(),
            },
        )
    {
        activation.activated_at = updated_at;
        items.push(ThreadItem::new(
            super::order::skill_id(&id),
            thread_id.into(),
            turn_id.into(),
            revision,
            revision,
            updated_at,
            updated_at,
            ThreadItemState::Skill(pl_protocol::ThreadSkillItem::new(activation)),
        ));
    }
    if let Some(task) = task {
        invocation = invocation.with_task_id(task.id.clone());
    }
    items.push(ThreadItem::new(
        super::order::tool_id(&id),
        thread_id.into(),
        turn_id.into(),
        ordinal,
        revision,
        created_at,
        updated_at,
        ThreadItemState::Tool(ThreadToolItem::new(invocation, state)),
    ));
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
        super::delivery_attachments(delivery)?,
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
