use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};

use super::contract::{
    McpCallRequest, McpConnectRequest, McpRuntimeHost, McpSession, McpToolDefinition,
};
use super::tool_adapter::format_mcp_content;
use super::transport::{HttpMcpClient, McpStderrSeverity, classify_mcp_stderr_line};
use super::{McpAvailabilityKind, McpRuntime, is_mcp_tool_name};
use crate::config::{
    EffectiveMcpServerConfig, McpServerConfig, McpServerMutationPolicy, McpServerSourceKind,
    McpServerStatusKind, McpServerTransport,
};
use crate::turn::ToolEffect;

#[derive(Clone, Default)]
struct FakeHost {
    state: Arc<Mutex<FakeHostState>>,
}

#[derive(Default)]
struct FakeHostState {
    definitions: BTreeMap<String, FakeServerDefinition>,
    connect_count: usize,
    shutdown_count: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct FakeServerDefinition {
    tools: Vec<McpToolDefinition>,
    response: String,
    fail_calls: Arc<AtomicBool>,
    connect_gate: Option<Arc<Notify>>,
}

struct FakeSession {
    definition: FakeServerDefinition,
    shutdown_count: Arc<AtomicUsize>,
}

impl McpRuntimeHost for FakeHost {
    type Error = io::Error;
    type Session = FakeSession;

    async fn connect(&self, request: McpConnectRequest) -> io::Result<Self::Session> {
        let (definition, shutdown_count) = {
            let mut state = self.state.lock().await;
            state.connect_count += 1;
            let definition = state
                .definitions
                .get(&request.server_id)
                .cloned()
                .ok_or_else(|| io::Error::other("missing fake server"))?;
            (definition, state.shutdown_count.clone())
        };
        if let Some(gate) = &definition.connect_gate {
            gate.notified().await;
        }
        Ok(FakeSession {
            definition,
            shutdown_count,
        })
    }
}

impl McpSession for FakeSession {
    type Error = io::Error;

    async fn list_tools(&self) -> io::Result<Vec<McpToolDefinition>> {
        Ok(self.definition.tools.clone())
    }

    async fn call_tool(&self, _request: McpCallRequest) -> io::Result<Value> {
        if self.definition.fail_calls.load(Ordering::SeqCst) {
            return Err(io::Error::other("transport failed"));
        }
        Ok(json!({
            "content": [{"type": "text", "text": self.definition.response}],
            "isError": false
        }))
    }

    async fn list_resources(&self, cursor: Option<String>) -> io::Result<Value> {
        Ok(json!({ "resources": [], "cursor": cursor }))
    }

    async fn list_resource_templates(&self, cursor: Option<String>) -> io::Result<Value> {
        Ok(json!({ "resourceTemplates": [], "cursor": cursor }))
    }

    async fn read_resource(&self, uri: String) -> io::Result<Value> {
        Ok(json!({ "uri": uri, "text": self.definition.response }))
    }

