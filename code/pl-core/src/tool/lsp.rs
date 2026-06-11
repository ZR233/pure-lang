use std::path::PathBuf;

use pl_lsp::{LanguageToolInfo, LspQuery, LspRuntimeRegistry};
use pl_protocol::PureError;

use super::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, ToolPathPolicy,
};

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
        lsp_query_input_schema()
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
            let query: LspQuery = serde_json::from_value(input.arguments).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("invalid LSP query input: {error}"),
                }
            })?;
            let query = resolve_query_path(query, &context, self.name())?;
            let result = self.registry.query(query).await.map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: error.to_string(),
                }
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
            })
        })
    }
}

fn lsp_query_input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "operation": {
                "type": "string",
                "enum": [
                    "goToDefinition",
                    "findReferences",
                    "hover",
                    "documentSymbol",
                    "workspaceSymbol",
                    "goToImplementation",
                    "prepareCallHierarchy",
                    "incomingCalls",
                    "outgoingCalls",
                    "diagnostics"
                ]
            },
            "filePath": {
                "type": "string",
                "description": "Workspace-relative or absolute path to the source file. Required for file and position operations."
            },
            "line": {
                "type": "integer",
                "minimum": 1,
                "description": "1-based line number for position operations."
            },
            "character": {
                "type": "integer",
                "minimum": 1,
                "description": "1-based UTF-16 character offset for position operations."
            },
            "query": {
                "type": "string",
                "description": "Workspace symbol query string."
            },
            "maxResults": {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum diagnostics to return."
            }
        },
        "required": ["operation"],
        "additionalProperties": false
    })
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
        context.workspace_root.clone(),
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
        let name = format!("lsp_query_{}", info.language_id);
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
        lsp_query_input_schema()
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
            let mut query: LspQuery = serde_json::from_value(input.arguments).map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: format!("invalid LSP query input: {error}"),
                }
            })?;
            query.language_id = Some(self.language_id.clone());
            let query = resolve_query_path(query, &context, self.name())?;
            let result = self.registry.query(query).await.map_err(|error| {
                PureError::ToolExecutionFailed {
                    tool: self.name().to_string(),
                    error: error.to_string(),
                }
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
    use crate::turn::{CompileMode, TurnOptions};
    use crate::{AgentControl, CoreSession};

    fn test_context(
        workspace_root: PathBuf,
        workspace_access: crate::tool::WorkspaceAccess,
    ) -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access,
            mode: CompileMode::Auto,
            workspace_root,
            workspace_instructions: None,
            instruction_snapshot: None,
            active_subagent: None,
            agent_control: AgentControl::default(),
            lsp_runtime: None,
            parent_session: Arc::new(CoreSession::new()),
        }
    }

    #[test]
    fn lsp_query_schema_exposes_supported_operations() {
        let tool = LspQueryTool::new(LspRuntimeRegistry::new());
        let schema = tool.input_schema();

        assert_eq!(schema["required"], serde_json::json!(["operation"]));
        assert!(
            schema["properties"]["operation"]["enum"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("goToDefinition"))
        );
        assert_eq!(
            schema["properties"]["line"]["minimum"],
            serde_json::json!(1)
        );
        assert_eq!(
            schema["properties"]["character"]["minimum"],
            serde_json::json!(1)
        );
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

    #[test]
    fn lsp_language_tool_schema_exposes_supported_operations() {
        let info = LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: "rust-analyzer".to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        };
        let tool = LspLanguageTool::new(&info, LspRuntimeRegistry::new());
        let schema = tool.input_schema();

        assert_eq!(schema["required"], serde_json::json!(["operation"]));
        assert!(
            schema["properties"]["operation"]["enum"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("diagnostics"))
        );
        assert_eq!(schema["additionalProperties"], serde_json::json!(false));
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
        };

        let error = tool.execute(input, context).await.unwrap_err();

        match error {
            PureError::ToolExecutionFailed { tool, error } => {
                assert_eq!(tool, "lsp_query_rust");
                assert!(error.contains("No LSP server configured for language: rust"));
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
            Some(std::fs::canonicalize(&file).unwrap())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
