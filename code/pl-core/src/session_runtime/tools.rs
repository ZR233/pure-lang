use std::future::Future;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::tool::ToolBudgetTiming;
use crate::{
    DynTool, StaticTool, StaticToolDefinition, ToolBatchPolicy, ToolCallContext, ToolDirective,
    ToolEffect, ToolName, ToolPolicy, ToolResult,
};

use super::{SessionWaitResult, SessionWakeEvent, ToolTaskStatus};

const WAIT_OUTPUT_BYTES: usize = 128 * 1024;

/// The single model-facing suspension point for all registered event sources.
#[derive(Debug, Default)]
pub struct WaitTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitInput {
    /// Optional wait limit in milliseconds; omitting it waits until an event or cancellation.
    #[schemars(range(min = 1, max = 3600000))]
    timeout_ms: Option<u64>,
}

impl StaticTool for WaitTool {
    type Input = WaitInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("wait"),
            "Suspend the current turn until any registered session event arrives: tool results, timers, agent events, or application messages. Previously queued events return immediately. This must be the only tool call in the response. A timeout does not cancel tasks.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::control()
            .with_effect(ToolEffect::Read)
            .with_batch_policy(ToolBatchPolicy::Solo)
            .with_budget_timing(ToolBudgetTiming::PauseWhenOnlyScheduledTool)
    }

    fn execute(
        &self,
        input: WaitInput,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            if input
                .timeout_ms
                .is_some_and(|timeout| !(1..=3_600_000).contains(&timeout))
            {
                return Err(tool_error(
                    "wait",
                    "timeoutMs must be between 1 and 3600000",
                ));
            }
            let session = context.session_control()?;
            let waiting = async {
                match input.timeout_ms {
                    Some(timeout) => {
                        match tokio::time::timeout(Duration::from_millis(timeout), session.wait())
                            .await
                        {
                            Ok(result) => result.map(SessionWaitResult::Events),
                            Err(_) => Ok(SessionWaitResult::TimedOut),
                        }
                    }
                    None => session.wait().await.map(SessionWaitResult::Events),
                }
            };
            let result = match context.cancellation_token() {
                Some(token) => tokio::select! {
                    result = waiting => result.map_err(|error| tool_error("wait", &error.to_string()))?,
                    _ = token.cancelled() => return Err(tool_error("wait", "Current turn cancelled; events remain pending")),
                },
                None => waiting
                    .await
                    .map_err(|error| tool_error("wait", &error.to_string()))?,
            };
            let mut output = ToolResult::json_with_budget(
                super::model_view::wait_result(&result)
                    .map_err(|error| tool_error("wait", &error.to_string()))?,
                WAIT_OUTPUT_BYTES / 4,
                WAIT_OUTPUT_BYTES,
            )?;
            if let SessionWaitResult::Events(batch) = result {
                for envelope in &batch.events {
                    if let SessionWakeEvent::ToolFinished(task) = &envelope.event
                        && let Some(result) = &task.result
                    {
                        output
                            .model_attachments
                            .extend(result.attachments.iter().cloned());
                        if task.status == ToolTaskStatus::Succeeded {
                            let complete;
                            let result = if task
                                .result_reference
                                .as_ref()
                                .is_some_and(|reference| !reference.preview_complete)
                            {
                                complete = session
                                    .read_complete(task.receipt.task_id.clone())
                                    .await
                                    .map_err(|error| tool_error("wait", &error.to_string()))?;
                                complete.result.as_ref().ok_or_else(|| {
                                    tool_error("wait", "complete result is missing")
                                })?
                            } else {
                                result
                            };
                            output.runtime_events.extend(
                                result
                                    .skill_activations
                                    .iter()
                                    .cloned()
                                    .map(|activation| ToolDirective::SkillActivated { activation }),
                            );
                        }
                    }
                }
                output
                    .runtime_events
                    .push(ToolDirective::SessionEvents { batch });
            }
            Ok(output)
        }
    }
}

/// Lists all current session task handles, with optional status and pagination.
#[derive(Debug, Default)]
pub struct ListToolTasksTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListToolTasksInput {
    /// Omit to list every nonterminal task, including tasks started in previous turns.
    status: Option<TaskStatusInput>,
    /// Exclusive cursor returned by the previous page.
    cursor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum TaskStatusInput {
    Queued,
    WaitingApproval,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl From<TaskStatusInput> for ToolTaskStatus {
    fn from(status: TaskStatusInput) -> Self {
        match status {
            TaskStatusInput::Queued => Self::Queued,
            TaskStatusInput::WaitingApproval => Self::WaitingApproval,
            TaskStatusInput::Running => Self::Running,
            TaskStatusInput::Cancelling => Self::Cancelling,
            TaskStatusInput::Succeeded => Self::Succeeded,
            TaskStatusInput::Failed => Self::Failed,
            TaskStatusInput::Cancelled => Self::Cancelled,
            TaskStatusInput::Interrupted => Self::Interrupted,
        }
    }
}

impl StaticTool for ListToolTasksTool {
    type Input = ListToolTasksInput;
    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("list_tool_tasks"),
            "List session-owned task handles and states. By default returns all nonterminal tasks, including earlier turns. Does not consume events; use wait when idle.",
        )
    }
    fn policy(&self) -> ToolPolicy {
        ToolPolicy::control().with_effect(ToolEffect::Read)
    }
    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let result = context
                .session_control()?
                .list(input.status.map(Into::into), input.cursor)
                .await
                .map_err(|error| tool_error("list_tool_tasks", &error.to_string()))?;
            ToolResult::json_with_budget(result, WAIT_OUTPUT_BYTES / 4, WAIT_OUTPUT_BYTES)
        }
    }
}

