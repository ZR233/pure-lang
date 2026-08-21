use std::path::PathBuf;

use futures::FutureExt;
use pl_lsp::{LspQuery, LspQueryOperation, LspRuntimeRegistry};
use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BoxFuture, FunctionToolDefinition, OutputTruncation, Tool, ToolContext, ToolEntry, ToolInput,
    ToolOutput, ToolPathPolicy, ToolSourceId, ToolSourceMetadata, deserialize_tool_input,
};

/// lsp seam 来源的命名空间描述。
pub fn lsp_namespace() -> Option<super::NamespaceDescriptor> {
    Some(super::NamespaceDescriptor::new(
        "lsp",
        "Language server semantic queries (definition/references/hover/symbols/...).",
    ))
}

/// 构造 lsp 来源的 seam 工具条目（`lsp_capabilities` + `lsp_query`）。
pub fn lsp_tool_entries(registry: LspRuntimeRegistry) -> Vec<ToolEntry> {
    let source = ToolSourceId::lsp();
    let metadata = || ToolSourceMetadata {
        source: source.clone(),
        namespace: lsp_namespace(),
        programmatic_eligible: true,
    };
    vec![
        ToolEntry::new(LspCapabilitiesTool::new(registry.clone()), metadata()),
        ToolEntry::new(LspQueryTool::new(registry), metadata()),
    ]
}

#[derive(Debug, Clone)]
pub struct LspCapabilitiesTool {
    registry: LspRuntimeRegistry,
}

impl LspCapabilitiesTool {
    pub fn new(registry: LspRuntimeRegistry) -> Self {
        Self { registry }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LspCapabilitiesInput {}

impl Tool for LspCapabilitiesTool {
    fn name(&self) -> &str {
        "lsp_capabilities"
    }

    fn description(&self) -> &str {
        "List language servers available in the current workspace with their language ids, supported lsp_query operations, and readiness. Call this before lsp_query to discover valid languageId values."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<LspCapabilitiesInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn effect(&self) -> Option<crate::turn::ToolEffect> {
        Some(crate::turn::ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        async move {
            deserialize_tool_input::<LspCapabilitiesInput>(self.name(), input.arguments)?;
            let capabilities = self
                .registry
                .capabilities_for_workspace(context.workspace.root())
                .await;
            let description = serde_json::to_string_pretty(&capabilities).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("failed to serialize LSP capabilities: {error}"),
                }
            })?;
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        }
        .boxed()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LspQueryInput {
    /// 目标语言 ID；运行期按 catalog 路由到对应 server。
    language_id: String,
    operation: LspQueryOperation,
    /// Workspace-relative or absolute path to the source file.
    file_path: Option<PathBuf>,
    /// 1-based line number for position operations.
    #[schemars(range(min = 1))]
    line: Option<u32>,
    /// 1-based UTF-16 character offset for position operations.
    #[schemars(range(min = 1))]
    character: Option<u32>,
    /// Workspace symbol query string.
    query: Option<String>,
    /// Maximum results to return.
    #[schemars(range(min = 1))]
    max_results: Option<usize>,
}

/// 统一的 LSP 语义查询工具；按 `languageId` 路由到对应 server。
#[derive(Debug, Clone)]
pub struct LspQueryTool {
    registry: LspRuntimeRegistry,
}

impl LspQueryTool {
    pub fn new(registry: LspRuntimeRegistry) -> Self {
        Self { registry }
    }
}

impl Tool for LspQueryTool {
    fn name(&self) -> &str {
        "lsp_query"
    }

    fn description(&self) -> &str {
        "Query language servers for semantic code intelligence. Provide languageId (see lsp_capabilities) plus an operation and its parameters. Prefer this over text search when resolving definitions, references, hover/type or signature information, implementations, symbols, call hierarchy, or diagnostics."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<LspQueryInput>::new(self.name(), self.description()).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn effect(&self) -> Option<crate::turn::ToolEffect> {
        Some(crate::turn::ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        async move {
            let parsed: LspQueryInput =
                deserialize_tool_input::<LspQueryInput>(self.name(), input.arguments)?;
            let query = LspQuery {
                operation: parsed.operation,
                file_path: parsed.file_path,
                line: parsed.line,
                character: parsed.character,
                query: parsed.query,
                max_results: parsed.max_results,
                language_id: Some(parsed.language_id.clone()),
            };
            let query = resolve_query_path(query, &context, self.name())?;
            let result = self
                .registry
                .query_in_workspace(context.workspace.root(), query)
                .await
                .map_err(|error| unknown_language_error(self.name(), &parsed.language_id, error))?;
            let description = serde_json::to_string_pretty(&result).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("failed to serialize LSP result: {error}"),
                }
            })?;
            Ok(ToolOutput {
                description,
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
                runtime_events: Vec::new(),
            })
        }
        .boxed()
    }
}

/// 未知 languageId 时附带当前可用语言的可恢复错误。
fn unknown_language_error(
    tool_name: &str,
    language_id: &str,
    error: pl_lsp::LspRuntimeError,
) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool_name.to_string(),
        error: format!(
            "no language server for languageId `{language_id}` ({error}); call lsp_capabilities to list available languages"
        ),
    }
}

