use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use pretty_assertions::assert_eq;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::service::{RequestContext, SubscriptionContext};
use rmcp::{ClientLifecycleMode, ClientServiceExt, ErrorData as McpError, RoleServer, ServiceExt};
use serde_json::{Map, Value, json};

use super::{ConnectedMcp, McpConnector, McpResetScope, McpRuntime};
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
            .install(ToolGroupId::new("mcp"), lease.agent_tools(None))
            .expect("install MCP tools");
        Self { runtime, closed }
    }

    pub(crate) async fn shutdown(self) {
        self.runtime.shutdown().await;
        wait_for_closed(&self.closed).await;
    }
}

#[derive(Debug, Clone)]
struct TestServer {
    tools: Vec<rmcp::model::Tool>,
}

#[derive(Debug, Clone)]
struct MutableToolServer {
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
    ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_ {
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
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2026_07_28)
        .with_server_info(Implementation::new("pl-test-mcp", "1.0.0"))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, McpError>> + Send + '_ {
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
    ) -> impl Future<Output = std::result::Result<CallToolResponse, McpError>> + Send + '_ {
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
    ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_ {
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
    ) -> impl Future<Output = std::result::Result<ReadResourceResponse, McpError>> + Send + '_ {
        async move {
            Ok(
                ReadResourceResult::new(vec![ResourceContents::text("resource body", request.uri)])
                    .into(),
            )
        }
    }
}

fn test_tool(name: &str) -> rmcp::model::Tool {
    annotated_tool(name, ToolAnnotations::new().read_only(true))
}

