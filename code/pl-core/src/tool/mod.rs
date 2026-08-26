mod ask_user;
pub mod cache;
mod command;
mod container;
mod context;
mod contract;
mod exec;
mod file;
mod git;
mod local;
mod lsp;
mod manager;
mod model_output;
pub mod output_format;
mod path_policy;
mod plan;
mod programmatic;
mod session_note;
mod shell;
mod skill;
mod text_document;
mod text_escape;
mod todo;
mod tool_result;
mod truncation;
mod web_search;
mod workspace_file;

pub use ask_user::*;
pub use command::*;
pub use container::*;
pub use context::*;
pub use contract::*;
pub use exec::*;
pub use file::*;
pub use futures::future::BoxFuture;
pub use git::*;
pub use local::*;
pub use lsp::*;
pub use manager::*;
pub use model_output::*;
pub use path_policy::*;
pub use plan::*;
pub use programmatic::*;
pub use session_note::*;
pub use shell::*;
pub use skill::*;
pub use todo::*;
pub use tool_result::*;
pub use truncation::*;
pub use web_search::*;
pub use workspace_file::*;

pub(crate) fn estimate_tool_schema_tokens(schemas: &[pl_protocol::ToolSpec]) -> u64 {
    let bytes = serde_json::to_vec(schemas).map_or(0, |value| value.len() as u64);
    bytes.saturating_add(3) / 4
}

pub(crate) fn estimate_tool_result_tokens<'a>(results: impl IntoIterator<Item = &'a str>) -> u64 {
    let bytes = results.into_iter().fold(0_u64, |total, result| {
        total.saturating_add(result.len() as u64)
    });
    bytes.saturating_add(3) / 4
}