fn resolve_query_path(
    mut query: LspQuery,
    context: &ToolContext,
    tool_name: &str,
) -> Result<LspQuery, PureError> {
    let Some(path) = query.file_path.take() else {
        return Ok(query);
    };
    let policy = ToolPathPolicy::new(
        context.workspace.root().to_path_buf(),
        context.allows_workspace_escape(),
        tool_name,
    )?;
    let original = path.display().to_string();
    let resolved = if query.operation.requires_file() {
        policy.resolve_existing_path(&path, &original)?
    } else {
        policy.resolve_existing_or_parent_path(&path, &original)?
    };
    query.file_path = Some(resolved);
    Ok(query)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::turn::TurnOptions;

    fn test_context(
        workspace_root: PathBuf,
        workspace_access: crate::tool::WorkspaceAccess,
    ) -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access,
            workspace: crate::tool::AgentWorkspace::local(workspace_root),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::session::AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        }
    }

    #[test]
    fn lsp_seam_entries_expose_capabilities_and_query_only() {
        let entries = lsp_tool_entries(LspRuntimeRegistry::new());

        let names = entries.iter().map(ToolEntry::name).collect::<Vec<_>>();
        assert_eq!(names, vec!["lsp_capabilities", "lsp_query"]);
        for entry in &entries {
            assert_eq!(entry.metadata().source, ToolSourceId::lsp());
            assert_eq!(
                entry
                    .metadata()
                    .namespace
                    .as_ref()
                    .map(|ns| ns.name.as_str()),
                Some("lsp")
            );
            assert!(entry.metadata().programmatic_eligible);
            assert_eq!(entry.tool().effect(), Some(crate::turn::ToolEffect::Read));
        }
        // catalog 中不得出现按语言命名的 lsp_query_{lang} 形态。
        assert!(!names.iter().any(|name| name.starts_with("lsp_query_")));
    }

    #[test]
    fn lsp_query_input_requires_language_id_and_keeps_typed_fields() {
        let schema = LspQueryTool::new(LspRuntimeRegistry::new()).input_schema();

        assert!(schema["properties"].get("languageId").is_some());
        assert!(schema["properties"].get("operation").is_some());
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("languageId"))
        );

        let error = deserialize_tool_input::<LspQueryInput>(
            "lsp_query",
            serde_json::json!({ "operation": "diagnostics" }),
        )
        .unwrap_err();
        assert!(error.to_string().contains("languageId"), "{error}");
    }

    #[tokio::test]
    async fn lsp_query_routes_by_language_id_with_recoverable_error() {
        let tool = LspQueryTool::new(LspRuntimeRegistry::new());
        let context = test_context(
            std::env::temp_dir(),
            crate::tool::WorkspaceAccess::WorkspaceOnly,
        );
        let input = ToolInput {
            arguments: serde_json::json!({
                "languageId": "kotlin",
                "operation": "diagnostics",
            }),
            session_id: "session-1".to_string(),
            tool_id: "tool-1".to_string(),
            revision_base: 0,
        };

        let error = tool.execute(input, context).await.unwrap_err();

        match error {
            PureError::ToolExecutionFailed { tool, error } => {
                assert_eq!(tool, "lsp_query");
                assert!(error.contains("languageId `kotlin`"), "{error}");
                assert!(error.contains("lsp_capabilities"), "{error}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn lsp_query_input_maps_to_registry_query_with_language_routing() {
        let parsed: LspQueryInput = serde_json::from_value(serde_json::json!({
            "languageId": "rust",
            "operation": "hover",
            "filePath": "src/lib.rs",
            "line": 3,
            "character": 5,
        }))
        .unwrap();

        assert_eq!(parsed.language_id, "rust");
        assert_eq!(parsed.operation, LspQueryOperation::Hover);
        assert_eq!(parsed.file_path, Some(PathBuf::from("src/lib.rs")));
        // languageId 提升为必填顶层字段并注入 LspQuery 路由。
        let query = LspQuery {
            operation: pl_lsp::LspQueryOperation::Hover,
            file_path: parsed.file_path.clone(),
            line: parsed.line,
            character: parsed.character,
            query: parsed.query.clone(),
            max_results: parsed.max_results,
            language_id: Some(parsed.language_id.clone()),
        };
        assert_eq!(query.language_id.as_deref(), Some("rust"));
    }

    #[test]
    fn resolve_query_path_rejects_workspace_escape() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pure-lsp-workspace-{unique}"));
        let outside = std::env::temp_dir().join(format!("pure-lsp-outside-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("lib.rs"), "fn main() {}\n").unwrap();

        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(outside.join("lib.rs")),
            line: Some(1),
            character: Some(1),
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        };
        let context = test_context(root.clone(), crate::tool::WorkspaceAccess::WorkspaceOnly);

        let result = resolve_query_path(query, &context, "lsp_query");

        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn resolve_query_path_uses_workspace_root_for_relative_file_path() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pure-lsp-relative-{unique}"));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let file = src.join("lib.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();

        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(PathBuf::from("src/lib.rs")),
            line: Some(1),
            character: Some(1),
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        };
        let context = test_context(root.clone(), crate::tool::WorkspaceAccess::WorkspaceOnly);

        let resolved = resolve_query_path(query, &context, "lsp_query").unwrap();

        assert_eq!(
            resolved.file_path,
            Some(dunce::canonicalize(&file).unwrap())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