/// Reads a task's canonical state and final result without consuming its wake event.
#[derive(Debug, Default)]
pub struct GetToolTaskTool;

/// Cooperatively cancels one task without ending the current Turn.
#[derive(Debug, Default)]
pub struct CancelToolTaskTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolTaskInput {
    /// Session task ID from a tool receipt or list_tool_tasks.
    task_id: String,
}

/// Task inspection, optionally reading one segment of its complete result.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetToolTaskInput {
    /// Session task ID from a tool receipt or list_tool_tasks.
    task_id: String,
    /// Opaque cursor from resultReference.cursor or a previous result page.
    result_cursor: Option<String>,
}

impl StaticTool for GetToolTaskTool {
    type Input = GetToolTaskInput;
    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("get_tool_task"),
            "Read a session task without consuming events. resultReference links to the complete result: pass its cursor as resultCursor, then follow nextCursor. Pages are UTF-8 segments of result JSON. Use wait rather than polling unfinished tasks.",
        )
    }
    fn policy(&self) -> ToolPolicy {
        ToolPolicy::control().with_effect(ToolEffect::Read)
    }
    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let session = context.session_control()?;
            if let Some(cursor) = input.result_cursor {
                let task = session
                    .read_complete(input.task_id)
                    .await
                    .map_err(|error| tool_error("get_tool_task", &error.to_string()))?;
                let page = super::model_view::result_page(&task, &cursor)
                    .map_err(|error| tool_error("get_tool_task", &error.to_string()))?;
                return ToolResult::json_with_budget(
                    page,
                    WAIT_OUTPUT_BYTES / 4,
                    WAIT_OUTPUT_BYTES,
                );
            }
            let task = session
                .get(input.task_id)
                .await
                .map_err(|error| tool_error("get_tool_task", &error.to_string()))?;
            let mut output =
                ToolResult::json_with_budget(&task, WAIT_OUTPUT_BYTES / 4, WAIT_OUTPUT_BYTES)?;
            if let Some(result) = task.result {
                output.model_attachments = result.attachments;
            }
            Ok(output)
        }
    }
}

impl StaticTool for CancelToolTaskTool {
    type Input = ToolTaskInput;
    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("cancel_tool_task"),
            "Request cancellation of a session task. Cancelling is not completed cleanup; use wait for the terminal event. Already completed tasks remain unchanged.",
        )
    }
    fn policy(&self) -> ToolPolicy {
        ToolPolicy::control().with_effect(ToolEffect::AgentControl)
    }
    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            let task = context
                .session_control()?
                .cancel(input.task_id)
                .await
                .map_err(|error| tool_error("cancel_tool_task", &error.to_string()))?;
            ToolResult::json_with_budget(
                super::model_view::task(&task)
                    .map_err(|error| tool_error("cancel_tool_task", &error.to_string()))?,
                WAIT_OUTPUT_BYTES / 4,
                WAIT_OUTPUT_BYTES,
            )
        }
    }
}

/// An independently scheduled timer, subject to the same session task lifecycle.
#[derive(Debug, Default)]
pub struct SleepTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SleepInput {
    /// Delay in milliseconds before publishing task completion.
    #[schemars(range(min = 1, max = 86400000))]
    duration_ms: u64,
}

impl StaticTool for SleepTool {
    type Input = SleepInput;
    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("sleep"),
            "Schedule a session timer. Short timers return their result directly; longer timers return a task receipt. Use wait for background completion alongside other session events.",
        )
    }
    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only().with_runtime_lock_policy(crate::tool::ToolRuntimeLockPolicy::None)
    }
    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = crate::Result<ToolResult>> + Send {
        async move {
            if !(1..=86_400_000).contains(&input.duration_ms) {
                return Err(tool_error(
                    "sleep",
                    "durationMs must be between 1 and 86400000",
                ));
            }
            let delay = tokio::time::sleep(Duration::from_millis(input.duration_ms));
            match context.cancellation_token() {
                Some(token) => tokio::select! {
                    _ = delay => {},
                    _ = token.cancelled() => return Err(tool_error("sleep", "Timer cancelled")),
                },
                None => delay.await,
            }
            Ok(ToolResult::success("Timer elapsed."))
        }
    }
}

/// Constructs the standard session task controls and timer using ordinary DynTool values.
pub fn session_control_tools() -> Vec<DynTool> {
    vec![
        WaitTool.into(),
        ListToolTasksTool.into(),
        GetToolTaskTool.into(),
        CancelToolTaskTool.into(),
        SleepTool.into(),
    ]
}

fn tool_error(tool: &str, error: &str) -> crate::PureError {
    crate::PureError::ToolExecutionFailed {
        tool: tool.to_owned(),
        error: error.to_owned(),
    }
}
