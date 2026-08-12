use std::path::PathBuf;

use pl_lsp::{LanguageToolInfo, LspQuery, LspRuntimeRegistry};
use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    BoxFuture, FunctionToolDefinition, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput,
    ToolPathPolicy, deserialize_tool_input,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LspQueryInput {
    operation: LspQueryOperationInput,
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
    /// Maximum diagnostics to return.
    #[schemars(range(min = 1))]
    max_results: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum LspQueryOperationInput {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    Diagnostics,
}

impl From<LspQueryOperationInput> for pl_lsp::LspQueryOperation {
    fn from(operation: LspQueryOperationInput) -> Self {
        match operation {
            LspQueryOperationInput::GoToDefinition => Self::GoToDefinition,
            LspQueryOperationInput::FindReferences => Self::FindReferences,
            LspQueryOperationInput::Hover => Self::Hover,
            LspQueryOperationInput::DocumentSymbol => Self::DocumentSymbol,
            LspQueryOperationInput::WorkspaceSymbol => Self::WorkspaceSymbol,
            LspQueryOperationInput::GoToImplementation => Self::GoToImplementation,
            LspQueryOperationInput::PrepareCallHierarchy => Self::PrepareCallHierarchy,
            LspQueryOperationInput::IncomingCalls => Self::IncomingCalls,
            LspQueryOperationInput::OutgoingCalls => Self::OutgoingCalls,
            LspQueryOperationInput::Diagnostics => Self::Diagnostics,
        }
    }
}

impl From<LspQueryInput> for LspQuery {
    fn from(input: LspQueryInput) -> Self {
        Self {
            operation: input.operation.into(),
            file_path: input.file_path,
            line: input.line,
            character: input.character,
            query: input.query,
            max_results: input.max_results,
            language_id: None,
        }
    }
}

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
        "Query active Language Server Protocol servers for semantic code intelligence. Prefer this over text search for supported languages when resolving definitions, references, hover/type or signature information, implementations, symbols, call hierarchy, or diagnostics. Supports Rust via rust-analyzer."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<LspQueryInput>::new(self.name(), self.description()).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let query: LspQuery =
                deserialize_tool_input::<LspQueryInput>(self.name(), input.arguments)?.into();
            let query = resolve_query_path(query, &context, self.name())?;
            let result = self
                .registry
                .query_in_workspace(context.workspace.root(), query)
                .await
                .map_err(|error| PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: error.to_string(),
                })?;
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
        })
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

/// 按语言生成的 LSP 查询工具。
///
/// 每个实例对应一种可用的 LSP 语言，工具名为 `lsp_query_{language_id}`。
/// 执行时自动注入 `language_id` 到 `LspQuery` 以精确路由到目标语言服务器。
#[derive(Debug, Clone)]
pub struct LspLanguageTool {
    registry: LspRuntimeRegistry,
    language_id: String,
    name: String,
    description: String,
}

impl LspLanguageTool {
    pub fn new(info: &LanguageToolInfo, registry: LspRuntimeRegistry) -> Self {
        let lang_id = &info.language_id;
        let name = format!("lsp_query_{lang_id}");
        let language_name = language_display_name(&info.language_id);
        let extensions = if info.extensions.is_empty() {
            "none declared".to_string()
        } else {
            info.extensions.join(", ")
        };
        let description = format!(
            "Query {server} for {language} semantic code intelligence. Prefer this over text search when resolving definitions, references, hover/type or signature information, implementations, symbols, call hierarchy, or diagnostics. Powered by {server_id}. Supported file extensions: {extensions}.",
            server = info.display_name,
            language = language_name,
            server_id = info.server_id,
        );
        Self {
            registry,
            language_id: info.language_id.clone(),
            name,
            description,
        }
    }
}

/// 为特定语言生成 LSP 查询工具。
pub fn lsp_tool_for_language(
    info: &LanguageToolInfo,
    registry: LspRuntimeRegistry,
) -> Box<dyn Tool> {
    Box::new(LspLanguageTool::new(info, registry))
}

impl Tool for LspLanguageTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<LspQueryInput>::new(self.name(), self.description()).input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let mut query: LspQuery =
                deserialize_tool_input::<LspQueryInput>(self.name(), input.arguments)?.into();
            query.language_id = Some(self.language_id.clone());
            let query = resolve_query_path(query, &context, self.name())?;
            let result = self
                .registry
                .query_in_workspace(context.workspace.root(), query)
                .await
                .map_err(|error| PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: error.to_string(),
                })?;
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
        })
    }
}

