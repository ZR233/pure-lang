use pl_protocol::{PureError, TodoItem, TodoStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const TOOL_UPDATE_TODO_LIST: &str = "update_todo_list";

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
struct TodoDocument {
    explanation: Option<String>,
    items: Vec<TodoItem>,
}

/// Decodes a frozen checklist without re-running its update tool or changing the saved text.
///
/// # Errors
/// Rejects unsupported producer versions and malformed checklist data.
pub fn saved_snapshot(
    payload: &pl_core::context::OpaquePayload,
    call_id: String,
) -> Result<pl_protocol::TodoListSnapshot, PureError> {
    if payload.format() != "pl.tool.todo" || payload.version() != 1 {
        return Err(invalid_todo_list(
            "unsupported saved todo payload format or version",
        ));
    }
    let document: TodoDocument = serde_json::from_str(payload.content())?;
    Ok(pl_protocol::TodoListSnapshot {
        call_id,
        agent_id: None,
        path: None,
        parent_path: None,
        explanation: document.explanation,
        items: document.items,
    })
}

fn todo_document(args: TodoListInput) -> Result<TodoDocument, PureError> {
    if args.items.len() > 32 {
        return Err(invalid_todo_list("todo list may contain at most 32 items"));
    }
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

    if items.iter().any(|item| item.step.chars().count() > 256) {
        return Err(invalid_todo_list("todo item exceeds 256 characters"));
    }
    let document = TodoDocument {
        explanation: args
            .explanation
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        items,
    };
    if serde_json::to_vec(&document)?.len() > 8 * 1024 {
        return Err(invalid_todo_list("todo context exceeds 8192 bytes"));
    }
    Ok(document)
}

/// Stateless authoring tool; each Thread owns checklist state as an opaque extension.
#[derive(Debug, Default)]
pub struct TodoUpdateTool;

/// Builds the checklist tool for a Thread using host-encoded model declaration material.
///
/// # Errors
/// Propagates invalid registration identity.
pub fn registration(
    declaration: pl_core::context::OpaquePayload,
) -> Result<pl_core::tool::opaque::Registration, pl_core::tool::opaque::RegistryError> {
    Ok(pl_core::tool::opaque::Registration::new(
        TOOL_UPDATE_TODO_LIST.into(),
        declaration,
        TodoUpdateTool,
    )?
    .with_extension_updates())
}

impl pl_core::tool::opaque::Tool for TodoUpdateTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let decode = |source: PureError| pl_core::tool::opaque::ToolError::new(source);
        let input: TodoListInput =
            serde_json::from_str(input.content()).map_err(pl_core::tool::opaque::ToolError::new)?;
        let document = todo_document(input).map_err(decode)?;
        let encoded =
            serde_json::to_string(&document).map_err(pl_core::tool::opaque::ToolError::new)?;
        let payload = pl_core::context::OpaquePayload::new("pl.tool.todo", 1, encoded.clone())
            .map_err(pl_core::tool::opaque::ToolError::new)?;
        let previous = context.extensions.get("pl.tool.todo");
        if previous.is_some_and(|record| {
            record.payload.format() != "pl.tool.todo" || record.payload.version() != 1
        }) {
            return Err(decode(invalid_todo_list(
                "unsupported saved todo payload format or version",
            )));
        }
        let mutations = if previous.is_some_and(|record| record.payload == payload) {
            Vec::new()
        } else {
            vec![pl_core::thread::extensions::ExtensionMutation::Put {
                id: "pl.tool.todo".into(),
                expected_revision: previous.map(|record| record.revision),
                payload: payload.clone(),
            }]
        };
        Ok(pl_core::tool::ToolOutput::new(
            payload,
            vec![pl_core::context::ContextContent::Text {
                text: std::sync::Arc::from(encoded),
            }],
        )
        .with_extension_mutations(mutations))
    }
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
    #[test]
    fn rejects_invalid_snapshots() {
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
            let error = todo_document(serde_json::from_value(arguments).unwrap()).unwrap_err();
            assert!(error.to_string().contains(message));
        }
    }
    #[tokio::test]
    async fn dynamic_todo_uses_opaque_cas_without_rewriting_unchanged_state() {
        use pl_core::tool::opaque::Tool;
        let input = || {
            pl_core::context::OpaquePayload::new(
                "application/json",
                1,
                r#"{"items":[{"step":"Inspect","status":"inProgress"}]}"#,
            )
            .unwrap()
        };
        let mut context = pl_core::tool::opaque::CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Default::default(),
            extension_sequence: 0,
            catalog: std::sync::Arc::from([]),
        };
        let output = TodoUpdateTool
            .execute(input(), context.clone())
            .await
            .unwrap();
        assert_eq!(output.extension_mutations().len(), 1);
        assert_eq!(output.payload().format(), "pl.tool.todo");
        context.extensions = std::sync::Arc::new(std::collections::BTreeMap::from([(
            "pl.tool.todo".into(),
            pl_core::thread::extensions::ExtensionRecord {
                revision: 3,
                payload: output.payload().clone(),
            },
        )]));
        let repeated = TodoUpdateTool
            .execute(input(), context.clone())
            .await
            .unwrap();
        assert!(repeated.extension_mutations().is_empty());
        assert_eq!(repeated.payload(), output.payload());
        std::sync::Arc::make_mut(&mut context.extensions)
            .get_mut("pl.tool.todo")
            .unwrap()
            .payload =
            pl_core::context::OpaquePayload::new("plugin.future", 99, "unknown").unwrap();
        assert!(TodoUpdateTool.execute(input(), context).await.is_err());
    }
}
