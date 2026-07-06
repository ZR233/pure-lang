use std::path::PathBuf;

use pl_protocol::{PureError, TodoItem, TodoListSnapshot, TodoStatus};
use pl_trace::AgentEvent;
use serde::{Deserialize, Serialize};

use super::truncation::OutputTruncation;
use super::{BoxFuture, Tool, ToolContext, ToolInput, ToolOutput};

#[derive(Debug, Default)]
pub struct TodoListTool;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TodoListInput {
    #[serde(default)]
    explanation: Option<String>,
    items: Vec<TodoListInputItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TodoListInputItem {
    step: String,
    status: TodoStatus,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TodoListResult {
    status: String,
}

impl Tool for TodoListTool {
    fn name(&self) -> &str {
        "update_todo_list"
    }

    fn description(&self) -> &str {
        "Update the current task checklist. Submit the full todo list snapshot; each update appears as a new timeline entry."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "explanation": {
                    "type": "string",
                    "description": "Optional short title or explanation for this todo list update."
                },
                "items": {
                    "type": "array",
                    "minItems": 1,
                    "description": "The complete todo list snapshot.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "step": {
                                "type": "string",
                                "description": "Task step text."
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "inProgress", "completed"],
                                "description": "Step status."
                            }
                        },
                        "required": ["step", "status"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["items"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: TodoListInput = serde_json::from_value(input.arguments).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("invalid input: {error}"),
                }
            })?;
            let snapshot = todo_list_snapshot(input.tool_id, args, &context)?;
            let _ = context
                .event_tx
                .send(AgentEvent::TodoListUpdated { snapshot });
            let description = serde_json::to_string(&TodoListResult {
                status: "updated".to_string(),
            })?;
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

fn todo_list_snapshot(
    call_id: String,
    args: TodoListInput,
    context: &ToolContext,
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
        if item.status == TodoStatus::InProgress {
            in_progress_count += 1;
        }
        items.push(TodoItem {
            step,
            status: item.status,
        });
    }
    if in_progress_count > 1 {
        return Err(invalid_todo_list("at most one item can be inProgress"));
    }

    let active = context.active_subagent.as_ref();
    Ok(TodoListSnapshot {
        call_id,
        agent_id: active.map(|agent| agent.id.clone()),
        path: Some(
            active
                .and_then(|agent| agent.agent_path.clone())
                .unwrap_or_else(|| crate::AgentPath::ROOT.to_string()),
        ),
        parent_path: active.and_then(|agent| agent.parent_id.clone()),
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
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{SubagentContext, WorkspaceAccess};
    use crate::{AgentSupervisor, CompileMode, CoreSession, TurnOptions};

    fn context() -> (ToolContext, tokio::sync::broadcast::Receiver<AgentEvent>) {
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(8);
        (
            ToolContext {
                event_tx,
                options: TurnOptions::default(),
                workspace_access: WorkspaceAccess::WorkspaceOnly,
                mode: CompileMode::Auto,
                workspace_root: std::env::temp_dir(),
                workspace_instructions: None,
                instruction_snapshot: None,
                provider_call_id: None,
                active_subagent: None,
                agent_supervisor: AgentSupervisor::default(),
                lsp_runtime: None,
                parent_session: Arc::new(CoreSession::new()),
            },
            event_rx,
        )
    }

    fn input(arguments: serde_json::Value) -> ToolInput {
        ToolInput {
            arguments,
            session_id: "session-1".to_string(),
            tool_id: "call-1".to_string(),
            revision_base: 0,
        }
    }

    #[tokio::test]
    async fn emits_root_todo_snapshot() {
        let (context, mut event_rx) = context();

        let output = TodoListTool
            .execute(
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
            serde_json::from_str::<TodoListResult>(&output.description).unwrap(),
            TodoListResult {
                status: "updated".to_string()
            }
        );
        let AgentEvent::TodoListUpdated { snapshot } = event_rx.recv().await.unwrap() else {
            panic!("expected todo list event");
        };
        assert_eq!(snapshot.call_id, "call-1");
        assert_eq!(snapshot.agent_id, None);
        assert_eq!(snapshot.path.as_deref(), Some(crate::AgentPath::ROOT));
        assert_eq!(snapshot.parent_path, None);
        assert_eq!(snapshot.explanation.as_deref(), Some("Plan the pass"));
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(snapshot.items[1].status, TodoStatus::InProgress);
    }

    #[tokio::test]
    async fn emits_subagent_identity() {
        let (mut context, mut event_rx) = context();
        context.active_subagent = Some(SubagentContext {
            id: "agent-1".to_string(),
            parent_id: Some(crate::AgentPath::ROOT.to_string()),
            agent_path: Some("/root/explorer-1".to_string()),
            role: "explorer".to_string(),
            task: "Explore".to_string(),
            depth: 1,
        });

        TodoListTool
            .execute(
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
        assert_eq!(
            snapshot.parent_path.as_deref(),
            Some(crate::AgentPath::ROOT)
        );
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
            let (context, _event_rx) = context();
            let error = TodoListTool
                .execute(input(arguments), context)
                .await
                .unwrap_err();
            assert!(error.to_string().contains(message));
        }
    }
}
