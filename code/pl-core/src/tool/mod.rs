mod ask_user;
mod bash;
mod cache;
mod command;
mod container;
mod context;
mod contract;
mod file;
mod git;
mod lsp;
mod mcp_resource;
mod mcp_tool;
mod model_output;
mod output_format;
mod path_policy;
mod plan;
mod registered;
mod registry;
mod shell;
mod skill;
mod text_escape;
mod todo;
mod tool_output;
mod truncation;
mod web_search;
mod workspace_file;

pub use ask_user::AskUserTool;
pub(crate) use bash::command_tool_pair;
pub use bash::{BashInput, BashTool, WriteStdinTool};
pub use cache::{ToolCachePolicy, TurnToolCacheHandle};
#[cfg(feature = "docker-tools")]
pub use container::DockerCliContainerBackend;
pub use container::{
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecOutput,
    ContainerExecRequest, ContainerTool, ContainerToolExecution, ContainerToolKind,
    NoContainerBackend, TOOL_CONTAINER_COPY, TOOL_CONTAINER_EXEC, execute_container_tool,
};
pub use context::*;
pub use contract::*;
pub use file::apply_patch::{Hunk as CodexPatchHunk, parse_patch as parse_codex_patch};
pub use file::{
    CopyPathTool, CreateDirectoryTool, DeletePathTool, MovePathTool, StatPathTool, WriteFileTool,
};
pub use git::{
    ExecutionBackend, ExecutionOutput, ExecutionRequest, GIT_TOKEN_ENV, GitCredential,
    GitCredentialOperation, GitCredentialProvider, GitCredentialRequest, GitPolicy,
    GitShellCommandRequest, GitShellCredential, GitTool, GitToolKind, GitWorkspaceConfig,
    LocalExecutionBackend, LocalExecutionFailure, NoGitCredentialProvider, TOOL_GIT_BRANCH,
    TOOL_GIT_COMMIT, TOOL_GIT_DIFF, TOOL_GIT_FETCH, TOOL_GIT_PUSH, TOOL_GIT_STATUS,
    TOOL_GIT_SYNC_DEFAULT_BRANCH, TOOL_GIT_WORKSPACE_INFO, git_askpass_script, git_shell_command,
    git_shell_credential_prelude, git_shell_retry_function,
};
pub use lsp::{LspLanguageTool, LspQueryTool, lsp_tool_for_language};
pub use mcp_resource::{
    McpListResourceTemplatesRequest, McpListResourcesRequest, McpReadResourceRequest,
    McpResourceBackend, McpResourceTool, McpResourceToolKind, TOOL_LIST_MCP_RESOURCE_TEMPLATES,
    TOOL_LIST_MCP_RESOURCES, TOOL_READ_MCP_RESOURCE,
};
pub use mcp_tool::{
    HostMcpToolSpec, McpTool, McpToolBackend, McpToolRequest, host_mcp_tool_schema,
    host_mcp_tool_schemas,
};
pub use model_output::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, MAX_MODEL_TOOL_OUTPUT_BYTES, enforce_model_output_limit,
    model_visible_tool_output, model_visible_tool_output_with_tokens,
};
pub use output_format::{
    MAX_TOOL_UI_PREVIEW_BYTES, SECRET_REDACTION_REPLACEMENT, SecretRedaction,
    ToolHistoryProjection, ToolLifecyclePhase, ToolLifecycleProjection,
    ToolOutputArtifactDescriptor, ToolOutputArtifactPathRequest, ToolOutputCapture,
    ToolOutputCaptureRequest, ToolOutputStream, ToolOutputStreamCapture, ToolOutputStreamSizes,
    redacted_trace_preview_value, tool_history_projection, tool_lifecycle_projection,
    tool_lifecycle_projections, tool_output_artifact_file_path, trace_preview_output,
    trace_preview_value,
};
pub use path_policy::{PathAccess, ToolPathPolicy};
pub use plan::PlanExitTool;
pub use registered::*;
pub use registry::*;
pub use shell::{ShellCommandTimeout, shell_command_with_timeout, shell_quote_word};
pub use skill::{SkillManageTool, SkillViewTool, SkillsListTool};
pub use todo::{TOOL_UPDATE_TODO_LIST, TodoListTool};
pub use tool_output::*;
pub use truncation::{OutputTruncation, TruncatedOutput, TruncationStrategy};
pub use web_search::{HostedWebSearchTool, TOOL_WEB_SEARCH, WebSearchTool};
pub use workspace_file::apply_patch_to_backend;
pub use workspace_file::{
    ContainerWorkspaceFileBackend, LocalWorkspaceFileBackend, LocalWorkspaceFileTool,
    TOOL_APPLY_PATCH, TOOL_LIST_FILES, TOOL_READ_FILE, TOOL_SEARCH_FILES, WorkspaceFileBackend,
    WorkspaceFileListEntry, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch,
    WorkspaceFileSearchRequest, WorkspaceFileSearchResult, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileTool, WorkspaceFileToolExecution, WorkspaceFileToolKind,
    WorkspaceFileWriteRequest, execute_workspace_file_tool,
};

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::contract::BoxFuture;
    use super::*;
    use crate::turn::{ToolEffect, TurnOptions};
    use pl_model::ToolSchema;
    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;
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
            Box::pin(async {
                Ok(ToolOutput {
                    description: "ok".to_string(),
                    truncated: empty_truncation(),
                    output_file: PathBuf::new(),
                    exit_code: None,
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            })
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
    fn registry_is_empty_initially() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
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
    fn tool_output_consumes_model_visible_output() {
        let output = ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: "visible".to_string(),
            success: true,
            ends_turn: false,
        });

        assert_eq!(output.into_model_output(), "visible");
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
                    ToolRuntimeEvent::EndTurn,
                    ToolRuntimeEvent::OutputArtifacts {
                        artifacts: vec![serde_json::json!({"id": "artifact-1", "sizeBytes": 19})],
                    },
                    ToolRuntimeEvent::OutputMetrics {
                        raw_bytes: 11,
                        model_visible_bytes: 11,
                        artifact_bytes: 19,
                        result_hash: crate::canonical_content_hash(b"full output"),
                    },
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
            }
        );
    }

    #[test]
    fn tool_execution_result_exposes_explicit_success_and_failure_constructors() {
        assert_eq!(
            ToolExecutionResult::<serde_json::Value>::success("ok"),
            ToolExecutionResult::new(true, "ok".to_string(), false)
        );
        assert_eq!(
            ToolExecutionResult::<serde_json::Value>::failure("bad"),
            ToolExecutionResult::new(false, "bad".to_string(), false)
        );
    }

    #[test]
    fn function_tool_schema_builds_strict_object_input_schema() {
        let schema = function_tool_schema(
            "save_task_plan",
            "Save a task plan.",
            [
                ToolInputSchemaField::required("title", serde_json::json!({ "type": "string" })),
                ToolInputSchemaField::required("markdown", serde_json::json!({ "type": "string" })),
                ToolInputSchemaField::optional("metadata", serde_json::json!({ "type": "object" })),
            ],
        );

        let ToolSchema::Function {
            name,
            description,
            input_schema,
        } = schema
        else {
            panic!("function tool schema");
        };
        assert_eq!(name, "save_task_plan");
        assert_eq!(description, "Save a task plan.");
        assert_eq!(
            input_schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "markdown": { "type": "string" },
                    "metadata": { "type": "object" }
                },
                "required": ["title", "markdown"],
                "additionalProperties": false
            })
        );
    }

    #[test]
    fn strict_tool_input_schema_uses_named_field_constructors() {
        let schema = strict_tool_input_schema([
            ToolInputSchemaField::required("path", serde_json::json!({ "type": "string" })),
            ToolInputSchemaField::optional("name", serde_json::json!({ "type": "string" })),
        ]);

        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "name": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            })
        );
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
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::TurnToolCacheHandle::default(),
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
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::TurnToolCacheHandle::default(),
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
    async fn registered_tool_from_typed_fallible_execution_result_deserializes_input() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ProductInput {
            item_id: String,
        }

        let tool = RegisteredTool::from_typed_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |input: ProductInput, _context| async move {
                Ok::<_, &'static str>(
                    ToolExecutionResult::<serde_json::Value>::json(serde_json::json!({
                        "itemId": input.item_id
                    }))
                    .expect("json output"),
                )
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let output = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "itemId": "task-1" }),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::TurnToolCacheHandle::default(),
                },
            )
            .await
            .expect("typed product tool output");

        assert_eq!(output.description, "{\"itemId\":\"task-1\"}");
    }

    #[test]
    fn registered_tool_from_schema_uses_function_schema_metadata() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct ProductInput {
            _item_id: String,
        }

        let schema = function_tool_schema(
            "product_tool",
            "Product tool",
            [ToolInputSchemaField::required(
                "itemId",
                serde_json::json!({ "type": "string" }),
            )],
        );

        let tool = RegisteredTool::from_schema_typed_fallible_execution_result(
            schema,
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        )
        .expect("function schema");

        assert_eq!(tool.name(), "product_tool");
        assert_eq!(tool.description(), "Product tool");
        assert_eq!(
            tool.input_schema(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "itemId": { "type": "string" }
                },
                "required": ["itemId"],
                "additionalProperties": false,
            })
        );
    }

    #[test]
    fn registered_tool_from_schema_rejects_custom_schema() {
        #[derive(Debug, Deserialize)]
        struct ProductInput;

        let result = RegisteredTool::from_schema_typed_fallible_execution_result(
            ToolSchema::custom_grammar("custom_tool", "Custom tool", "lark", "start: /x/"),
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        );

        assert_eq!(
            result
                .expect_err("custom schema must be rejected")
                .to_string(),
            "registered tool `custom_tool` must use a function schema"
        );
    }

    #[tokio::test]
    async fn registered_tool_from_typed_fallible_execution_result_rejects_invalid_input() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ProductInput {
            #[serde(rename = "itemId")]
            _item_id: String,
        }

        let tool = RegisteredTool::from_typed_fallible_execution_result(
            "product_tool",
            "Product tool",
            serde_json::json!({ "type": "object" }),
            |_input: ProductInput, _context| async move {
                Ok::<_, &'static str>(ToolExecutionResult::<serde_json::Value>::success("ok"))
            },
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let result = tool
            .execute(
                ToolInput {
                    arguments: serde_json::json!({ "item_id": "task-1" }),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                ToolContext {
                    event_tx,
                    options: TurnOptions::default(),
                    workspace_access: WorkspaceAccess::WorkspaceOnly,
                    workspace_root: PathBuf::new(),
                    workspace_instructions: None,
                    instruction_snapshot: None,
                    provider_call_id: None,
                    active_subagent: None,
                    lsp_runtime: None,
                    parent_session: Arc::new(crate::session::AgentSession::new()),
                    working_set: crate::TurnWorkingSetHandle::default(),
                    tool_cache: crate::TurnToolCacheHandle::default(),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(PureError::ToolExecutionFailed { tool, error })
                if tool == "product_tool"
                    && error.contains("invalid input")
                    && error.contains("itemId")
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
    fn registry_debug_shows_names() {
        let mut reg = ToolRegistry::new();
        reg.register(EchoTool);

        let debug = format!("{reg:?}");
        assert!(debug.contains("echo"));
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
        let preview = trace_preview_value(&value, 1_000);

        assert!(preview.contains("<redacted>"));
        assert!(preview.contains("visible"));
        assert!(!preview.contains("secret-token"));
        assert!(!preview.contains("secret-key"));
        assert!(!preview.contains(&"YWJj".repeat(30)));
    }

    #[test]
    fn explicit_secret_redaction_handles_text_and_json() {
        let redaction = SecretRedaction::new(["secret", "secret-token", ""]);

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
            workspace_root: root.clone(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::session::AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::TurnToolCacheHandle::default(),
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
