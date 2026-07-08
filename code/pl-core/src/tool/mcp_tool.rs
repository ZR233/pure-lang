use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::{PureError, Result};
use serde_json::{Value, json};

use super::{BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

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
    fn call_tool(&self, request: McpToolRequest) -> impl Future<Output = Result<Value>> + Send;
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
                .await?;
            json_output(&self.name, value)
        })
    }
}

fn json_output(tool: &str, value: Value) -> Result<ToolOutput> {
    let description =
        serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool.to_string(),
            error: format!("failed to serialize MCP tool output: {error}"),
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
