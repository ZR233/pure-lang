use std::future::Future;
use std::path::PathBuf;

use pl_protocol::{PureError, TodoItem, TodoListSnapshot, TodoStatus};
use pl_trace::AgentEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::truncation::OutputTruncation;
use super::{StaticTool, ToolCallContext, ToolPolicy, ToolResult};
use crate::turn::ToolEffect;

pub const TOOL_UPDATE_TODO_LIST: &str = "update_todo_list";

#[derive(Debug, Clone)]
pub struct TodoListTool {
    working_set: crate::TurnWorkingSetHandle,
}

impl TodoListTool {
    pub fn new(working_set: crate::TurnWorkingSetHandle) -> Self {
        Self { working_set }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TodoListInput {
    /// Optional short title or explanation for this todo list update.
    #[serde(default)]
    explanation: Option<String>,
    /// The complete todo list snapshot.
    #[schemars(length(min = 1))]
    items: Vec<TodoListInputItem>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TodoListInputItem {
    /// Task step text.
    step: String,
    /// Step status.
    status: TodoListInputStatus,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum TodoListInputStatus {
    Pending,
    InProgress,
    Completed,
}

impl From<TodoListInputStatus> for TodoStatus {
    fn from(status: TodoListInputStatus) -> Self {
        match status {
            TodoListInputStatus::Pending => Self::Pending,
            TodoListInputStatus::InProgress => Self::InProgress,
            TodoListInputStatus::Completed => Self::Completed,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TodoListResult {
    status: String,
}

impl StaticTool for TodoListTool {
    type Input = TodoListInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin(TOOL_UPDATE_TODO_LIST),
            "Update the current task checklist. Submit the full todo list snapshot; each update appears as a new timeline entry.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::Read)
    }

    fn execute(
        &self,
        args: TodoListInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let snapshot = todo_list_snapshot(context.identity().item_id.clone(), args, &context)?;
            self.working_set
                .apply(crate::TurnWorkingSetChange::ReplaceTodo(snapshot.clone()))?;
            let _ = context
                .events()
                .send(AgentEvent::TodoListUpdated { snapshot });
            let description = serde_json::to_string(&TodoListResult {
                status: "updated".to_string(),
            })?;
            Ok(ToolResult::from_runtime_text(
                description,
                OutputTruncation::empty(),
                PathBuf::new(),
                Some(0),
                false,
                Vec::new(),
            ))
        }
    }
}

fn todo_list_snapshot(
    call_id: String,
    args: TodoListInput,
    context: &ToolCallContext,
) -> Result<TodoListSnapshot, PureError> {
    if args.items.is_empty() {
        return Err(invalid_todo_list("items must not be empty"));
    }
    let mut in_progress_count = 0;
    let mut items = Vec::with_capacity(args.items.len());
    for item in args.items {
        let step = item.step.trim().to_string();
        if step.is_empty() {
            return Err(invalid_todo_list("item step must not be empty"));
        }
        let status = TodoStatus::from(item.status);
        if status == TodoStatus::InProgress {
            in_progress_count += 1;
        }
        items.push(TodoItem { step, status });
    }
    if in_progress_count > 1 {
        return Err(invalid_todo_list("at most one item can be inProgress"));
    }

    Ok(TodoListSnapshot {
        call_id,
        agent_id: Some(context.identity().agent_id.clone()),
        path: Some(
            context
                .identity()
                .agent_path
                .clone()
                .unwrap_or_else(|| "/root".to_string()),
        ),
        parent_path: context.identity().parent_agent_id.clone(),
        explanation: args
            .explanation
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        items,
    })
}

fn invalid_todo_list(message: &str) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "update_todo_list".to_string(),
        error: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{StaticToolTestExt, ToolInput};

    fn context() -> (
        ToolCallContext,
        crate::TurnWorkingSetHandle,
        tokio::sync::broadcast::Receiver<AgentEvent>,
    ) {
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
        let working_set = crate::TurnWorkingSetHandle::default();
        (ToolCallContext::test(event_tx), working_set, event_rx)
    }

    fn input(arguments: serde_json::Value) -> ToolInput {
        ToolInput { arguments }
    }

    #[tokio::test]
    async fn emits_root_todo_snapshot() {
        let (context, working_set, mut event_rx) = context();

        let output = TodoListTool::new(working_set)
            .execute_raw(
                input(serde_json::json!({
                    "explanation": "Plan the pass",
                    "items": [
                        {"step": "Read code", "status": "completed"},
                        {"step": "Patch tool", "status": "inProgress"},
                        {"step": "Run tests", "status": "pending"}
                    ]
                })),
                context,
            )
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_str::<TodoListResult>(&output.canonical_output()).unwrap(),
            TodoListResult {
                status: "updated".to_string()
            }
        );
        let AgentEvent::TodoListUpdated { snapshot } = event_rx.recv().await.unwrap() else {
            panic!("expected todo list event");
        };
        assert_eq!(snapshot.call_id, "call-1");
        assert_eq!(snapshot.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(snapshot.path.as_deref(), Some("/root"));
        assert_eq!(snapshot.parent_path, None);
        assert_eq!(snapshot.explanation.as_deref(), Some("Plan the pass"));
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.items[1].status, TodoStatus::InProgress);
    }

    #[tokio::test]
    async fn emits_subagent_identity() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = ToolCallContext::new(
            crate::tool::ToolCallIdentity {
                call_id: "call-1".to_string(),
                item_id: "call-1".to_string(),
                agent_id: "agent-1".to_string(),
                parent_agent_id: Some("/root".to_string()),
                agent_path: Some("/root/explorer-1".to_string()),
                agent_role: "explorer".to_string(),
                agent_depth: 1,
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                step: 0,
            },
            event_tx,
        );
        let tool = TodoListTool::new(crate::TurnWorkingSetHandle::default());

        tool.execute_raw(
            input(serde_json::json!({
                "items": [{"step": "Inspect", "status": "pending"}]
            })),
            context,
        )
        .await
        .unwrap();

        let AgentEvent::TodoListUpdated { snapshot } = event_rx.recv().await.unwrap() else {
            panic!("expected todo list event");
        };
        assert_eq!(snapshot.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(snapshot.path.as_deref(), Some("/root/explorer-1"));
        assert_eq!(snapshot.parent_path.as_deref(), Some("/root"));
    }

    #[tokio::test]
    async fn rejects_invalid_snapshots() {
        let cases = [
            (serde_json::json!({"items": []}), "items must not be empty"),
            (
                serde_json::json!({"items": [{"step": " ", "status": "pending"}]}),
                "item step must not be empty",
            ),
            (
                serde_json::json!({
                    "items": [
                        {"step": "One", "status": "inProgress"},
                        {"step": "Two", "status": "inProgress"}
                    ]
                }),
                "at most one item can be inProgress",
            ),
        ];
        for (arguments, message) in cases {
            let (context, working_set, _event_rx) = context();
            let error = TodoListTool::new(working_set)
                .execute_raw(input(arguments), context)
                .await
                .unwrap_err();
            assert!(error.to_string().contains(message));
        }
    }
}