fn language_display_name(language_id: &str) -> String {
    match language_id {
        "rust" => "Rust".to_string(),
        "typescript" => "TypeScript".to_string(),
        "javascript" => "JavaScript".to_string(),
        "python" => "Python".to_string(),
        "go" => "Go".to_string(),
        other => {
            let mut chars = other.chars();
            let Some(first) = chars.next() else {
                return "unknown".to_string();
            };
            format!(
                "{}{}",
                first.to_uppercase().collect::<String>(),
                chars.as_str()
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pl_lsp::LspQueryOperation;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::AgentSession;
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
            parent_session: Arc::new(AgentSession::new()),
            working_set: crate::TurnWorkingSetHandle::default(),
            tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
        }
    }

    #[test]
    fn lsp_language_tool_name_and_description_are_per_instance() {
        let registry = LspRuntimeRegistry::new();
        let rust = LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };
        let typescript = LanguageToolInfo {
            language_id: "typescript".to_string(),
            server_id: "typescript-language-server".to_string(),
            display_name: "TypeScript language server".to_string(),
            extensions: vec![".ts".to_string(), ".tsx".to_string()],
        };

        let rust_tool = LspLanguageTool::new(&rust, registry.clone());
        let typescript_tool = LspLanguageTool::new(&typescript, registry);

        assert_eq!(rust_tool.name(), "lsp_query_rust");
        assert_eq!(typescript_tool.name(), "lsp_query_typescript");
        assert!(rust_tool.description().contains("rust-analyzer"));
        assert!(rust_tool.description().contains("Rust"));
        assert!(rust_tool.description().contains(".rs"));
        assert!(typescript_tool.description().contains("TypeScript"));
        assert!(typescript_tool.description().contains(".tsx"));
    }

    #[tokio::test]
    async fn lsp_language_tool_execute_injects_language_id() {
        let info = LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };
        let tool = LspLanguageTool::new(&info, LspRuntimeRegistry::new());
        let context = test_context(
            std::env::temp_dir(),
            crate::tool::WorkspaceAccess::WorkspaceOnly,
        );
        let input = ToolInput {
            arguments: serde_json::json!({"operation": "diagnostics"}),
            session_id: "session-1".to_string(),
            tool_id: "tool-1".to_string(),
            revision_base: 0,
        };

        let error = tool.execute(input, context).await.unwrap_err();

        match error {
            PureError::ToolExecutionFailed { tool, error } => {
                assert_eq!(tool, "lsp_query_rust");
                assert!(error.contains("no LSP server found for language: rust"));
            }
            other => panic!("unexpected error: {other}"),
        }
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
        let outside_file = outside.join("lib.rs");
        std::fs::write(&outside_file, "fn main() {}\n").unwrap();

        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(outside_file),
            line: Some(1),
            character: Some(1),
            query: None,
            max_results: None,
            language_id: None,
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
            language_id: None,
        };
        let context = test_context(root.clone(), crate::tool::WorkspaceAccess::WorkspaceOnly);

        let resolved = resolve_query_path(query, &context, "lsp_query").unwrap();

        assert_eq!(
            resolved.file_path,
            Some(dunce::canonicalize(&file).unwrap())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_query_path_rejects_link_ancestor() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pure-lsp-link-root-{unique}"));
        let outside = std::env::temp_dir().join(format!("pure-lsp-link-outside-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("lib.rs"), "fn main() {}\n").unwrap();
        create_directory_link(&outside, &root.join("linked"));
        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(PathBuf::from("linked/lib.rs")),
            line: Some(1),
            character: Some(1),
            query: None,
            max_results: None,
            language_id: None,
        };
        let context = test_context(root.clone(), crate::tool::WorkspaceAccess::WorkspaceOnly);

        let error = resolve_query_path(query, &context, "lsp_query")
            .unwrap_err()
            .to_string();

        assert!(error.contains("reparse point"), "{error}");
        remove_directory_link(&root.join("linked"));
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(target: &std::path::Path, link: &std::path::Path) {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    }

    #[cfg(unix)]
    fn remove_directory_link(link: &std::path::Path) {
        std::fs::remove_file(link).unwrap();
    }

    #[cfg(windows)]
    fn remove_directory_link(link: &std::path::Path) {
        std::fs::remove_dir(link).unwrap();
    }
}
