use std::path::PathBuf;
use std::pin::Pin;

use futures::Future;
use pl_protocol::{PureError, Result};
use serde_json::Value;

use crate::tool::{OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};
use crate::turn::ToolEffect;

use super::contract::McpCallRequest;
use super::runtime::{McpRuntimeToolDescriptor, McpTurnLease};
use super::wire::McpCallToolResult;

#[derive(Debug, Clone)]
pub(super) struct McpLeaseToolAdapter {
    lease: McpTurnLease,
    descriptor: McpRuntimeToolDescriptor,
}

impl McpLeaseToolAdapter {
    pub(super) fn new(lease: McpTurnLease, descriptor: McpRuntimeToolDescriptor) -> Self {
        Self { lease, descriptor }
    }
}

impl Tool for McpLeaseToolAdapter {
    fn name(&self) -> &str {
        &self.descriptor.exposed_name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn input_schema(&self) -> Value {
        self.descriptor.input_schema.clone()
    }

    fn effect(&self) -> Option<ToolEffect> {
        self.descriptor.effect
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>> {
        Box::pin(async move {
            let value = self
                .lease
                .call_tool(
                    self.descriptor.server_id.clone(),
                    McpCallRequest {
                        name: self.descriptor.raw_name.clone(),
                        arguments: input.arguments,
                    },
                )
                .await?;
            let result: McpCallToolResult = serde_json::from_value(value).map_err(|error| {
                self.lease.mark_unavailable(
                    self.descriptor.server_id.clone(),
                    format!("invalid MCP tools/call response: {error}"),
                );
                PureError::from(error)
            })?;
            if result.is_error {
                return Err(PureError::ToolExecutionFailed {
                    tool: self.descriptor.exposed_name.clone(),
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
