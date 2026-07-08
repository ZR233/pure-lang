use std::sync::Arc;

use crate::config::ToolCapabilityConfig;
use crate::tool::{RegisteredTool, ToolRuntimeLockPolicy};
use pretty_assertions::assert_eq;
use tokio::sync::Mutex;

use super::*;

#[test]
fn agent_kernel_uses_registered_tools_as_product_extension_point() {
    let kernel_source = include_str!("../kernel.rs");

    assert!(
        kernel_source.contains(&format!("{}{}", "Registered", "Tool")),
        "AgentKernel 必须通过动态 RegisteredTool 暴露产品工具扩展点"
    );
    for old_extension in [
        format!("{}{}", "ProductTool", "Router"),
        format!("{}{}", "ProductTool", "Definition"),
        format!("{}{}", "ProductTool", "Request"),
        format!("{}{}", "with_product_tool", "_router"),
    ] {
        assert!(
            !kernel_source.contains(&old_extension),
            "AgentKernel 不应继续暴露旧产品工具扩展层 `{old_extension}`"
        );
    }
}

#[tokio::test]
async fn agent_kernel_registers_dynamic_tools() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_for_tool = calls.clone();
    let tool = RegisteredTool::new(
        "dynamic_echo",
        "Echo dynamic product input through a registered handler.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"],
            "additionalProperties": false
        }),
        move |input, _context| {
            let calls = calls_for_tool.clone();
            Box::pin(async move {
                let message = input
                    .arguments
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                calls.lock().await.push(message.clone());
                ToolOutput::json(serde_json::json!({
                    "echo": message,
                }))
            })
        },
    )
    .with_runtime_lock_policy(ToolRuntimeLockPolicy::Shared);
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(std::env::temp_dir()))
    .with_registered_tool(tool)
    .build()
    .await;

    assert_eq!(kernel.tool_names(), vec!["dynamic_echo".to_string()]);
    let registered = kernel
        .tool("dynamic_echo")
        .expect("dynamic tool registered");
    assert_eq!(
        registered.runtime_lock_policy(),
        ToolRuntimeLockPolicy::Shared
    );

    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let output = registered
        .execute(
            ToolInput {
                arguments: serde_json::json!({ "message": "hello" }),
                session_id: "session-1".to_string(),
                tool_id: "tool-1".to_string(),
                revision_base: 0,
            },
            test_tool_context(event_tx),
        )
        .await
        .unwrap();

    assert_eq!(output.description, "{\"echo\":\"hello\"}");
    assert_eq!(calls.lock().await.as_slice(), &["hello".to_string()]);
}

#[tokio::test]
async fn agent_kernel_registers_and_routes_product_tools_as_registered_tools() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let tool = product_echo_tool(calls.clone());
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(std::env::temp_dir()))
    .with_registered_tool(tool)
    .build()
    .await;

    assert_eq!(kernel.tool_names(), vec!["product_echo".to_string()]);

    let tool = kernel
        .tool("product_echo")
        .expect("product tool registered");
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let output = tool
        .execute(
            ToolInput {
                arguments: serde_json::json!({ "message": "hello" }),
                session_id: "session-1".to_string(),
                tool_id: "tool-1".to_string(),
                revision_base: 0,
            },
            test_tool_context(event_tx),
        )
        .await
        .unwrap();

    assert_eq!(output.description, "product:hello");
    assert_eq!(
        calls.lock().await.as_slice(),
        &["product_echo:hello".to_string()]
    );
}

#[tokio::test]
async fn agent_kernel_executes_registered_tool_with_kernel_context() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_for_tool = calls.clone();
    let workspace_root = std::env::temp_dir().join("pl-core-agent-kernel-tool-context");
    let tool = RegisteredTool::new(
        "context_echo",
        "Echo input and record kernel supplied context.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"],
            "additionalProperties": false
        }),
        move |input, context| {
            let calls = calls_for_tool.clone();
            async move {
                let message = input
                    .arguments
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                calls.lock().await.push(serde_json::json!({
                    "message": message,
                    "workspaceRoot": context.workspace_root,
                    "hasRegistrar": context.agent_tool_registrar.is_some(),
                    "sessionId": input.session_id,
                    "toolId": input.tool_id,
                }));
                ToolOutput::json(serde_json::json!({ "ok": true }))
            }
        },
    );
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(workspace_root.clone()))
    .with_registered_tool(tool)
    .build()
    .await;

    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let output = kernel
        .execute_tool(AgentKernelToolRequest::new(
            "context_echo",
            serde_json::json!({ "message": "hello" }),
            "session-1",
            "tool-1",
            event_tx,
        ))
        .await
        .unwrap();

    assert_eq!(output.description, "{\"ok\":true}");
    assert_eq!(
        calls.lock().await.as_slice(),
        &[serde_json::json!({
            "message": "hello",
            "workspaceRoot": workspace_root,
            "hasRegistrar": true,
            "sessionId": "session-1",
            "toolId": "tool-1",
        })]
    );
}

