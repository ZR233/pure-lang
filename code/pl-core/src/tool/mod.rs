mod ask_user;
pub mod cache;
mod command;
mod container;
mod context;
mod contract;
mod exec;
mod file;
mod git;
mod lsp;
mod model_output;
mod orchestration;
pub mod output_format;
mod path_policy;
mod plan;
mod registered;
mod registry;
mod session_note;
mod shell;
mod skill;
mod text_document;
mod text_escape;
mod todo;
mod tool_output;
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
pub use lsp::*;
pub use model_output::*;
pub use orchestration::*;
pub use path_policy::*;
pub use plan::*;
pub use registered::*;
pub use registry::*;
pub use session_note::*;
pub use shell::*;
pub use skill::*;
pub use todo::*;
pub use tool_output::*;
pub use truncation::*;
pub use web_search::*;
pub use workspace_file::*;

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::turn::{ToolEffect, TurnOptions};
    use futures::FutureExt;
    use futures::future::BoxFuture;
    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;
    use schemars::JsonSchema;
    use serde::Deserialize;

    fn empty_truncation() -> OutputTruncation {
        OutputTruncation::empty()
    }

    #[derive(Debug)]
    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo input"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            })
        }

        fn execute<'a>(
            &'a self,
            _input: ToolInput,
            _context: ToolContext,
        ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
            async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: empty_truncation(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            }
            .boxed()
        }
    }

    #[test]
    fn registry_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn registry_schemas() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let schemas = reg.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name(), "echo");
    }

    #[test]
    fn registry_schemas_are_filtered_by_host_policy() {
        let output = || ToolOutput {
            description: "ok".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: Vec::new(),
        };
        let mut registry = ToolRegistry::new();
        registry.register(RegisteredTool::new(
            "undeclared",
            "undeclared",
            serde_json::json!({"type": "object"}),
            move |_input, _context| {
                let output = output();
                async move { Ok(output) }
            },
        ));
        registry.register(
            RegisteredTool::new(
                "declared_read",
                "declared read",
                serde_json::json!({"type": "object"}),
                |_input, _context| async {
                    Ok(ToolOutput {
                        description: "ok".to_string(),
                        truncated: OutputTruncation::empty(),
                        output_file: PathBuf::new(),
                        exit_code: Some(0),
                        timed_out: false,
                        runtime_events: Vec::new(),
                    })
                },
            )
            .with_effect(ToolEffect::Read),
        );

        let policy = crate::AgentExecutionPolicy {
            visible_tools: crate::ToolVisibilitySet::from_tool_names(["declared_read"]),
            allowed_effects: crate::ToolEffectSet::from_effects([ToolEffect::Read]),
            ..crate::AgentExecutionPolicy::default()
        };
        let names = registry
            .schemas_for_policy(&policy)
            .into_iter()
            .map(|schema| schema.name().to_string())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["declared_read".to_string()]);
    }

    #[test]
    fn tool_output_from_model_output_sets_exit_code_and_end_turn_event() {
        let output = ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: "saved".to_string(),
            success: false,
            ends_turn: true,
        });

        assert_eq!(
            output,
            ToolOutput {
                description: "saved".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(1),
                timed_out: false,
                runtime_events: vec![ToolRuntimeEvent::EndTurn],
            }
        );
    }

    #[test]
    fn tool_output_reports_end_turn_runtime_event() {
        let output = ToolOutput {
            description: "saved".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: vec![ToolRuntimeEvent::ToolResultRevision { revision: 1 }],
        };
        assert!(!output.ends_turn());

        let output = ToolOutput {
            runtime_events: vec![
                ToolRuntimeEvent::ToolResultRevision { revision: 1 },
                ToolRuntimeEvent::EndTurn,
            ],
            ..output
        };
        assert!(output.ends_turn());
    }

    #[test]
    fn tool_output_decodes_runtime_output_artifacts() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        let output = ToolOutput {
            description: "saved".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(0),
            timed_out: false,
            runtime_events: vec![
                ToolRuntimeEvent::ToolResultRevision { revision: 1 },
                ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![serde_json::json!({"id": "artifact-1"})],
                },
                ToolRuntimeEvent::EndTurn,
            ],
        };

        assert_eq!(
            output.output_artifacts_as::<ArtifactRecord>(),
            vec![ArtifactRecord {
                id: "artifact-1".to_string(),
            }]
        );
    }

    #[test]
    fn tool_output_projects_execution_result_for_product_adapters() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        let output = ToolOutput {
            description: "model output".to_string(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(1),
            timed_out: false,
            runtime_events: vec![
                ToolRuntimeEvent::OutputArtifacts {
                    artifacts: vec![serde_json::json!({"id": "artifact-1"})],
                },
                ToolRuntimeEvent::EndTurn,
            ],
        };

        assert_eq!(
            output.to_execution_result::<ArtifactRecord>(),
            ToolExecutionResult {
                success: false,
                output: "model output".to_string(),
                model_output: "model output".to_string(),
                ends_turn: true,
                output_artifacts: vec![ArtifactRecord {
                    id: "artifact-1".to_string(),
                }],
                output_bytes_budget: None,
            }
        );
    }

    #[test]
    fn tool_execution_result_keeps_full_output_and_builds_tool_output() {
        let execution = ToolExecutionResult::with_model_tokens(
            true,
            "full output".to_string(),
            true,
            10_000,
            vec![serde_json::json!({"id": "artifact-1", "sizeBytes": 19})],
        );

        assert_eq!(
            execution,
            ToolExecutionResult {
                success: true,
                output: "full output".to_string(),
                model_output: "full output".to_string(),
                ends_turn: true,
                output_artifacts: vec![serde_json::json!({"id": "artifact-1", "sizeBytes": 19})],
                output_bytes_budget: None,
            }
        );
        assert_eq!(
            execution.into_tool_output(),
            ToolOutput {
                description: "full output".to_string(),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: vec![
                    ToolRuntimeEvent::OutputArtifacts {
                        artifacts: vec![serde_json::json!({"id": "artifact-1", "sizeBytes": 19})],
                    },
                    ToolRuntimeEvent::OutputMetrics {
                        raw_bytes: 11,
                        model_visible_bytes: 11,
                        artifact_bytes: 19,
                        result_hash: crate::canonical_content_hash(b"full output"),
                    },
                    ToolRuntimeEvent::EndTurn,
                ],
            }
        );
    }

    #[test]
    fn tool_execution_result_serializes_json_model_output() {
        let execution = ToolExecutionResult::<serde_json::Value>::json(serde_json::json!({
            "queued": [1],
            "deduped": [2],
            "ignored": []
        }))
        .expect("serialize JSON tool output");

        assert_eq!(
            execution,
            ToolExecutionResult {
                success: true,
                output: "{\"deduped\":[2],\"ignored\":[],\"queued\":[1]}".to_string(),
                model_output: "{\"deduped\":[2],\"ignored\":[],\"queued\":[1]}".to_string(),
                ends_turn: false,
                output_artifacts: Vec::new(),
                output_bytes_budget: None,
            }
        );
    }

    #[test]
    fn typed_function_tool_definition_flattens_components_and_rejects_unknown_fields() {
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

        let definition =
            FunctionToolDefinition::<SearchInput>::new("search_product", "Search product records.");
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

    #[tokio::test]
    async fn registered_tool_from_execution_result_honors_cancelled_context() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool_calls = calls.clone();
        let tool = RegisteredTool::from_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            move |_input, _context| {
                let tool_calls = tool_calls.clone();
                async move {
                    tool_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ToolExecutionResult::<serde_json::Value>::new(
                        true,
                        "ran".to_string(),
                        false,
                    ))
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
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default().with_cancellation(token),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    workspace: AgentWorkspace::local(PathBuf::new()),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
                },
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
    async fn registered_tool_from_fallible_execution_result_maps_display_error() {
        #[derive(Debug)]
        struct DisplayError(&'static str);

        impl fmt::Display for DisplayError {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.0)
            }
        }

        let tool = RegisteredTool::from_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |_input, _context| async move {
                Err::<ToolExecutionResult<serde_json::Value>, DisplayError>(DisplayError("boom"))
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    workspace: AgentWorkspace::local(PathBuf::new()),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
                },
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
    fn registry_unregister_removes_named_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        assert!(reg.unregister("echo"));
        assert!(!reg.unregister("echo"));
        assert!(reg.get("echo").is_none());
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

    #[test]
    fn registry_sync_lsp_language_tools_registers_and_removes_languages() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);
        let registry = pl_lsp::LspRuntimeRegistry::new();
        let rust = pl_lsp::LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };

        let registered = reg.sync_lsp_language_tools(&registry, vec![rust]);

        assert_eq!(registered, vec!["rust".to_string()]);
        assert!(reg.get("echo").is_some());
        assert!(reg.get("lsp_query_rust").is_some());

        let rust = pl_lsp::LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };
        let registered = reg.sync_lsp_language_tools(&registry, vec![rust]);

        assert_eq!(registered, vec!["rust".to_string()]);
        assert!(reg.get("lsp_query_rust").is_some());

        let registered = reg.sync_lsp_language_tools(&registry, Vec::new());

        assert!(registered.is_empty());
        assert!(reg.get("echo").is_some());
        assert!(reg.get("lsp_query_rust").is_none());
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
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let context = ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            workspace: AgentWorkspace::local(root.clone()),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::session::AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        };
        let first_guard = context.workspace_write_lock().await;
        let second_context = context.clone();
        let second = tokio::spawn(async move { second_context.workspace_write_lock().await });
        tokio::task::yield_now().await;

        assert!(!second.is_finished());
        drop(first_guard);
        let second_guard = second.await.unwrap();
        drop(second_guard);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