pub(crate) fn tool_error(tool: &str, error: impl std::fmt::Display) -> pl_protocol::PureError {
    pl_protocol::PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::sync::Arc;

    use super::*;
    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;
    use schemars::JsonSchema;
    use serde::Deserialize;

    #[test]
    fn tool_result_keeps_canonical_content_and_typed_directives() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        let mut output = ToolResult::failure("model output");
        output.runtime_events.extend([
            ToolDirective::OutputArtifacts {
                artifacts: vec![serde_json::json!({"id": "artifact-1"})],
            },
            ToolDirective::EndTurn {
                final_content: Some("final answer".to_string()),
            },
        ]);

        assert!(!output.success);
        assert_eq!(output.canonical_output(), "model output");
        assert_eq!(output.model_output(), "model output");
        assert!(output.ends_turn());
        assert_eq!(output.end_turn_content(), Some("final answer"));
        assert_eq!(
            output.output_artifacts_as::<ArtifactRecord>(),
            vec![ArtifactRecord {
                id: "artifact-1".to_string(),
            }]
        );
    }

    #[test]
    fn typed_tool_flattens_components_and_rejects_unknown_fields() {
        #[derive(Debug, Deserialize, JsonSchema)]
        struct EmptyInput {}

        #[derive(Debug, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct PaginationInput {
            limit: Option<usize>,
            cursor: Option<String>,
        }

        #[derive(Debug, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase")]
        struct SearchInput {
            query: String,
            #[serde(flatten)]
            pagination: PaginationInput,
        }

        let definition = TypedTool::<SearchInput>::new("search_product", "Search product records.");
        let input_schema = definition.input_schema();

        assert_eq!(input_schema["type"], "object");
        assert_eq!(input_schema["additionalProperties"], false);
        assert!(input_schema.get("$schema").is_none());
        assert!(input_schema.get("title").is_none());
        for field in ["query", "limit", "cursor"] {
            assert!(input_schema["properties"].get(field).is_some());
        }

        let input = deserialize_tool_input::<SearchInput>(
            "search_product",
            serde_json::json!({"query": "rust", "limit": 20}),
        )
        .expect("flattened input");
        assert_eq!(input.query, "rust");
        assert_eq!(input.pagination.limit, Some(20));
        assert_eq!(input.pagination.cursor, None);

        let error = deserialize_tool_input::<SearchInput>(
            "search_product",
            serde_json::json!({"query": "rust", "page": 2}),
        )
        .expect_err("unknown flattened field");
        assert!(error.to_string().contains("unknown field `page`"));

        let empty_schema = typed_tool_input_schema::<EmptyInput>();
        assert_eq!(empty_schema["additionalProperties"], false);
        let error = deserialize_tool_input::<EmptyInput>(
            "empty_product",
            serde_json::json!({"unexpected": true}),
        )
        .expect_err("unknown field on empty input");
        assert!(error.to_string().contains("expected no fields"));
    }

    #[test]
    fn typed_tool_normalizes_tagged_object_unions() {
        #[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        struct CreateInput {
            name: String,
        }

        #[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
        #[serde(deny_unknown_fields)]
        struct DeleteInput {
            name: String,
        }

        #[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
        #[serde(rename_all = "camelCase", tag = "action")]
        enum ManageInput {
            Create(CreateInput),
            Delete(DeleteInput),
        }

        let schema = typed_tool_input_schema::<ManageInput>();

        assert_eq!(schema["type"], "object");
        assert!(schema["oneOf"].is_array());
        assert!(schema.get("additionalProperties").is_none());
        assert_eq!(
            deserialize_tool_input::<ManageInput>(
                "manage_product",
                serde_json::json!({"action": "create", "name": "local"}),
            )
            .expect("tagged object union input"),
            ManageInput::Create(CreateInput {
                name: "local".to_string(),
            })
        );
        let error = deserialize_tool_input::<ManageInput>(
            "manage_product",
            serde_json::json!({
                "action": "delete",
                "name": "local",
                "unexpected": true,
            }),
        )
        .expect_err("unknown tagged variant field");
        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[tokio::test]
    async fn local_tool_honors_cancelled_context() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool_calls = calls.clone();
        let tool = LocalTool::new(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            move |_input, _context| {
                let tool_calls = tool_calls.clone();
                async move {
                    tool_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ToolResult::success("ran"))
                }
            },
        );
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                ToolCallContext::test(event_tx).with_cancellation(Some(token)),
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool" && error.contains("cancel")
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn typed_function_tool_maps_display_error() {
        #[derive(Debug, Deserialize, JsonSchema)]
        struct EmptyInput {}

        #[derive(Debug)]
        struct DisplayError(&'static str);

        impl fmt::Display for DisplayError {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.0)
            }
        }

        let tool = TypedTool::<EmptyInput>::new("product_tool", "Product tool").handler(
            |_input, _context| async move { Err::<ToolResult, DisplayError>(DisplayError("boom")) },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                ToolCallContext::test(event_tx),
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool" && error == "boom"
        ));
    }

    #[tokio::test]
    async fn tool_backend_future_returns_cancelled_error_before_running() {
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();

        let result = run_tool_backend_with_cancellation(
            async { Ok::<_, &'static str>("ran") },
            Some(token),
            || "cancelled",
        )
        .await;

        assert_eq!(result, Err("cancelled"));
    }

    #[tokio::test]
    async fn tool_backend_future_returns_cancelled_error_while_running() {
        let token = tokio_util::sync::CancellationToken::new();
        let task_token = token.clone();
        let task = tokio::spawn(async move {
            run_tool_backend_with_cancellation(
                async {
                    std::future::pending::<()>().await;
                    Ok::<_, &'static str>("ran")
                },
                Some(task_token),
                || "cancelled",
            )
            .await
        });

        token.cancel();

        assert_eq!(task.await.expect("task joins"), Err("cancelled"));
    }

    #[test]
    fn model_visible_tool_output_truncates_json_with_codex_shape() {
        let long_stdout = "x".repeat(65);
        let output = model_visible_tool_output_with_tokens(
            &serde_json::json!({ "status": 0, "stdout": long_stdout, "stderr": "" }).to_string(),
            8,
        );
        let value = serde_json::from_str::<serde_json::Value>(&output).unwrap();

        assert_eq!(value["truncated"], true);
        assert!(value.pointer("/bytesReturned").is_some());
        assert!(value.pointer("/bytesOmitted").is_some());
        assert!(value.pointer("/nextOffset").is_some());
        assert!(value.pointer("/bytes_returned").is_none());
        let visible = value
            .get("stdout")
            .or_else(|| value.get("jsonPreview"))
            .and_then(serde_json::Value::as_str)
            .expect("visible output");
        assert!(visible.len() <= 32);
    }

    #[test]
    fn model_visible_tool_output_keeps_json_array_items_structured() {
        let output = model_visible_tool_output_with_tokens(
            &serde_json::to_string(&vec![
                serde_json::json!({"id": 1, "state": "completed"}),
                serde_json::json!({"id": 2, "payload": "x".repeat(200)}),
                serde_json::json!({"id": 3, "state": "queued"}),
            ])
            .unwrap(),
            24,
        );
        let value = serde_json::from_str::<serde_json::Value>(&output).unwrap();

        assert!(value["items"].is_array());
        assert!(value["itemsReturned"].as_u64().is_some());
        assert!(value["itemsOmitted"].as_u64().is_some());
    }

    #[test]
    fn trace_preview_redacts_sensitive_values() {
        let value = serde_json::json!({
            "token": "secret-token",
            "nested": { "api_key": "secret-key", "normal": "visible" },
            "payload": "YWJj".repeat(90),
        });
        let preview = output_format::redaction::trace_preview_value(&value, 1_000);

        assert!(preview.contains("<redacted>"));
        assert!(preview.contains("visible"));
        assert!(!preview.contains("secret-token"));
        assert!(!preview.contains("secret-key"));
        assert!(!preview.contains(&"YWJj".repeat(30)));
    }

    #[test]
    fn explicit_secret_redaction_handles_text_and_json() {
        let redaction =
            output_format::redaction::SecretRedaction::new(["secret", "secret-token", ""]);

        assert_eq!(
            redaction.redact_str("secret-token and secret"),
            "<redacted> and <redacted>"
        );
        assert_eq!(
            redaction.redact_json_value(serde_json::json!({
                "secret-token": "visible",
                "items": ["secret-token", { "value": "secret" }],
            })),
            serde_json::json!({
                "<redacted>": "visible",
                "items": ["<redacted>", { "value": "<redacted>" }],
            })
        );
    }

    #[tokio::test]
    async fn workspace_write_lock_is_shared_for_same_workspace() {
        let root = std::env::temp_dir().join(format!(
            "pure-lang-write-lock-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let workspace = ToolWorkspace::new(AgentWorkspace::local(root.clone()));
        let first_guard = workspace.write_lock().await;
        let second_workspace = workspace.clone();
        let second = tokio::spawn(async move { second_workspace.write_lock().await });
        tokio::task::yield_now().await;

        assert!(!second.is_finished());
        drop(first_guard);
        let second_guard = second.await.unwrap();
        drop(second_guard);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
