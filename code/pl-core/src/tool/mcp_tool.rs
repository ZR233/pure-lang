use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::{PureError, Result};
use serde_json::{Value, json};

use super::{BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

/// 宿主侧 MCP tool 的模型可见 schema 描述。
///
/// 宿主负责发现 MCP session 里的原始 tool；pl-core 负责把这些发现结果
/// 转成统一的模型工具 schema，并通过 `McpToolBackend` 进入共享 dispatch。
#[derive(Debug, Clone, PartialEq)]
pub struct HostMcpToolSpec {
    pub model_name: String,
    pub server: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn host_mcp_tool_schema(spec: HostMcpToolSpec) -> ToolSchema {
    let description = if spec.description.is_empty() {
        format!("Call MCP tool `{}` on server `{}`.", spec.name, spec.server)
    } else {
        spec.description
    };
    ToolSchema::function(spec.model_name, description, spec.input_schema)
}

pub fn host_mcp_tool_schemas(specs: impl IntoIterator<Item = HostMcpToolSpec>) -> Vec<ToolSchema> {
    specs.into_iter().map(host_mcp_tool_schema).collect()
}

/// 宿主提供的 MCP tool 调用请求。
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolRequest {
    pub name: String,
    pub arguments: Value,
}

/// 宿主 MCP tool 后端。
///
/// pl-core 负责注册模型可见 schema、执行统一 tool dispatch、trace 和
/// tool result history；宿主只把请求投递到自己管理的 MCP session。
pub trait McpToolBackend: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn call_tool(
        &self,
        request: McpToolRequest,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;
}

/// 宿主提供 schema 的 MCP tool。
#[derive(Clone)]
pub struct McpTool<B> {
    name: String,
    description: String,
    input_schema: Value,
    backend: Arc<B>,
}

impl<B> fmt::Debug for McpTool<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpTool")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<B> McpTool<B> {
    pub fn new(schema: ToolSchema, backend: Arc<B>) -> Self {
        let name = schema.name().to_string();
        let description = schema.description().to_string();
        let input_schema = match schema {
            ToolSchema::Function { input_schema, .. } => input_schema,
            ToolSchema::Custom { .. } => json!({ "type": "object" }),
        };
        Self {
            name,
            description,
            input_schema,
            backend,
        }
    }
}

impl<B> Tool for McpTool<B>
where
    B: McpToolBackend + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        Box::pin(async move {
            let value = self
                .backend
                .call_tool(McpToolRequest {
                    name: self.name.clone(),
                    arguments: input.arguments,
                })
                .await
                .map_err(|error| tool_error(&self.name, error))?;
            json_output(&self.name, value)
        })
    }
}

fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

fn json_output(tool: &str, value: Value) -> Result<ToolOutput> {
    let description = serde_json::to_string(&value).map_err(|error| {
        tool_error(
            tool,
            format!("failed to serialize MCP tool output: {error}"),
        )
    })?;
    Ok(ToolOutput {
        description,
        truncated: OutputTruncation::empty(),
        output_file: PathBuf::new(),
        exit_code: None,
        timed_out: false,
        runtime_events: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use pl_model::ToolSchema;
    use pl_protocol::PureError;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{HostMcpToolSpec, McpTool, McpToolBackend, McpToolRequest, host_mcp_tool_schema};
    use crate::tool::{Tool, ToolContext, ToolInput, WorkspaceAccess};

    #[derive(Debug)]
    struct DisplayError(&'static str);

    impl std::fmt::Display for DisplayError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    #[derive(Debug)]
    struct FailingBackend;

    impl McpToolBackend for FailingBackend {
        type Error = DisplayError;

        async fn call_tool(
            &self,
            _request: McpToolRequest,
        ) -> std::result::Result<serde_json::Value, Self::Error> {
            Err(DisplayError("offline"))
        }
    }

    #[test]
    fn host_mcp_tool_schema_uses_canonical_description_and_input_shape() {
        let schema = host_mcp_tool_schema(HostMcpToolSpec {
            model_name: "mcp__docs__lookup".to_string(),
            server: "docs".to_string(),
            name: "lookup".to_string(),
            description: String::new(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }),
        });

        assert_eq!(schema.name(), "mcp__docs__lookup");
        assert_eq!(
            schema.description(),
            "Call MCP tool `lookup` on server `docs`."
        );
        let ToolSchema::Function { input_schema, .. } = schema else {
            panic!("host MCP tool must be a function schema");
        };
        assert_eq!(
            input_schema,
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            })
        );
    }

    #[test]
    fn host_mcp_tool_schema_preserves_explicit_description() {
        let schema = host_mcp_tool_schema(HostMcpToolSpec {
            model_name: "mcp__repo__search".to_string(),
            server: "repo".to_string(),
            name: "search".to_string(),
            description: "Search repository docs.".to_string(),
            input_schema: json!({ "type": "object" }),
        });

        assert_eq!(schema.description(), "Search repository docs.");
    }

    #[tokio::test]
    async fn mcp_tool_maps_backend_display_error_to_tool_error() {
        let tool = McpTool::new(
            host_mcp_tool_schema(HostMcpToolSpec {
                model_name: "mcp__docs__lookup".to_string(),
                server: "docs".to_string(),
                name: "lookup".to_string(),
                description: String::new(),
                input_schema: json!({ "type": "object" }),
            }),
            std::sync::Arc::new(FailingBackend),
        );
        let error = tool
            .execute(
                ToolInput {
                    arguments: json!({}),
                    session_id: "session".to_string(),
                    tool_id: "tool-call".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .expect_err("backend should fail");

        assert!(matches!(
            error,
            PureError::ToolExecutionFailed { tool, error }
                if tool == "mcp__docs__lookup" && error == "offline"
        ));
    }

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: crate::TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::CompileMode::Simple,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            agent_supervisor: crate::AgentSupervisor::default(),
            agent_tool_registrar: None,
            lsp_runtime: None,
            parent_session: std::sync::Arc::new(crate::CoreSession::new()),
        }
    }
}
