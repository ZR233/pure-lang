//! MCP resource façade declarations and Thread-local execution.
use super::{
    McpTurnLease,
    runtime::ResourceOperation,
    thread::{bound_text, project_content},
};
use crate::media::ToolMediaHost;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
    },
};
use pl_protocol::{PureError, Result};
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

const TOOL_LIST_MCP_RESOURCES: &str = "list_mcp_resources";
const TOOL_LIST_MCP_RESOURCE_TEMPLATES: &str = "list_mcp_resource_templates";
const TOOL_READ_MCP_RESOURCE: &str = "read_mcp_resource";

/// Resource operations exposed independently from individual remote MCP tools.
#[derive(Debug, Clone, Copy)]
pub enum McpResourceToolKind {
    ListResources,
    ListResourceTemplates,
    ReadResource,
}

impl McpResourceToolKind {
    /// All stable resource façade operations.
    pub fn all() -> &'static [Self] {
        &[
            Self::ListResources,
            Self::ListResourceTemplates,
            Self::ReadResource,
        ]
    }

    /// Stable tool identity, independent of service generation and health.
    pub fn name(self) -> &'static str {
        match self {
            Self::ListResources => TOOL_LIST_MCP_RESOURCES,
            Self::ListResourceTemplates => TOOL_LIST_MCP_RESOURCE_TEMPLATES,
            Self::ReadResource => TOOL_READ_MCP_RESOURCE,
        }
    }

    /// Model-facing operation description.
    pub fn description(self) -> &'static str {
        match self {
            Self::ListResources => "List resources provided by MCP servers.",
            Self::ListResourceTemplates => "List resource templates provided by MCP servers.",
            Self::ReadResource => "Read a specific resource from an MCP server.",
        }
    }

    /// Tool-owned schema; core never interprets these argument fields.
    pub fn input_schema(self) -> Value {
        match self {
            Self::ListResources | Self::ListResourceTemplates => serde_json::json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "cursor": { "type": "string" }
                },
                "additionalProperties": false
            }),
            Self::ReadResource => serde_json::json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "uri": { "type": "string" }
                },
                "required": ["server", "uri"],
                "additionalProperties": false
            }),
        }
    }

    pub(super) fn parse(self, arguments: Value) -> Result<(Option<String>, ResourceOperation)> {
        match self {
            Self::ListResources => {
                let arguments: ListResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    arguments.server,
                    ResourceOperation::ListResources {
                        cursor: arguments.cursor,
                    },
                ))
            }
            Self::ListResourceTemplates => {
                let arguments: ListResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    arguments.server,
                    ResourceOperation::ListResourceTemplates {
                        cursor: arguments.cursor,
                    },
                ))
            }
            Self::ReadResource => {
                let arguments: ReadResourceArguments = parse_resource_arguments(self, arguments)?;
                Ok((
                    Some(arguments.server),
                    ResourceOperation::ReadResource { uri: arguments.uri },
                ))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListResourceArguments {
    server: Option<String>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadResourceArguments {
    server: String,
    uri: String,
}

fn parse_resource_arguments<T: for<'de> Deserialize<'de>>(
    kind: McpResourceToolKind,
    arguments: Value,
) -> Result<T> {
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: kind.name().to_string(),
        error: format!("invalid input: {error}"),
    })
}

/// One Thread's resource façade, retaining service leases until in-flight queries finish.
#[derive(Debug)]
pub struct ThreadMcpResourceTool<H> {
    authorization: pl_core::tool::opaque::ToolAuthorization,
    lease: Mutex<Option<McpTurnLease>>,
    kind: McpResourceToolKind,
    media: Arc<H>,
}

impl<H: ToolMediaHost> ThreadMcpResourceTool<H> {
    /// Creates an instance over resource-capable services in this frozen generation.
    pub fn new(lease: McpTurnLease, kind: McpResourceToolKind, media: Arc<H>) -> Self {
        Self {
            authorization: lease.authorization(),
            lease: Mutex::new(Some(lease)),
            kind,
            media,
        }
    }

    /// Returns stable declaration material for model-side encoding.
    pub fn declaration(&self) -> pl_protocol::ToolSpec {
        pl_protocol::ToolSpec::function(
            self.kind.name(),
            self.kind.description(),
            self.kind.input_schema(),
        )
        .allow_programmatic(serde_json::json!({"type":"object", "additionalProperties":true}))
    }