    async fn shutdown(&self) {
        self.shutdown_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl FakeHost {
    async fn define_raw(&self, server_id: &str, tools: Vec<McpToolDefinition>) {
        self.state.lock().await.definitions.insert(
            server_id.to_string(),
            FakeServerDefinition {
                tools,
                response: "ok".to_string(),
                fail_calls: Arc::new(AtomicBool::new(false)),
                connect_gate: None,
            },
        );
    }

    async fn define(&self, server_id: &str, response: &str, fail_calls: Arc<AtomicBool>) {
        self.define_tools(server_id, response, fail_calls, ["read/page"])
            .await;
    }

    async fn define_tools(
        &self,
        server_id: &str,
        response: &str,
        fail_calls: Arc<AtomicBool>,
        tools: impl IntoIterator<Item = &'static str>,
    ) {
        self.define_tools_with_gate(server_id, response, fail_calls, tools, None)
            .await;
    }

    async fn define_tools_with_gate(
        &self,
        server_id: &str,
        response: &str,
        fail_calls: Arc<AtomicBool>,
        tools: impl IntoIterator<Item = &'static str>,
        connect_gate: Option<Arc<Notify>>,
    ) {
        self.state.lock().await.definitions.insert(
            server_id.to_string(),
            FakeServerDefinition {
                tools: tools
                    .into_iter()
                    .map(|name| McpToolDefinition {
                        name: name.to_string(),
                        description: Some(format!("Tool {name}")),
                        input_schema: json!({ "type": "object" }),
                    })
                    .collect(),
                response: response.to_string(),
                fail_calls,
                connect_gate,
            },
        );
    }

    async fn connect_count(&self) -> usize {
        self.state.lock().await.connect_count
    }

    async fn shutdown_count(&self) -> usize {
        self.state
            .lock()
            .await
            .shutdown_count
            .load(Ordering::SeqCst)
    }
}

#[tokio::test]
async fn discovery_filters_are_applied_by_pl_before_exposed_names_are_assigned() {
    let host = FakeHost::default();
    host.define_tools(
        "filtered",
        "ok",
        Arc::new(AtomicBool::new(false)),
        ["keep", "deny", "other"],
    )
    .await;
    let runtime = McpRuntime::new(host);
    let handle = runtime.handle();
    let mut server = effective_server(
        "filtered",
        McpServerStatusKind::Enabled,
        McpServerSourceKind::User,
        "v1",
    );
    server.config.enabled_tools = Some(vec!["keep".to_string(), "deny".to_string()]);
    server.config.disabled_tools = vec!["deny".to_string()];

    handle
        .reconcile(BTreeMap::from([("filtered".to_string(), server)]))
        .await
        .unwrap();
    let lease = handle.acquire_turn_lease().await.unwrap();

    assert_eq!(
        lease
            .tools()
            .iter()
            .map(|tool| tool.raw_name.as_str())
            .collect::<Vec<_>>(),
        vec!["keep"]
    );
}

#[test]
fn zero_runtime_timeout_is_rejected_before_connecting() {
    let mut server = McpServerConfig {
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://example.com/mcp".to_string()),
        startup_timeout_secs: Some(0),
        ..Default::default()
    };
    assert!(server.validate("future").is_err());

    server.startup_timeout_secs = Some(1);
    server.tool_timeout_secs = Some(0);
    assert!(server.validate("future").is_err());
}

fn effective_server(
    id: &str,
    status_kind: McpServerStatusKind,
    source_kind: McpServerSourceKind,
    revision: &str,
) -> EffectiveMcpServerConfig {
    EffectiveMcpServerConfig {
        id: id.to_string(),
        config: McpServerConfig {
            transport: McpServerTransport::StreamableHttp,
            url: Some(format!("https://example.com/{revision}")),
            ..Default::default()
        },
        source_kind,
        source_label: "Test".to_string(),
        source_detail: None,
        status_kind,
        status_message: None,
        mutation_policy: McpServerMutationPolicy::UserEditable,
        bearer_token: None,
        tool_effect: (source_kind == McpServerSourceKind::BuiltIn).then_some(ToolEffect::Read),
    }
}

#[test]
fn format_mcp_content_prefers_text_parts() {
    let content = vec![
        json!({"type": "text", "text": "hello"}),
        json!({"type": "json", "json": {"ok": true}}),
    ];

    assert_eq!(format_mcp_content(&content), "hello\n{\"ok\":true}");
}

#[test]
fn mcp_stderr_classification_filters_only_informational_lines() {
    assert_eq!(
        classify_mcp_stderr_line(r#"{"level":"INFO","message":"Running"}"#),
        McpStderrSeverity::Info
    );
    assert_eq!(
        classify_mcp_stderr_line("[2026-06-24] WARN: retrying"),
        McpStderrSeverity::Warning
    );
    assert_eq!(
        classify_mcp_stderr_line("child process exited unexpectedly"),
        McpStderrSeverity::Error
    );
}

#[test]
fn http_client_uses_bearer_token_override() {
    let server = McpServerConfig {
        transport: McpServerTransport::StreamableHttp,
        url: Some("https://example.com/mcp".to_string()),
        bearer_token_env_var: Some("IGNORED_ENV_VAR".to_string()),
        ..Default::default()
    };

    let client =
        HttpMcpClient::new("zhipu_search", &server, Some("coding-plan-key".to_string())).unwrap();

    assert_eq!(client.bearer_token.as_deref(), Some("coding-plan-key"));
}

#[tokio::test]
async fn generation_is_atomic_and_active_lease_keeps_old_session() {
    let host = FakeHost::default();
    host.define("future server", "v1", Arc::new(AtomicBool::new(false)))
        .await;
    let runtime = McpRuntime::new(host.clone());
    let handle = runtime.handle();
    handle
        .reconcile(BTreeMap::from([(
            "future server".to_string(),
            effective_server(
                "future server",
                McpServerStatusKind::Enabled,
                McpServerSourceKind::User,
                "v1",
            ),
        )]))
        .await
        .unwrap();
    let old = handle.acquire_turn_lease().await.unwrap();
    assert_eq!(old.tools()[0].exposed_name, "mcp__future_server__read_page");

    host.define("future server", "v2", Arc::new(AtomicBool::new(false)))
        .await;
    handle
        .reconcile(BTreeMap::from([(
            "future server".to_string(),
            effective_server(
                "future server",
                McpServerStatusKind::Enabled,
                McpServerSourceKind::User,
                "v2",
            ),
        )]))
        .await
        .unwrap();
    let current = handle.acquire_turn_lease().await.unwrap();

    let old_value = old
        .call_tool(
            "future server".to_string(),
            McpCallRequest {
                name: "read/page".to_string(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap();
    let current_value = current
        .call_tool(
            "future server".to_string(),
            McpCallRequest {
                name: "read/page".to_string(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap();

    assert_eq!(old_value["content"][0]["text"], "v1");
    assert_eq!(current_value["content"][0]["text"], "v2");
    assert_eq!(host.connect_count().await, 2);
    drop(old);
    tokio::task::yield_now().await;
    assert_eq!(host.shutdown_count().await, 1);
    drop(current);
    handle.shutdown().await;
    assert_eq!(host.shutdown_count().await, 2);
}

#[tokio::test]
async fn active_lease_calls_continue_while_next_generation_is_preparing() {
    let host = FakeHost::default();
    host.define("future", "v1", Arc::new(AtomicBool::new(false)))
        .await;
    let handle = McpRuntime::new(host.clone()).handle();
    handle
        .reconcile(BTreeMap::from([(
            "future".to_string(),
            effective_server(
                "future",
                McpServerStatusKind::Enabled,
                McpServerSourceKind::User,
                "v1",
            ),
        )]))
        .await
        .unwrap();
    let old = handle.acquire_turn_lease().await.unwrap();
    let gate = Arc::new(Notify::new());
    host.define_tools_with_gate(
        "future",
        "v2",
        Arc::new(AtomicBool::new(false)),
        ["read/page"],
        Some(gate.clone()),
    )
    .await;
    let reconcile = tokio::spawn({
        let handle = handle.clone();
        async move {
            handle
                .reconcile(BTreeMap::from([(
                    "future".to_string(),
                    effective_server(
                        "future",
                        McpServerStatusKind::Enabled,
                        McpServerSourceKind::User,
                        "v2",
                    ),
                )]))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while host.connect_count().await < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let value = tokio::time::timeout(
        Duration::from_millis(250),
        old.call_tool(
            "future".to_string(),
            McpCallRequest {
                name: "read/page".to_string(),
                arguments: json!({}),
            },
        ),
    )
    .await
    .expect("old generation calls must not wait for reconcile")
    .unwrap();

    assert_eq!(value["content"][0]["text"], "v1");
    gate.notify_one();
    reconcile.await.unwrap().unwrap();
    drop(old);
    handle.shutdown().await;
}

#[tokio::test]
async fn unchanged_server_is_reused_and_builtin_tools_are_read_effect() {
    let host = FakeHost::default();
    host.define("zhipu", "ok", Arc::new(AtomicBool::new(false)))
        .await;
    let handle = McpRuntime::new(host.clone()).handle();
    let config = effective_server(
        "zhipu",
        McpServerStatusKind::Enabled,
        McpServerSourceKind::BuiltIn,
        "same",
    );
    handle
        .reconcile(BTreeMap::from([("zhipu".to_string(), config.clone())]))
        .await
        .unwrap();
    handle
        .reconcile(BTreeMap::from([("zhipu".to_string(), config)]))
        .await
        .unwrap();
    let lease = handle.acquire_turn_lease().await.unwrap();

    assert_eq!(host.connect_count().await, 1);
    assert_eq!(lease.tools()[0].effect, Some(ToolEffect::Read));
    assert!(is_mcp_tool_name(&lease.tools()[0].exposed_name));
    drop(lease);
    handle.shutdown().await;
}

#[tokio::test]
async fn runtime_normalizes_tool_schema_before_exposing_generation() {
    let host = FakeHost::default();
    host.define_raw(
        "schema",
        vec![McpToolDefinition {
            name: "lookup".to_string(),
            description: None,
            input_schema: serde_json::json!({ "type": "object" }),
        }],
    )
    .await;
    let runtime = McpRuntime::new(host);
    let handle = runtime.handle();
    handle
        .reconcile(BTreeMap::from([(
            "schema".to_string(),
            effective_server(
                "schema",
                McpServerStatusKind::Enabled,
                McpServerSourceKind::User,
                "command-a",
            ),
        )]))
        .await
        .unwrap();

    let lease = handle.acquire_turn_lease().await.unwrap();

    assert_eq!(
        lease.tools()[0].input_schema,
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    );
}

#[tokio::test]
async fn failing_server_is_removed_from_new_leases_without_polluting_others() {
    let host = FakeHost::default();
    let failing = Arc::new(AtomicBool::new(true));
    host.define("broken", "unused", failing).await;
    host.define("healthy", "ok", Arc::new(AtomicBool::new(false)))
        .await;
    let handle = McpRuntime::new(host).handle();
    handle
        .reconcile(BTreeMap::from([
            (
                "broken".to_string(),
                effective_server(
                    "broken",
                    McpServerStatusKind::Enabled,
                    McpServerSourceKind::User,
                    "broken",
                ),
            ),
            (
                "healthy".to_string(),
                effective_server(
                    "healthy",
                    McpServerStatusKind::Enabled,
                    McpServerSourceKind::User,
                    "healthy",
                ),
            ),
        ]))
        .await
        .unwrap();
    let lease = handle.acquire_turn_lease().await.unwrap();
    let error = lease
        .call_tool(
            "broken".to_string(),
            McpCallRequest {
                name: "read/page".to_string(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("transport failed"));

    let next = handle.acquire_turn_lease().await.unwrap();
    assert!(next.tools().iter().all(|tool| tool.server_id == "healthy"));
    let snapshots = handle.snapshots().await;
    assert_eq!(
        snapshots["broken"].availability_kind,
        McpAvailabilityKind::Unavailable
    );
    assert_eq!(
        snapshots["healthy"].availability_kind,
        McpAvailabilityKind::Available
    );
    let repeated = lease
        .call_tool(
            "broken".to_string(),
            McpCallRequest {
                name: "read/page".to_string(),
                arguments: json!({}),
            },
        )
        .await
        .unwrap_err();
    assert!(repeated.to_string().contains("unavailable"));
    drop(lease);
    drop(next);
    handle.shutdown().await;
}

#[tokio::test]
async fn disabled_server_is_visible_in_health_without_connecting() {
    let host = FakeHost::default();
    let handle = McpRuntime::new(host.clone()).handle();
    handle
        .reconcile(BTreeMap::from([(
            "draft".to_string(),
            effective_server(
                "draft",
                McpServerStatusKind::Disabled,
                McpServerSourceKind::User,
                "disabled",
            ),
        )]))
        .await
        .unwrap();

    assert_eq!(host.connect_count().await, 0);
    assert_eq!(
        handle.snapshots().await["draft"].availability_kind,
        McpAvailabilityKind::Disabled
    );
    handle.shutdown().await;
}
