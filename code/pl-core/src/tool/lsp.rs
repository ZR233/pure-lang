use std::future::Future;
use std::path::PathBuf;

use pl_lsp::query::{LspQuery, LspQueryOperation};
use pl_lsp::runtime::{LspRuntimeError, LspRuntimeRegistry};
use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    DynTool, OutputTruncation, StaticTool, ToolCallContext, ToolPathPolicy, ToolPolicy, ToolResult,
    ToolWorkspace,
};

/// 构造 lsp 来源的 seam 工具条目（`lsp_capabilities` + `lsp_query`）。
pub fn lsp_tools(registry: LspRuntimeRegistry, workspace: ToolWorkspace) -> Vec<DynTool> {
    vec![
        LspCapabilitiesTool::new(registry.clone(), workspace.clone()).into(),
        LspQueryTool::new(registry, workspace).into(),
    ]
}

#[derive(Debug, Clone)]
pub struct LspCapabilitiesTool {
    registry: LspRuntimeRegistry,
    workspace: ToolWorkspace,
}

impl LspCapabilitiesTool {
    pub fn new(registry: LspRuntimeRegistry, workspace: ToolWorkspace) -> Self {
        Self {
            registry,
            workspace,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LspCapabilitiesInput {}

impl StaticTool for LspCapabilitiesTool {
    type Input = LspCapabilitiesInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("lsp_capabilities"),
            "List language servers available in the current workspace with their language ids, supported lsp_query operations, and readiness. Call this before lsp_query to discover valid languageId values.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_programmatic_calls()
    }

    fn execute(
        &self,
        _input: LspCapabilitiesInput,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let capabilities = self
                .registry
                .capabilities_for_workspace(self.workspace.root())
                .await;
            let description = serde_json::to_string_pretty(&capabilities).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: "lsp_capabilities".to_string(),
                    error: format!("failed to serialize LSP capabilities: {error}"),
                }
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspQueryInput {
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
    workspace: ToolWorkspace,
}

impl LspQueryTool {
    pub fn new(registry: LspRuntimeRegistry, workspace: ToolWorkspace) -> Self {
        Self {
            registry,
            workspace,
        }
    }
}

impl StaticTool for LspQueryTool {
    type Input = LspQueryInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("lsp_query"),
            "Query language servers for semantic code intelligence. Provide languageId (see lsp_capabilities) plus an operation and its parameters. Prefer this over text search when resolving definitions, references, hover/type or signature information, implementations, symbols, call hierarchy, or diagnostics.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_programmatic_calls()
    }

    fn execute(
        &self,
        parsed: LspQueryInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let query = LspQuery {
                operation: parsed.operation,
                file_path: parsed.file_path,
                line: parsed.line,
                character: parsed.character,
                query: parsed.query,
                max_results: parsed.max_results,
                language_id: Some(parsed.language_id.clone()),
            };
            let query = resolve_query_path(query, &self.workspace, &context, "lsp_query")?;
            let result = self
                .registry
                .query_in_workspace(self.workspace.root(), query)
                .await
                .map_err(|error| unknown_language_error("lsp_query", &parsed.language_id, error))?;
            let description = serde_json::to_string_pretty(&result).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: "lsp_query".to_string(),
                    error: format!("failed to serialize LSP result: {error}"),
                }
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

/// 未知 languageId 时附带当前可用语言的可恢复错误。
fn unknown_language_error(tool_name: &str, language_id: &str, error: LspRuntimeError) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool_name.to_string(),
        error: format!(
            "no language server for languageId `{language_id}` ({error}); call lsp_capabilities to list available languages"
        ),
    }
}

fn resolve_query_path(
    mut query: LspQuery,
    workspace: &ToolWorkspace,
    context: &ToolCallContext,
    tool_name: &str,
) -> Result<LspQuery, PureError> {
    let Some(path) = query.file_path.take() else {
        return Ok(query);
    };
    let policy = ToolPathPolicy::new(
        workspace.root().to_path_buf(),
        workspace.allows_workspace_escape(context),
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
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::tool::{StaticToolTestExt, ToolApprovalContext, ToolInput, deserialize_tool_input};

    fn test_context(
        _workspace_root: PathBuf,
        workspace_access: crate::tool::WorkspaceAccess,
    ) -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx).with_approval(ToolApprovalContext::new(
            crate::turn::PermissionMode::RequestApproval,
            workspace_access,
        ))
    }

    fn workspace(root: PathBuf) -> ToolWorkspace {
        ToolWorkspace::new(crate::tool::AgentWorkspace::local(root))
    }

    #[test]
    fn lsp_tools_expose_capabilities_and_query_only() {
        let tools = lsp_tools(LspRuntimeRegistry::new(), workspace(std::env::temp_dir()));

        let names = tools
            .iter()
            .map(|tool| tool.definition().name().wire_name())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["lsp_capabilities", "lsp_query"]);
        for tool in &tools {
            assert_eq!(tool.policy().effect(), Some(crate::turn::ToolEffect::Read));
        }
        // catalog 中不得出现按语言命名的 lsp_query_{lang} 形态。
        assert!(!names.iter().any(|name| name.starts_with("lsp_query_")));
    }

    #[test]
    fn lsp_query_input_requires_language_id_and_keeps_typed_fields() {
        let schema = LspQueryTool::new(LspRuntimeRegistry::new(), workspace(std::env::temp_dir()))
            .input_schema();

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
        let tool = LspQueryTool::new(LspRuntimeRegistry::new(), workspace(std::env::temp_dir()));
        let context = test_context(
            std::env::temp_dir(),
            crate::tool::WorkspaceAccess::WorkspaceOnly,
        );
        let input = ToolInput {
            arguments: serde_json::json!({
                "languageId": "kotlin",
                "operation": "diagnostics",
            }),
        };

        let error = tool.execute_raw(input, context).await.unwrap_err();

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
            operation: LspQueryOperation::Hover,
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

        let result = resolve_query_path(query, &workspace(root.clone()), &context, "lsp_query");

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

        let resolved =
            resolve_query_path(query, &workspace(root.clone()), &context, "lsp_query").unwrap();

        assert_eq!(
            resolved.file_path,
            Some(dunce::canonicalize(&file).unwrap())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
