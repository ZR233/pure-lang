use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput};

pub const TOOL_LIST_MCP_RESOURCES: &str = "list_mcp_resources";
pub const TOOL_LIST_MCP_RESOURCE_TEMPLATES: &str = "list_mcp_resource_templates";
pub const TOOL_READ_MCP_RESOURCE: &str = "read_mcp_resource";

/// MCP resource 工具的模型可见协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpResourceToolKind {
    ListResources,
    ListResourceTemplates,
    ReadResource,
}

impl McpResourceToolKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::ListResources,
            Self::ListResourceTemplates,
            Self::ReadResource,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ListResources => TOOL_LIST_MCP_RESOURCES,
            Self::ListResourceTemplates => TOOL_LIST_MCP_RESOURCE_TEMPLATES,
            Self::ReadResource => TOOL_READ_MCP_RESOURCE,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().copied().find(|kind| kind.name() == name)
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ListResources => "List resources provided by MCP servers.",
            Self::ListResourceTemplates => "List resource templates provided by MCP servers.",
            Self::ReadResource => "Read a specific resource from an MCP server.",
        }
    }

    pub fn input_schema(self) -> Value {
        match self {
            Self::ListResources | Self::ListResourceTemplates => object_schema(vec![
                ("server", json!({ "type": "string" }), false),
                ("cursor", json!({ "type": "string" }), false),
            ]),
            Self::ReadResource => object_schema(vec![
                ("server", json!({ "type": "string" }), true),
                ("uri", json!({ "type": "string" }), true),
            ]),
        }
    }

    pub fn to_schema(self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

/// MCP resource 工具的宿主后端。
///
/// pl-core 负责 schema、输入解析、trace 和 tool result history；宿主只提供
/// 当前 agent 可见的 MCP/skill resource 查询能力。
pub trait McpResourceBackend: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn list_resources(
        &self,
        request: McpListResourcesRequest,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn list_resource_templates(
        &self,
        request: McpListResourceTemplatesRequest,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;

    fn read_resource(
        &self,
        request: McpReadResourceRequest,
    ) -> impl Future<Output = std::result::Result<Value, Self::Error>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpListResourcesRequest {
    pub server: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpListResourceTemplatesRequest {
    pub server: Option<String>,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpReadResourceRequest {
    pub server: String,
    pub uri: String,
}

#[derive(Debug, Clone)]
pub struct McpResourceTool<B> {
    kind: McpResourceToolKind,
    backend: Arc<B>,
}

impl<B> McpResourceTool<B> {
    pub fn new(kind: McpResourceToolKind, backend: Arc<B>) -> Self {
        Self { kind, backend }
    }
}

impl<B> Tool for McpResourceTool<B>
where
    B: McpResourceBackend + 'static,
{
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> Value {
        self.kind.input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolContext,
    ) -> BoxFuture<'a, pl_protocol::Result<ToolOutput>> {
        Box::pin(async move {
            let value = match self.kind {
                McpResourceToolKind::ListResources => {
                    let args: ListResourcesArgs =
                        serde_json::from_value(input.arguments).map_err(|error| {
                            invalid_input(self.kind.name(), format!("invalid input: {error}"))
                        })?;
                    self.backend
                        .list_resources(McpListResourcesRequest {
                            server: args.server,
                            cursor: args.cursor,
                        })
                        .await
                        .map_err(|error| tool_error(self.kind.name(), error))?
                }
                McpResourceToolKind::ListResourceTemplates => {
                    let args: ListResourcesArgs =
                        serde_json::from_value(input.arguments).map_err(|error| {
                            invalid_input(self.kind.name(), format!("invalid input: {error}"))
                        })?;
                    self.backend
                        .list_resource_templates(McpListResourceTemplatesRequest {
                            server: args.server,
                            cursor: args.cursor,
                        })
                        .await
                        .map_err(|error| tool_error(self.kind.name(), error))?
                }
                McpResourceToolKind::ReadResource => {
                    let args: ReadResourceArgs =
                        serde_json::from_value(input.arguments).map_err(|error| {
                            invalid_input(self.kind.name(), format!("invalid input: {error}"))
                        })?;
                    self.backend
                        .read_resource(McpReadResourceRequest {
                            server: args.server,
                            uri: args.uri,
                        })
                        .await
                        .map_err(|error| tool_error(self.kind.name(), error))?
                }
            };
            json_output(self.kind.name(), value)
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListResourcesArgs {
    server: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadResourceArgs {
    server: String,
    uri: String,
}

fn json_output(tool: &str, value: Value) -> pl_protocol::Result<ToolOutput> {
    let description = serde_json::to_string(&value).map_err(|error| {
        tool_error(
            tool,
            format!("failed to serialize MCP resource output: {error}"),
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

fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

fn invalid_input(tool: &str, error: impl Into<String>) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.into(),
    }
}

fn object_schema(fields: Vec<(&str, Value, bool)>) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for (name, schema, is_required) in fields {
        properties.insert(name.to_string(), schema);
        if is_required {
            required.push(Value::String(name.to_string()));
        }
    }
    let mut object = serde_json::Map::new();
    object.insert("type".to_string(), Value::String("object".to_string()));
    object.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        object.insert("required".to_string(), Value::Array(required));
    }
    object.insert("additionalProperties".to_string(), Value::Bool(false));
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use pretty_assertions::assert_eq;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeBackend {
        calls: Mutex<Vec<String>>,
        fail: Option<DisplayError>,
    }

    #[derive(Debug, Clone)]
    struct DisplayError(&'static str);

    impl std::fmt::Display for DisplayError {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.0)
        }
    }

    impl McpResourceBackend for FakeBackend {
        type Error = DisplayError;

        async fn list_resources(
            &self,
            request: McpListResourcesRequest,
        ) -> std::result::Result<Value, Self::Error> {
            if let Some(error) = self.fail.clone() {
                return Err(error);
            }
            self.calls
                .lock()
                .expect("lock")
                .push(format!("list:{:?}:{:?}", request.server, request.cursor));
            Ok(json!({ "resources": [{ "uri": "mcp://one" }] }))
        }

        async fn list_resource_templates(
            &self,
            request: McpListResourceTemplatesRequest,
        ) -> std::result::Result<Value, Self::Error> {
            if let Some(error) = self.fail.clone() {
                return Err(error);
            }
            self.calls.lock().expect("lock").push(format!(
                "templates:{:?}:{:?}",
                request.server, request.cursor
            ));
            Ok(json!({ "resourceTemplates": [{ "uriTemplate": "mcp://{id}" }] }))
        }

        async fn read_resource(
            &self,
            request: McpReadResourceRequest,
        ) -> std::result::Result<Value, Self::Error> {
            if let Some(error) = self.fail.clone() {
                return Err(error);
            }
            self.calls
                .lock()
                .expect("lock")
                .push(format!("read:{}:{}", request.server, request.uri));
            Ok(json!({ "contents": [{ "uri": request.uri, "text": "hello" }] }))
        }
    }

    #[tokio::test]
    async fn dispatches_each_resource_tool_through_backend() {
        let backend = Arc::new(FakeBackend::default());
        let output = McpResourceTool::new(McpResourceToolKind::ReadResource, backend.clone())
            .execute(
                ToolInput {
                    arguments: json!({ "server": "docs", "uri": "mcp://one" }),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .expect("execute");

        assert_eq!(
            serde_json::from_str::<Value>(&output.description).expect("json"),
            json!({ "contents": [{ "uri": "mcp://one", "text": "hello" }] })
        );
        assert_eq!(
            backend.calls.lock().expect("lock").clone(),
            vec!["read:docs:mcp://one".to_string()]
        );
    }

    #[tokio::test]
    async fn rejects_unknown_arguments() {
        let backend = Arc::new(FakeBackend::default());
        let error = McpResourceTool::new(McpResourceToolKind::ListResources, backend)
            .execute(
                ToolInput {
                    arguments: json!({ "server": "docs", "extra": true }),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .expect_err("invalid input");

        assert!(error.to_string().contains("unknown field"));
    }

    #[tokio::test]
    async fn maps_backend_display_error_to_current_resource_tool() {
        let backend = Arc::new(FakeBackend {
            calls: Mutex::new(Vec::new()),
            fail: Some(DisplayError("broker unavailable")),
        });
        let error = McpResourceTool::new(McpResourceToolKind::ReadResource, backend)
            .execute(
                ToolInput {
                    arguments: json!({ "server": "docs", "uri": "mcp://one" }),
                    session_id: "session".to_string(),
                    tool_id: "tool".to_string(),
                    revision_base: 0,
                },
                test_context(),
            )
            .await
            .expect_err("backend should fail");

        assert!(matches!(
            error,
            PureError::ToolExecutionFailed { tool, error }
                if tool == TOOL_READ_MCP_RESOURCE && error == "broker unavailable"
        ));
    }

    fn test_context() -> ToolContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolContext {
            event_tx,
            options: crate::TurnOptions::default(),
            workspace_access: crate::tool::WorkspaceAccess::WorkspaceOnly,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            instruction_snapshot: None,
            provider_call_id: None,
            active_subagent: None,
            lsp_runtime: None,
            parent_session: Arc::new(crate::AgentSession::new()),
        }
    }
}
