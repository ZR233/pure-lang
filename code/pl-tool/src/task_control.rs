//! Model-callable task controls over the Thread's restricted task capability.
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{
        TaskAccess,
        task::{TaskRecord, TaskStatus},
    },
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

/// Task control appearance; scheduling and state transitions remain in core.
#[derive(Debug, Clone, Copy)]
pub enum TaskControlKind {
    Wait,
    Query,
    Cancel,
}

#[derive(Debug)]
pub struct TaskControlTool {
    kind: TaskControlKind,
}

impl TaskControlTool {
    /// Selects the task-control operation without creating runtime resources.
    pub fn new(kind: TaskControlKind) -> Self {
        Self { kind }
    }

    /// Registers only the permissions needed by this control.
    ///
    /// # Errors
    /// Returns invalid registration identity.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        let kind = self.kind;
        let id = match kind {
            TaskControlKind::Wait => "wait",
            TaskControlKind::Query => "get_tool_task",
            TaskControlKind::Cancel => "cancel_tool_task",
        };
        let registration = Registration::new(id.into(), declaration, self)?;
        Ok(match kind {
            TaskControlKind::Wait => registration.with_task_waiting(),
            TaskControlKind::Cancel => registration.with_task_cancellation(),
            TaskControlKind::Query => registration,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitTasksInput {
    /// Task IDs to observe. An empty list waits for Thread messages.
    #[serde(default)]
    pub task_ids: Vec<String>,
    /// Maximum wait in milliseconds; defaults to sixty seconds.
    #[serde(default)]
    #[schemars(range(min = 1, max = 300000))]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryTaskInput {
    pub task_id: String,
    /// UTF-8 byte offset into the immutable complete payload; omitted reads status only.
    #[serde(default)]
    pub result_cursor: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelTaskInput {
    pub task_id: String,
}

#[derive(Debug, thiserror::Error)]
enum ControlError {
    #[error("task controls require a Thread execution context")]
    MissingRuntime,
    #[error("wait timeout must be between 1 and 300000 milliseconds")]
    Timeout,
    #[error("result cursor is outside the payload or splits a UTF-8 character")]
    Cursor,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status<'a> {
    task_id: &'a str,
    tool_id: &'a str,
    status: TaskStatus,
    cancel_requested: bool,
}
impl<'a> From<&'a TaskRecord> for Status<'a> {
    fn from(task: &'a TaskRecord) -> Self {
        Self {
            task_id: &task.id,
            tool_id: &task.tool_id,
            status: task.status,
            cancel_requested: task.cancel_requested,
        }
    }
}

impl Tool for TaskControlTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let access = context
            .tasks
            .as_ref()
            .ok_or_else(|| ToolError::new(ControlError::MissingRuntime))?;
        match self.kind {
            TaskControlKind::Wait => {
                let input: WaitTasksInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let timeout = input.timeout_ms.unwrap_or(60_000);
                if !(1..=300_000).contains(&timeout) {
                    return Err(ToolError::new(ControlError::Timeout));
                }
                let result = tokio::time::timeout(
                    Duration::from_millis(timeout),
                    access.wait(&input.task_ids, context.cancellation.clone()),
                )
                .await;
                let (tasks, messages_ready, timed_out) = match result {
                    Ok(result) => {
                        let result = result.map_err(ToolError::new)?;
                        (result.tasks, result.messages_ready, false)
                    }
                    Err(_) => (
                        input
                            .task_ids
                            .iter()
                            .map(|id| access.get(id))
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(ToolError::new)?,
                        false,
                        true,
                    ),
                };
                let view = serde_json::json!({"tasks":tasks.iter().map(Status::from).collect::<Vec<_>>(),"messagesReady":messages_ready,"timedOut":timed_out});
                let payload = serde_json::json!({"tasks":tasks,"messagesReady":messages_ready,"timedOut":timed_out});
                output("pl.tool.wait", &payload, &view)
            }
            TaskControlKind::Query => {
                let input: QueryTaskInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                query(access, input)
            }
            TaskControlKind::Cancel => {
                let input: CancelTaskInput =
                    serde_json::from_str(input.content()).map_err(ToolError::new)?;
                let receipt = access
                    .cancel(input.task_id.clone())
                    .await
                    .map_err(ToolError::new)?;
                let view = serde_json::json!({"taskId":input.task_id,"cancellation":receipt});
                output("pl.tool.cancel-task", &view, &view)
            }
        }
    }
}

fn query(access: &TaskAccess, input: QueryTaskInput) -> Result<ToolOutput, ToolError> {
    let task = access.get(&input.task_id).map_err(ToolError::new)?;
    let mut view = serde_json::json!({"task":Status::from(&task)});
    if let Some(start) = input.result_cursor
        && let Some(delivery) = access.result(&input.task_id).map_err(ToolError::new)?
    {
        let payload = delivery.output.payload();
        let text = payload.content();
        if start > text.len() || !text.is_char_boundary(start) {
            return Err(ToolError::new(ControlError::Cursor));
        }
        let mut end = start.saturating_add(8192).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        view["result"] = serde_json::json!({"format":payload.format(),"version":payload.version(),"content":&text[start..end],"nextCursor":(end<text.len()).then_some(end),"totalBytes":text.len()});
    }
    output(
        "pl.tool.task",
        &serde_json::json!({"task":task,"result":view.get("result")}),
        &view,
    )
}

fn output(
    format: &str,
    payload: &impl Serialize,
    view: &impl Serialize,
) -> Result<ToolOutput, ToolError> {
    let payload = OpaquePayload::new(
        format,
        1,
        serde_json::to_string(payload).map_err(ToolError::new)?,
    )
    .map_err(ToolError::new)?;
    let text = serde_json::to_string(view).map_err(ToolError::new)?;
    Ok(ToolOutput::new(
        payload,
        vec![ContextContent::Text {
            text: Arc::from(text),
        }],
    ))
}

/// Lists this Thread's tasks without consuming messages or task results.
#[derive(Debug, Default)]
pub struct ListTasksTool;

impl ListTasksTool {
    /// Stable declaration for recovering task identities after a model context change.
    pub fn declaration() -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            "list_tool_tasks",
            "List tasks owned by this Thread without consuming results; use get_tool_task for complete-result pages.",
            schemars::schema_for!(ListTasksInput).to_value(),
        )
    }

    /// Registers the read-only task view without waiting or cancellation authority.
    ///
    /// # Errors
    /// Returns an invalid registration identity.
    pub fn registration(self, declaration: OpaquePayload) -> Result<Registration, RegistryError> {
        Registration::new("list_tool_tasks".into(), declaration, self)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListTasksInput {}

impl Tool for ListTasksTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let _: ListTasksInput = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let access = context
            .tasks
            .as_ref()
            .ok_or_else(|| ToolError::new(ControlError::MissingRuntime))?;
        let tasks = access.list();
        let view = tasks.iter().map(Status::from).collect::<Vec<_>>();
        output("pl.tool.list-tasks", &tasks, &view)
    }
}
