use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use futures::Future;
use pl_protocol::{PureError, Result};
use serde_json::Value;

use crate::tool::{OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

use super::client::McpClient;
use super::wire::{McpCallToolResult, McpToolDefinition};
use super::{McpRuntimeRegistry, exposed_tool_name};

#[derive(Debug, Clone)]
pub(crate) struct McpToolAdapter {
    server_id: String,
    exposed_name: String,
    raw_name: String,
    description: String,
    input_schema: Value,
    client: Arc<dyn McpClient>,
    registry: Option<McpRuntimeRegistry>,
}

impl McpToolAdapter {
    pub(super) fn new(
        server_id: &str,
        definition: McpToolDefinition,
        client: Arc<dyn McpClient>,
        registry: Option<McpRuntimeRegistry>,
    ) -> Result<Self> {
        let exposed_name = exposed_tool_name(server_id, &definition.name)?;
        Ok(Self {
            server_id: server_id.to_string(),
            exposed_name,
            raw_name: definition.name,
            description: definition.description.unwrap_or_default(),
            input_schema: definition.input_schema,
            client,
            registry,
        })
    }
}

impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.exposed_name
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
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>> {
        Box::pin(async move {
            let params = serde_json::json!({
                "name": self.raw_name,
                "arguments": input.arguments,
            });
            let value = match self.client.request("tools/call", params).await {
                Ok(value) => value,
                Err(error) => {
                    if let Some(registry) = &self.registry {
                        registry
                            .mark_unavailable(&self.server_id, error.to_string())
                            .await;
                    }
                    return Err(error);
                }
            };
            let result: McpCallToolResult = match serde_json::from_value(value) {
                Ok(result) => result,
                Err(error) => {
                    if let Some(registry) = &self.registry {
                        registry
                            .mark_unavailable(&self.server_id, error.to_string())
                            .await;
                    }
                    return Err(error.into());
                }
            };
            if result.is_error {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.exposed_name.clone(),
                    error: format_mcp_content(&result.content),
                });
            }
            Ok(ToolOutput {
                description: format_mcp_content(&result.content),
                truncated: OutputTruncation::empty(),
                output_file: PathBuf::new(),
                exit_code: None,
                timed_out: false,
                runtime_events: Vec::new(),
            })
        })
    }
}

pub(super) fn format_mcp_content(content: &[Value]) -> String {
    if content.is_empty() {
        return String::new();
    }
    let parts = content
        .iter()
        .map(format_mcp_content_part)
        .collect::<Vec<_>>();
    parts.join("\n")
}

fn format_mcp_content_part(content: &Value) -> String {
    let Some(object) = content.as_object() else {
        return compact_json(content);
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| compact_json(content)),
        Some("json") => object
            .get("json")
            .map(compact_json)
            .unwrap_or_else(|| compact_json(content)),
        _ => compact_json(&Value::Object(object.clone())),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}