fn annotated_tool(name: &str, annotations: ToolAnnotations) -> rmcp::model::Tool {
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

fn plain_tool(name: &str) -> rmcp::model::Tool {
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

async fn test_connection(tools: Vec<rmcp::model::Tool>, closed: Arc<AtomicBool>) -> ConnectedMcp {
    let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
    let server = TestServer { tools };
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

async fn mutable_test_connection(
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

struct InstalledMcp {
    manager: ToolManager,
    plan: ToolPlan,
    runtime: super::McpRuntimeHandle,
    closed: Arc<AtomicBool>,
}

async fn installed_runtime(
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
        .install(ToolGroupId::new("mcp"), lease.agent_tools(None))
        .expect("install MCP tools");
    let plan = tools.freeze();
    InstalledMcp {
        manager,
        plan,
        runtime,
        closed,
    }
}

#[tokio::test]
async fn manager_plan_exposes_mcp_tools_and_audit_pipeline() {
    let installed = installed_runtime(vec![test_tool("lookup")], None).await;

    let binding = installed
        .plan
        .binding("mcp__docs__lookup")
        .expect("published MCP tool");
    // 无显式配置时 readOnlyHint=true 推导 Read。
    let tool = binding.tool();
    assert_eq!(tool.effect(), Some(ToolEffect::Read));
    assert!(tool.supports_parallel_tool_calls());
    assert_eq!(tool.runtime_lock_policy(), ToolRuntimeLockPolicy::Shared);
    assert_eq!(
        tool.cache_policy(&json!({})),
        crate::tool::cache::ToolCachePolicy::Never
    );
    let display = tool.display_metadata().expect("MCP display metadata");
    assert_eq!(
        display
            .annotations
            .as_ref()
            .and_then(|value| value.get("readOnlyHint")),
        Some(&Value::Bool(true))
    );
    assert!(display.icons.is_some());
    assert!(display.metadata.is_some());
    assert!(matches!(
        tool.spec(),
        pl_protocol::ToolSpec::Function {
            output_schema: Some(_),
            ..
        }
    ));

    let output = installed
        .manager
        .execute(
            &installed.plan,
            "mcp__docs__lookup",
            tool_input(json!({ "query": "meaning" })),
            test_context(),
        )
        .await
        .expect("execute MCP tool");
    assert!(output.canonical_output().contains("\"answer\":42"));
    let audit = audit_metadata(&output.runtime_events);
    assert_eq!(audit["result"]["structuredContent"]["answer"], 42);
    assert_eq!(audit["result"]["_meta"]["auditId"], "audit-1");
    assert_eq!(audit["result"]["resultType"], "complete");

    installed.runtime.shutdown().await;
    wait_for_closed(&installed.closed).await;
}

#[tokio::test]
async fn untrusted_annotations_keep_conservative_defaults() {
    // 无显式配置且只有 destructiveHint：不映射写 effect，保持 None（保守）。
    let installed = installed_runtime(
        vec![annotated_tool(
            "destructive",
            ToolAnnotations::new().destructive(true),
        )],
        None,
    )
    .await;

    let binding = installed
        .plan
        .binding("mcp__docs__destructive")
        .expect("published MCP tool");
    let tool = binding.tool();
    assert_eq!(tool.effect(), None);
    assert!(!tool.supports_parallel_tool_calls());
    assert_eq!(tool.runtime_lock_policy(), ToolRuntimeLockPolicy::Exclusive);

    installed.runtime.shutdown().await;
    wait_for_closed(&installed.closed).await;
}

#[tokio::test]
async fn explicit_server_effect_overrides_remote_hints() {
    // 无 annotation 的工具 + 显式配置 Read：优先配置。
    let installed = installed_runtime(vec![plain_tool("configured")], Some(ToolEffect::Read)).await;

    let binding = installed
        .plan
        .binding("mcp__docs__configured")
        .expect("published MCP tool");
    assert_eq!(binding.tool().effect(), Some(ToolEffect::Read));

    installed.runtime.shutdown().await;
    wait_for_closed(&installed.closed).await;
}

#[tokio::test]
async fn mcp_error_result_keeps_structured_audit_and_failed_terminal_marker() {
    let installed = installed_runtime(vec![test_tool("fail")], None).await;

    let output = installed
        .manager
        .execute(
            &installed.plan,
            "mcp__docs__fail",
            tool_input(json!({})),
            test_context(),
        )
        .await
        .expect("typed MCP error remains an auditable output");
    assert!(
        output
            .canonical_output()
            .starts_with("Tool execution error: ")
    );
    assert!(
        output
            .runtime_events
            .contains(&ToolDirective::ExecutionFailed)
    );
    assert_eq!(
        audit_metadata(&output.runtime_events)["result"]["structuredContent"]["code"],
        "TEST_FAILURE"
    );

    installed.runtime.shutdown().await;
    wait_for_closed(&installed.closed).await;
}

#[tokio::test]
async fn resource_facades_are_published_and_use_rmcp_typed_resource_api() {
    let installed = installed_runtime(vec![test_tool("lookup")], None).await;

    let names = installed.plan.names().collect::<Vec<_>>();
    for facade in [
        "list_mcp_resources",
        "list_mcp_resource_templates",
        "read_mcp_resource",
    ] {
        assert!(names.contains(&facade), "missing {facade} in {names:?}");
    }
    let output = installed
        .manager
        .execute(
            &installed.plan,
            "read_mcp_resource",
            tool_input(json!({ "server": "docs", "uri": "mcp://docs/one" })),
            test_context(),
        )
        .await
        .expect("read resource");
    let value: Value = serde_json::from_str(&output.canonical_output()).expect("resource JSON");
    assert_eq!(value["contents"][0]["text"], "resource body");

    installed.runtime.shutdown().await;
    wait_for_closed(&installed.closed).await;
}

#[tokio::test]
async fn generation_replacement_changes_new_leases_without_mutating_old_ones() {
    let first_closed = Arc::new(AtomicBool::new(false));
    let second_closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([
        (
            "docs".to_string(),
            test_connection(vec![test_tool("first")], first_closed.clone()).await,
        ),
        (
            "docs".to_string(),
            test_connection(vec![test_tool("second")], second_closed.clone()).await,
        ),
    ]);
    let runtime = McpRuntime::new(connector).handle();
    let servers = BTreeMap::from([("docs".to_string(), config("docs", None))]);
    runtime.reconcile(servers.clone()).await.expect("first");

    let first = runtime.acquire_turn_lease().await.expect("first lease");
    assert_eq!(first.tools()[0].exposed_name, "mcp__docs__first");

    runtime
        .reset(McpResetScope::All, servers)
        .await
        .expect("second");
    let second = runtime.acquire_turn_lease().await.expect("second lease");
    assert_eq!(second.tools()[0].exposed_name, "mcp__docs__second");
    assert_eq!(first.tools()[0].exposed_name, "mcp__docs__first");
    assert!(second.generation() > first.generation());

    drop(first);
    drop(second);
    runtime.shutdown().await;
    wait_for_closed(&first_closed).await;
    wait_for_closed(&second_closed).await;
}

#[tokio::test]
async fn tool_list_changed_atomically_publishes_the_next_generation() {
    let closed = Arc::new(AtomicBool::new(false));
    let (connection, tool_changes, tools, _fail_list, _list_calls) =
        mutable_test_connection(vec![test_tool("first")], closed.clone()).await;
    let connector = McpConnector::testing([("docs".to_string(), connection)]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([(
            "docs".to_string(),
            config("docs", Some(ToolEffect::Read)),
        )]))
        .await
        .expect("initial MCP generation");
    let first = runtime.acquire_turn_lease().await.expect("first lease");
    assert_eq!(first.tools()[0].raw_name, "first");

    *tools
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = vec![test_tool("second")];
    let mut updates = runtime.subscribe();
    tool_changes.notify_one();
    tokio::time::timeout(Duration::from_secs(5), updates.recv())
        .await
        .expect("MCP refresh update timeout")
        .expect("MCP refresh update");

    let second = runtime.acquire_turn_lease().await.expect("second lease");
    assert!(second.generation() > first.generation());
    assert_eq!(second.tools()[0].raw_name, "second");
    assert_eq!(first.tools()[0].raw_name, "first");

    drop(first);
    drop(second);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn failed_tool_list_changed_refresh_preserves_the_current_generation() {
    let closed = Arc::new(AtomicBool::new(false));
    let (connection, tool_changes, _tools, fail_list, list_calls) =
        mutable_test_connection(vec![test_tool("stable")], closed.clone()).await;
    let connector = McpConnector::testing([("docs".to_string(), connection)]);
    let runtime = McpRuntime::new(connector).handle();
    runtime
        .reconcile(BTreeMap::from([(
            "docs".to_string(),
            config("docs", Some(ToolEffect::Read)),
        )]))
        .await
        .expect("initial MCP generation");
    let first = runtime.acquire_turn_lease().await.expect("stable lease");
    let calls_before_refresh = list_calls.load(Ordering::SeqCst);
    fail_list.store(true, Ordering::SeqCst);
    tool_changes.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        while list_calls.load(Ordering::SeqCst) == calls_before_refresh {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("failing MCP refresh timeout");

    let current = runtime.acquire_turn_lease().await.expect("current lease");
    assert_eq!(current.generation(), first.generation());
    assert_eq!(current.tools()[0].raw_name, "stable");

    drop(first);
    drop(current);
    runtime.shutdown().await;
    wait_for_closed(&closed).await;
}

#[tokio::test]
async fn retired_generation_closes_only_after_last_lease_releases() {
    let first_closed = Arc::new(AtomicBool::new(false));
    let second_closed = Arc::new(AtomicBool::new(false));
    let connector = McpConnector::testing([
        (
            "docs".to_string(),
            test_connection(vec![test_tool("first")], first_closed.clone()).await,
        ),
        (
            "docs".to_string(),
            test_connection(vec![test_tool("second")], second_closed.clone()).await,
        ),
    ]);
    let runtime = McpRuntime::new(connector).handle();
    let servers = BTreeMap::from([("docs".to_string(), config("docs", None))]);
    runtime.reconcile(servers.clone()).await.expect("first");
    let first = runtime.acquire_turn_lease().await.expect("first lease");
    runtime
        .reset(McpResetScope::All, servers)
        .await
        .expect("second");
    let second = runtime.acquire_turn_lease().await.expect("second lease");
    assert_eq!(first.tools()[0].raw_name, "first");
    assert_eq!(second.tools()[0].raw_name, "second");
    assert!(!first_closed.load(Ordering::SeqCst));

    drop(first);
    wait_for_closed(&first_closed).await;
    assert!(!second_closed.load(Ordering::SeqCst));
    drop(second);
    runtime.shutdown().await;
    wait_for_closed(&second_closed).await;
}

fn config(server_id: &str, effect: Option<ToolEffect>) -> EffectiveMcpServerConfig {
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

fn json_object(value: Value) -> Map<String, Value> {
    value.as_object().expect("JSON object").clone()
}

fn tool_input(arguments: Value) -> ToolInput {
    ToolInput { arguments }
}

fn test_context() -> ToolCallContext {
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    ToolCallContext::test(event_tx)
}

fn audit_metadata(events: &[ToolDirective]) -> &Value {
    events
        .iter()
        .find_map(|event| match event {
            ToolDirective::AuditMetadata { metadata } => Some(metadata),
            _ => None,
        })
        .expect("MCP audit metadata")
}

async fn wait_for_closed(closed: &AtomicBool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !closed.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("MCP service closed");
}