    /// Transfers an instance without granting it framework control permissions.
    ///
    /// # Errors
    /// Returns an invalid registry identity error.
    pub fn registration(
        self,
        declaration: OpaquePayload,
    ) -> std::result::Result<Registration, RegistryError> {
        let authorization = self.authorization.clone();
        Ok(
            Registration::new(self.kind.name().into(), declaration, self)?
                .deferred()
                .with_authorization(authorization),
        )
    }
}

impl<H: ToolMediaHost> Tool for ThreadMcpResourceTool<H> {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> std::result::Result<ToolOutput, ToolError> {
        if input.format() != "application/json" || input.version() != 1 {
            return Err(resource_error("unsupported MCP resource argument encoding"));
        }
        let (server, operation) = self
            .kind
            .parse(serde_json::from_str(input.content()).map_err(ToolError::new)?)
            .map_err(ToolError::new)?;
        if context.cancellation.is_cancelled() {
            return Err(resource_error(
                "MCP resource query cancelled before dispatch",
            ));
        }
        let lease = self
            .lease
            .lock()
            .map_err(|_| resource_error("resource lease lock poisoned"))?
            .clone()
            .ok_or_else(|| resource_error("MCP resource tool is closed"))?;
        let value = lease
            .resource_query(server, operation)
            .await
            .map_err(ToolError::new)?;
        match self.kind {
            McpResourceToolKind::ListResources | McpResourceToolKind::ListResourceTemplates => {
                let encoded = serde_json::to_string(&value).map_err(ToolError::new)?;
                let mut context = vec![ContextContent::Text {
                    text: Arc::from(encoded.as_str()),
                }];
                bound_text(&mut context);
                Ok(ToolOutput::new(
                    OpaquePayload::new("pl.tool.mcp-resources", 1, encoded)
                        .map_err(ToolError::new)?,
                    context,
                ))
            }
            McpResourceToolKind::ReadResource => {
                project_resource(
                    value,
                    self.media.as_ref(),
                    context.model_projection.as_ref(),
                )
                .await
            }
        }
    }

    async fn close(&self) -> std::result::Result<(), ToolError> {
        let lease = self
            .lease
            .lock()
            .map_err(|_| resource_error("resource lease lock poisoned"))?
            .take();
        drop(lease);
        Ok(())
    }
}

async fn project_resource(
    mut value: Value,
    host: &impl ToolMediaHost,
    projection: Option<&OpaquePayload>,
) -> std::result::Result<ToolOutput, ToolError> {
    let projected = async {
        let parsed: rmcp::model::ReadResourceResult =
            serde_json::from_value(value.clone()).map_err(ToolError::new)?;
        let result = rmcp::model::CallToolResult::success(
            parsed
                .contents
                .into_iter()
                .map(rmcp::model::ContentBlock::resource)
                .collect(),
        );
        let (payload, context) = project_content(&result, host, projection).await?;
        let projected: Value = serde_json::from_str(payload.content()).map_err(ToolError::new)?;
        let contents = projected
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| resource_error("projected MCP resources lack content"))?
            .iter()
            .map(|content| {
                content
                    .get("resource")
                    .cloned()
                    .ok_or_else(|| resource_error("projected MCP resource envelope changed"))
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok::<_, ToolError>((contents, context))
    }
    .await;
    match projected {
        Ok((contents, context)) => {
            value
                .as_object_mut()
                .ok_or_else(|| resource_error("MCP resource result is not an object"))?
                .insert("contents".into(), Value::Array(contents));
            Ok(ToolOutput::new(
                OpaquePayload::new(
                    "pl.tool.mcp-resources",
                    1,
                    serde_json::to_string(&value).map_err(ToolError::new)?,
                )
                .map_err(ToolError::new)?,
                context,
            ))
        }
        Err(source) => {
            let output = ToolOutput::new(
                OpaquePayload::new(
                    "pl.tool.mcp-resources-unarchived",
                    1,
                    serde_json::to_string(&value).map_err(ToolError::new)?,
                )
                .map_err(ToolError::new)?,
                vec![ContextContent::Text {
                    text: Arc::from(format!(
                        "MCP resource was received but could not be prepared: {source}"
                    )),
                }],
            );
            Err(source.with_output(output))
        }
    }
}

fn resource_error(message: &str) -> ToolError {
    ToolError::new(std::io::Error::other(message.to_owned()))
}