#[tokio::test]
async fn agent_kernel_host_profile_exposes_only_product_tools() {
    let tool = product_echo_tool(Arc::new(Mutex::new(Vec::new())));
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(std::env::temp_dir()))
    .with_registered_tool(tool)
    .build()
    .await;

    assert_eq!(kernel.tool_names(), vec!["product_echo".to_string()]);
    assert!(kernel.core().tools.get("bash").is_none());
    assert!(kernel.core().tools.get("read_file").is_none());
    assert!(kernel.core().tools.get("spawn_agent").is_none());
    assert!(kernel.core().tools.get("git_status").is_none());
}

#[tokio::test]
async fn agent_kernel_local_workspace_combines_shared_and_product_tools() {
    let tool = product_echo_tool(Arc::new(Mutex::new(Vec::new())));
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::local_workspace(std::env::temp_dir()))
    .with_registered_tool(tool)
    .build()
    .await;
    let names = kernel.tool_names();

    assert!(names.contains(&"bash".to_string()));
    assert!(names.contains(&"read_file".to_string()));
    assert!(names.contains(&"spawn_agent".to_string()));
    assert!(names.contains(&"product_echo".to_string()));
    assert!(!names.contains(&"git_status".to_string()));
}

#[tokio::test]
async fn agent_kernel_builder_registers_tool_set_and_replays_it_for_children() {
    let workspace_root = std::env::temp_dir().join("pl-core-agent-kernel-tool-set");
    let tool_set = ToolSetBuilder::from_capabilities(ToolCapabilityConfig {
        bash: false,
        workspace_files: false,
        skills: false,
        mcp: false,
        lsp: false,
        subagents: false,
        ask_user: true,
        git: false,
        docker: false,
        container: false,
    });
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(workspace_root.clone()))
    .with_tool_set(tool_set)
    .build()
    .await;

    assert!(kernel.tool("request_user_input").is_some());

    let registrar = kernel
        .agent_tool_registrar()
        .expect("kernel exposes child tool registrar");
    let mut child_core =
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .build();
    registrar
        .register_tools(&mut child_core, workspace_root, None)
        .await
        .unwrap();

    assert!(child_core.tools.get("request_user_input").is_some());
}

#[tokio::test]
async fn agent_kernel_registrar_rebuilds_product_tools_for_child_core() {
    let tool = product_echo_tool(Arc::new(Mutex::new(Vec::new())));
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(std::env::temp_dir()))
    .with_registered_tool(tool)
    .build()
    .await;
    let registrar = kernel.agent_tool_registrar().expect("agent tool registrar");
    let mut child_core =
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
            .unwrap()
            .build();

    registrar
        .register_tools(&mut child_core, std::env::temp_dir(), None)
        .await
        .unwrap();

    assert!(child_core.tools.get("product_echo").is_some());
    assert!(child_core.tools.get("bash").is_none());
}

fn product_echo_tool(calls: Arc<Mutex<Vec<String>>>) -> RegisteredTool {
    RegisteredTool::new(
        "product_echo",
        "Echo product input through a registered handler.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"],
            "additionalProperties": false
        }),
        move |input, _context| {
            let calls = calls.clone();
            async move {
                let message = input
                    .arguments
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                calls.lock().await.push(format!("product_echo:{message}"));
                Ok(ToolOutput {
                    description: format!("product:{message}"),
                    truncated: OutputTruncation::empty(),
                    output_file: std::path::PathBuf::new(),
                    exit_code: Some(0),
                    timed_out: false,
                    runtime_events: Vec::new(),
                })
            }
        },
    )
}
