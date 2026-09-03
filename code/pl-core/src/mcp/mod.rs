mod connector;
mod health;
mod naming;
mod output;
mod runtime;

pub use connector::{ConnectedMcp, McpConnectRequest, McpConnector};
pub use health::{McpAvailabilityKind, McpAvailabilitySnapshot};
pub use output::McpImageOutputContext;
pub use runtime::{
    McpGeneration, McpResetScope, McpRuntime, McpRuntimeHandle, McpRuntimeToolDescriptor,
    McpTurnLease,
};

const MCP_TOOL_PREFIX: &str = "mcp__";

pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

/// MCP 测试共享基建：内存 rmcp server、连接工厂与 `McpTestHarness`。
/// mcp 行为测试与 core 的工具执行测试共同使用。
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use rmcp::handler::server::ServerHandler;
    use rmcp::model::*;
    use rmcp::service::{RequestContext, SubscriptionContext};
    use rmcp::{
        ClientLifecycleMode, ClientServiceExt, ErrorData as McpError, RoleServer, ServiceExt,
    };
    use serde_json::{Map, Value, json};

    use super::{ConnectedMcp, McpConnector, McpRuntime};
    use crate::config::*;
    use crate::tool::*;
    use crate::turn::ToolEffect;

    pub(crate) struct McpTestHarness {
        runtime: super::McpRuntimeHandle,
        closed: Arc<AtomicBool>,
    }

    impl McpTestHarness {
        /// 把带只读 annotation 的 MCP server 工具安装到该 agent 的工具集合。
        pub(crate) async fn install_read_tool(core: &mut crate::TurnEngine) -> Self {
            let closed = Arc::new(AtomicBool::new(false));
            let connector = McpConnector::testing([(
                "docs".to_string(),
                test_connection(vec![test_tool("lookup")], closed.clone()).await,
            )]);
            let runtime = McpRuntime::new(connector).handle();
            runtime
                .reconcile(BTreeMap::from([(
                    "docs".to_string(),
                    config("docs", Some(ToolEffect::Read)),
                )]))
                .await
                .expect("reconcile test MCP runtime");
            let lease = runtime.acquire_turn_lease().await.expect("MCP lease");
            core.agent_tools()
                .install(ToolInstallGroup::direct(
                    ToolGroupId::new("mcp"),
                    lease.agent_tools(None).expect("construct MCP tools"),
                ))
                .expect("install MCP tools");
            Self { runtime, closed }
        }

        pub(crate) async fn shutdown(self) {
            self.runtime.shutdown().await;
            wait_for_closed(&self.closed).await;
        }
    }

    #[derive(Debug, Clone)]
    pub(crate) struct TestServer {
        tools: Vec<rmcp::model::Tool>,
        supports_resources: bool,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct MutableToolServer {
        tools: Arc<std::sync::RwLock<Vec<rmcp::model::Tool>>>,
        fail_list: Arc<AtomicBool>,
        list_calls: Arc<AtomicUsize>,
        tool_changes: Arc<tokio::sync::Notify>,
    }

    impl ServerHandler for MutableToolServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(
                ServerCapabilities::builder()
                    .enable_tools()
                    .enable_tool_list_changed()
                    .build(),
            )
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(Implementation::new("pl-mutable-test-mcp", "1.0.0"))
        }

        fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_
        {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            let fail = self.fail_list.load(Ordering::SeqCst);
            let tools = self
                .tools
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            async move {
                if fail {
                    Err(McpError::internal_error(
                        "injected tools/list failure",
                        None,
                    ))
                } else {
                    Ok(ListToolsResult {
                        tools,
                        ..Default::default()
                    })
                }
            }
        }

        fn accepted_subscription_filter(
            &self,
            requested: &SubscriptionFilter,
        ) -> Option<SubscriptionFilter> {
            Some(requested.supported_by(&self.get_info().capabilities))
        }

        fn listen(
            &self,
            context: SubscriptionContext,
        ) -> impl Future<Output = std::result::Result<(), McpError>> + Send + '_ {
            let tool_changes = self.tool_changes.clone();
            async move {
                loop {
                    tokio::select! {
                        () = tool_changes.notified() => {
                            if context.sink().notify_tool_list_changed().await.is_err() {
                                break;
                            }
                        }
                        () = context.cancelled() => break,
                    }
                }
                Ok(())
            }
        }
    }

    #[expect(
        clippy::manual_async_fn,
        reason = "RPITIT keeps the required Send bound explicit"
    )]
    impl ServerHandler for TestServer {
        fn get_info(&self) -> ServerInfo {
            let capabilities = if self.supports_resources {
                ServerCapabilities::builder()
                    .enable_tools()
                    .enable_resources()
                    .build()
            } else {
                ServerCapabilities::builder().enable_tools().build()
            };
            ServerInfo::new(capabilities)
                .with_protocol_version(ProtocolVersion::V_2026_07_28)
                .with_server_info(Implementation::new("pl-test-mcp", "1.0.0"))
        }

        fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_
        {
            let tools = self.tools.clone();
            async move {
                Ok(ListToolsResult {
                    tools,
                    ..Default::default()
                })
            }
        }

        fn call_tool(
            &self,
            request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<CallToolResponse, McpError>> + Send + '_
        {
            async move {
                let mut meta = Map::new();
                meta.insert("auditId".to_string(), json!("audit-1"));
                let result = if request.name == "fail" {
                    CallToolResult::structured_error(json!({
                        "code": "TEST_FAILURE",
                        "details": { "retryable": false }
                    }))
                } else {
                    CallToolResult::structured(json!({
                        "answer": 42,
                        "arguments": request.arguments
                    }))
                }
                .with_meta(Some(MetaObject(meta)));
                Ok(result.into())
            }
        }

        fn list_resources(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_
        {
            async {
                Ok(ListResourcesResult {
                    resources: vec![Resource::new("mcp://docs/one", "one")],
                    ..Default::default()
                })
            }
        }

        fn list_resource_templates(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<ListResourceTemplatesResult, McpError>> + Send + '_
        {
            async {
                Ok(ListResourceTemplatesResult {
                    resource_templates: vec![ResourceTemplate::new("mcp://docs/{id}", "docs")],
                    ..Default::default()
                })
            }
        }

        fn read_resource(
            &self,
            request: ReadResourceRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> impl Future<Output = std::result::Result<ReadResourceResponse, McpError>> + Send + '_
        {
            async move {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(
                    "resource body",
                    request.uri,
                )])
                .into())
            }
        }
    }

    pub(crate) fn test_tool(name: &str) -> rmcp::model::Tool {
        annotated_tool(name, ToolAnnotations::new().read_only(true))
    }

    pub(crate) fn annotated_tool(name: &str, annotations: ToolAnnotations) -> rmcp::model::Tool {
        let mut tool = rmcp::model::Tool::new(
            name.to_string(),
            format!("{name} description"),
            json_object(json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "additionalProperties": false
            })),
        );
        tool.output_schema = Some(Arc::new(json_object(json!({
            "type": "object",
            "properties": { "answer": { "type": "integer" } }
        }))));
        tool.annotations = Some(annotations);
        tool.icons = Some(vec![Icon::new("data:image/png;base64,AA==")]);
        tool.meta = Some(MetaObject(Map::from_iter([(
            "displayGroup".to_string(),
            json!("tests"),
        )])));
        tool
    }

    pub(crate) fn plain_tool(name: &str) -> rmcp::model::Tool {
        rmcp::model::Tool::new(
            name.to_string(),
            format!("{name} description"),
            json_object(json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })),
        )
    }

    pub(crate) async fn test_connection(
        tools: Vec<rmcp::model::Tool>,
        closed: Arc<AtomicBool>,
    ) -> ConnectedMcp {
        test_connection_with_resources(tools, closed, true).await
    }

    pub(crate) async fn test_connection_with_resources(
        tools: Vec<rmcp::model::Tool>,
        closed: Arc<AtomicBool>,
        supports_resources: bool,
    ) -> ConnectedMcp {
        let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
        let server = TestServer {
            tools,
            supports_resources,
        };
        tokio::spawn(async move {
            let service = server
                .serve(server_transport)
                .await
                .expect("serve test MCP server");
            let _ = service.waiting().await;
            closed.store(true, Ordering::SeqCst);
        });
        let client = super::connector::McpClientHandler::without_notifications(
            ClientInfo::new(
                ClientCapabilities::default(),
                Implementation::new("pl-test-client", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2026_07_28),
        )
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("discover test MCP server");
        ConnectedMcp::from_running(client).await
    }

    pub(crate) async fn mutable_test_connection(
        tools: Vec<rmcp::model::Tool>,
        closed: Arc<AtomicBool>,
    ) -> (
        ConnectedMcp,
        Arc<tokio::sync::Notify>,
        Arc<std::sync::RwLock<Vec<rmcp::model::Tool>>>,
        Arc<AtomicBool>,
        Arc<AtomicUsize>,
    ) {
        let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
        let tools = Arc::new(std::sync::RwLock::new(tools));
        let fail_list = Arc::new(AtomicBool::new(false));
        let list_calls = Arc::new(AtomicUsize::new(0));
        let tool_changes = Arc::new(tokio::sync::Notify::new());
        let server = MutableToolServer {
            tools: tools.clone(),
            fail_list: fail_list.clone(),
            list_calls: list_calls.clone(),
            tool_changes: tool_changes.clone(),
        };
        tokio::spawn(async move {
            let service = server
                .serve(server_transport)
                .await
                .expect("serve mutable MCP server");
            let _ = service.waiting().await;
            closed.store(true, Ordering::SeqCst);
        });
        let client = super::connector::McpClientHandler::without_notifications(
            ClientInfo::new(
                ClientCapabilities::default(),
                Implementation::new("pl-test-client", "1.0.0"),
            )
            .with_protocol_version(ProtocolVersion::V_2026_07_28),
        )
        .serve_with_lifecycle(
            client_transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .expect("discover mutable MCP server");
        let connection = ConnectedMcp::from_running(client).await;
        assert!(
            connection.has_tool_subscription().await,
            "modern list_changed server must establish a subscription"
        );
        (connection, tool_changes, tools, fail_list, list_calls)
    }

    pub(crate) struct InstalledMcp {
        pub(crate) manager: ToolManager,
        pub(crate) plan: ToolPlan,
        pub(crate) runtime: super::McpRuntimeHandle,
        pub(crate) closed: Arc<AtomicBool>,
    }

    pub(crate) async fn installed_runtime(
        tools: Vec<rmcp::model::Tool>,
        effect: Option<ToolEffect>,
    ) -> InstalledMcp {
        let closed = Arc::new(AtomicBool::new(false));
        let connector = McpConnector::testing([(
            "docs".to_string(),
            test_connection(tools, closed.clone()).await,
        )]);
        let runtime = McpRuntime::new(connector).handle();
        runtime
            .reconcile(BTreeMap::from([(
                "docs".to_string(),
                config("docs", effect),
            )]))
            .await
            .expect("reconcile test MCP runtime");
        let lease = runtime.acquire_turn_lease().await.expect("MCP lease");
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("mcp-test", GlobalToolInheritance::Isolated);
        tools
            .install(ToolInstallGroup::direct(
                ToolGroupId::new("mcp"),
                lease.agent_tools(None).expect("construct MCP tools"),
            ))
            .expect("install MCP tools");
        let plan = tools.freeze();
        InstalledMcp {
            manager,
            plan,
            runtime,
            closed,
        }
    }

    pub(crate) fn config(server_id: &str, effect: Option<ToolEffect>) -> EffectiveMcpServerConfig {
        EffectiveMcpServerConfig {
            id: server_id.to_string(),
            config: McpServerConfig {
                transport: McpServerTransport::StreamableHttp,
                url: Some("http://test.invalid/mcp".to_string()),
                ..McpServerConfig::default()
            },
            source_kind: McpServerSourceKind::User,
            source_label: "test".to_string(),
            source_detail: None,
            status_kind: McpServerStatusKind::Enabled,
            status_message: None,
            mutation_policy: McpServerMutationPolicy::UserEditable,
            bearer_token: None,
            tool_effect: effect,
        }
    }

    pub(crate) fn json_object(value: Value) -> Map<String, Value> {
        value.as_object().expect("JSON object").clone()
    }

    pub(crate) fn tool_input(arguments: Value) -> ToolInput {
        ToolInput { arguments }
    }

    pub(crate) fn test_context() -> ToolCallContext {
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx)
    }

    pub(crate) fn audit_metadata(events: &[ToolDirective]) -> &Value {
        events
            .iter()
            .find_map(|event| match event {
                ToolDirective::AuditMetadata { metadata } => Some(metadata),
                _ => None,
            })
            .expect("MCP audit metadata")
    }

    pub(crate) async fn wait_for_closed(closed: &AtomicBool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !closed.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("MCP service closed");
    }
}
