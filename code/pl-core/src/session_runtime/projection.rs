use pl_protocol::{
    ThreadItem, ThreadItemState, ThreadToolFailure, ThreadToolFailureKind, ThreadToolInvocation,
    ThreadToolItem, ThreadToolOutput, ThreadToolState,
};

use super::{ToolTaskSnapshot, ToolTaskStatus};

pub(crate) fn task_notifications(
    next: &super::SessionTasks,
    previous: &super::SessionTasks,
    current: &pl_protocol::ThreadSnapshot,
) -> crate::Result<Vec<crate::ThreadNotificationFact>> {
    next.execution_changes_since(previous)
        .map(|task| {
            let item = task_item(
                task,
                current
                    .items
                    .iter()
                    .find(|item| item.id == task.receipt.item_id),
            )?;
            let notification = if task.status.is_terminal() {
                pl_protocol::ThreadNotification::ItemCompleted {
                    item: Box::new(item),
                }
            } else {
                pl_protocol::ThreadNotification::ItemStarted {
                    item: Box::new(item),
                }
            };
            Ok(crate::ThreadNotificationFact::durable(
                task.updated_at,
                notification,
            ))
        })
        .collect()
}

fn task_item(task: &ToolTaskSnapshot, previous: Option<&ThreadItem>) -> crate::Result<ThreadItem> {
    build_task_item(task, previous, preview(previous))
}

pub(crate) fn task_output_item(
    task: &ToolTaskSnapshot,
    previous: Option<&ThreadItem>,
    delta: &str,
) -> crate::Result<ThreadItem> {
    let mut output = preview(previous);
    output.push_str(delta);
    if output.len() > 8192 {
        let mut start = output.len() - 8192;
        while !output.is_char_boundary(start) {
            start += 1;
        }
        output.drain(..start);
    }
    let mut item = build_task_item(task, previous, output)?;
    item.updated_at = crate::time::unix_seconds();
    Ok(item)
}

fn build_task_item(
    task: &ToolTaskSnapshot,
    previous: Option<&ThreadItem>,
    streamed: String,
) -> crate::Result<ThreadItem> {
    let receipt = &task.receipt;
    let invocation = match previous.map(ThreadItem::state) {
        Some(ThreadItemState::Tool(tool)) => tool.invocation().clone(),
        _ => ThreadToolInvocation::new(
            receipt.item_id.clone(),
            receipt.tool_name.clone(),
            String::new(),
        )
        .with_provider_identity(Some(receipt.call_id.clone()), None),
    }
    .with_task_id(receipt.task_id.clone());
    let state = match task.status {
        ToolTaskStatus::Queued => ThreadToolState::Queued(pl_protocol::QueuedThreadTool),
        ToolTaskStatus::WaitingApproval => {
            ThreadToolState::AwaitingApproval(pl_protocol::AwaitingApprovalThreadTool)
        }
        ToolTaskStatus::Running => {
            ThreadToolState::Running(pl_protocol::RunningThreadTool::new(streamed))
        }
        ToolTaskStatus::Cancelling => {
            ThreadToolState::Cancelling(pl_protocol::CancellingThreadTool::new(streamed))
        }
        ToolTaskStatus::Succeeded
        | ToolTaskStatus::Failed
        | ToolTaskStatus::Cancelled
        | ToolTaskStatus::Interrupted => {
            let result = task
                .result
                .as_ref()
                .ok_or_else(|| crate::PureError::Protocol("terminal task has no result".into()))?;
            let artifacts = super::task_artifacts(result);
            let output = ThreadToolOutput::new(
                result.output.clone(),
                result.attachments.clone(),
                artifacts,
                result.exit_code,
            );
            match task.status {
                ToolTaskStatus::Succeeded => ThreadToolState::Succeeded(
                    pl_protocol::SucceededThreadTool::new(task.updated_at, output),
                ),
                ToolTaskStatus::Failed => {
                    ThreadToolState::Failed(pl_protocol::FailedThreadTool::new(
                        task.updated_at,
                        ThreadToolFailure::new(
                            if result.timed_out {
                                ThreadToolFailureKind::TimedOut
                            } else {
                                ThreadToolFailureKind::Execution
                            },
                            result.output.clone(),
                        ),
                        Some(output),
                    ))
                }
                ToolTaskStatus::Cancelled => ThreadToolState::Cancelled(
                    pl_protocol::CancelledThreadTool::new(task.updated_at, result.output.clone()),
                ),
                ToolTaskStatus::Interrupted => ThreadToolState::Interrupted(
                    pl_protocol::InterruptedThreadTool::new(task.updated_at, result.output.clone()),
                ),
                ToolTaskStatus::Queued
                | ToolTaskStatus::WaitingApproval
                | ToolTaskStatus::Running
                | ToolTaskStatus::Cancelling => unreachable!("terminal branch"),
            }
        }
    };
    Ok(ThreadItem::new(
        receipt.item_id.clone(),
        receipt.thread_id.clone(),
        receipt.turn_id.clone(),
        previous.map_or(0, |item| item.ordinal),
        previous.map_or(0, |item| item.revision.saturating_add(1)),
        previous.map_or(task.created_at, |item| item.created_at),
        task.updated_at,
        ThreadItemState::Tool(ThreadToolItem::new(invocation, state)),
    ))
}

fn preview(previous: Option<&ThreadItem>) -> String {
    previous.map_or_else(String::new, |item| match item.state() {
        ThreadItemState::Tool(tool) => match tool.state() {
            ThreadToolState::Running(value) => value.streamed_output().to_owned(),
            ThreadToolState::Cancelling(value) => value.streamed_output().to_owned(),
            _ => String::new(),
        },
        _ => String::new(),
    })
}
