use std::path::{Path, PathBuf};

use pl_lsp::{LspQuery, LspRuntimeRegistry};
use pl_protocol::PureError;

use super::{BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

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
            let query = resolve_query_path(query, &context)?;
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

fn resolve_query_path(mut query: LspQuery, context: &ToolContext) -> Result<LspQuery, PureError> {
    let Some(path) = query.file_path.take() else {
        return Ok(query);
    };
    let candidate = if path.is_absolute() {
        path
    } else {
        context.workspace_root.join(path)
    };
    let resolved = if query.operation.requires_file() {
        std::fs::canonicalize(&candidate).map_err(|error| PureError::ToolExecutionFailed {
            tool: "lsp_query".to_string(),
            error: format!(
                "failed to resolve filePath '{}': {error}",
                candidate.display()
            ),
        })?
    } else {
        canonicalize_existing_or_parent(&candidate)
    };
    let workspace_root = std::fs::canonicalize(&context.workspace_root)
        .unwrap_or_else(|_| context.workspace_root.clone());
    if !resolved.starts_with(&workspace_root) && !context.allows_workspace_escape() {
        return Err(PureError::ToolExecutionFailed {
            tool: "lsp_query".to_string(),
            error: format!("filePath must be inside workspace: {}", candidate.display()),
        });
    }
    query.file_path = Some(resolved);
    Ok(query)
}

fn canonicalize_existing_or_parent(candidate: &Path) -> PathBuf {
    let mut current = candidate.to_path_buf();
    loop {
        if current.exists()
            && let Ok(canonical) = std::fs::canonicalize(&current)
        {
            return canonical;
        }
        let Some(parent) = current.parent() else {
            return candidate.to_path_buf();
        };
        if parent == current {
            return candidate.to_path_buf();
        }
        current = parent.to_path_buf();
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
        };
        let context = test_context(root.clone(), crate::tool::WorkspaceAccess::WorkspaceOnly);

        let result = resolve_query_path(query, &context);

        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
