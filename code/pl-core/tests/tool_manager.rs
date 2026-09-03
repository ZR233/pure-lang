use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pl_core::{
    AgentToolSet, AgentWorkspace, AskUserTool, CompleteTool, DynTool, DynamicToolExecutor,
    GlobalToolInheritance, HostedWebSearchTool, StatPathTool, StaticTool, StaticToolDefinition,
    ToolCallContext, ToolCallIdentity, ToolDefinition, ToolDirective, ToolExecution, ToolGroupId,
    ToolInput, ToolInstallGroup, ToolManager, ToolName, ToolPolicy, ToolResult, ToolWorkspace,
    WriteFileTool,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug)]
struct ExternalLookupTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ExternalLookupInput {
    /// Business key to look up.
    #[schemars(length(min = 1, max = 128))]
    key: String,
}

impl StaticTool for ExternalLookupTool {
    type Input = ExternalLookupInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::bare("external_lookup").expect("valid static tool name"),
            "Look up a value from an embedding application.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
    }

    fn execute(
        &self,
        input: Self::Input,
        _context: ToolCallContext,
    ) -> impl Future<Output = pl_core::Result<ToolResult>> + Send {
        std::future::ready(ToolResult::json(serde_json::json!({
            "found": true,
            "key": input.key,
        })))
    }
}

fn external_agent_tools(manager: &ToolManager) -> AgentToolSet {
    manager.agent_tool_set("external-agent", GlobalToolInheritance::Isolated)
}

#[test]
fn downstream_can_select_and_register_public_builtin_tools() {
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    let workspace = ToolWorkspace::new(AgentWorkspace::local(std::env::temp_dir()));

    tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new("selected-builtins"),
            vec![
                AskUserTool.into(),
                CompleteTool.into(),
                StatPathTool::new(workspace.clone()).into(),
                WriteFileTool::new(workspace).into(),
            ],
        ))
        .expect("public built-ins can be selected independently");

    assert_eq!(
        tools.freeze().names().collect::<Vec<_>>(),
        vec!["complete", "request_user_input", "stat_path", "write_file",]
    );
}

#[tokio::test]
async fn external_tool_is_registered_snapshotted_and_executed_only_from_the_plan() {
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new("embedding-application"),
            vec![ExternalLookupTool.into()],
        ))
        .expect("register external tool");
    let plan = tools.freeze();

    assert_eq!(plan.names().collect::<Vec<_>>(), vec!["external_lookup"]);
    assert_eq!(plan.specs()[0].name(), "external_lookup");
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let output = manager
        .execute(
            &plan,
            "external_lookup",
            ToolInput {
                arguments: serde_json::json!({"key": "answer"}),
            },
            ToolCallContext::new(
                ToolCallIdentity {
                    call_id: "call-1".to_string(),
                    item_id: "item-1".to_string(),
                    agent_id: "external-agent".to_string(),
                    session_id: "session-1".to_string(),
                    turn_id: "turn-1".to_string(),
                    ..ToolCallIdentity::default()
                },
                event_tx,
            ),
        )
        .await
        .expect("execute through frozen plan");

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.canonical_output())
            .expect("canonical JSON result"),
        serde_json::json!({"found": true, "key": "answer"})
    );
}

#[tokio::test]
async fn manager_rejects_a_plan_from_another_runtime() {
    let owner = ToolManager::new();
    let tools = external_agent_tools(&owner);
    tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new("embedding-application"),
            vec![ExternalLookupTool.into()],
        ))
        .expect("register external tool");
    let foreign = ToolManager::new();
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

    let error = foreign
        .execute(
            &tools.freeze(),
            "external_lookup",
            ToolInput {
                arguments: serde_json::json!({"key": "answer"}),
            },
            ToolCallContext::new(ToolCallIdentity::default(), event_tx),
        )
        .await
        .expect_err("foreign manager must reject the plan");

    assert!(error.to_string().contains("different ToolManager"));
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum LookupMode {
    Exact,
    Prefix,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuilderLookupInput {
    /// Business key to look up.
    #[schemars(length(min = 1, max = 128))]
    key: String,
    /// Lookup matching mode.
    mode: LookupMode,
    /// Maximum records to return.
    #[schemars(range(min = 1, max = 50))]
    limit: Option<u8>,
}

#[tokio::test]
async fn public_builder_generates_schema_and_rejects_invalid_input_before_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = calls.clone();
    let tool = pl_core::static_tool::<BuilderLookupInput>(StaticToolDefinition::new(
        ToolName::namespaced("host", "builder_lookup").unwrap(),
        "Look up host application data.",
    ))
    .policy(ToolPolicy::read_only())
    .build(move |input, _context| {
        let handler_calls = handler_calls.clone();
        async move {
            handler_calls.fetch_add(1, Ordering::SeqCst);
            ToolResult::json(serde_json::json!({
                "key": input.key,
                "limit": input.limit,
                "mode": match input.mode {
                    LookupMode::Exact => "exact",
                    LookupMode::Prefix => "prefix",
                }
            }))
        }
    });
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new("embedding-application"),
            vec![tool],
        ))
        .unwrap();
    let plan = tools.freeze();
    let pl_protocol::ToolSpec::Function { input_schema, .. } = &plan.specs()[0] else {
        panic!("builder must create a function tool");
    };
    assert_eq!(input_schema["required"], serde_json::json!(["key", "mode"]));
    assert_eq!(
        input_schema["properties"]["key"]["description"],
        "Business key to look up."
    );
    assert_eq!(input_schema["properties"]["key"]["minLength"], 1);
    assert_eq!(input_schema["properties"]["key"]["maxLength"], 128);
    assert_eq!(input_schema["properties"]["limit"]["minimum"], 1);
    assert_eq!(input_schema["properties"]["limit"]["maximum"], 50);
    assert_eq!(
        input_schema["$defs"]["LookupMode"]["enum"],
        serde_json::json!(["exact", "prefix"])
    );
    assert_eq!(input_schema["additionalProperties"], false);

    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let error = manager
        .execute(
            &plan,
            "host__builder_lookup",
            ToolInput {
                arguments: serde_json::json!({
                    "key": "answer",
                    "mode": "exact",
                    "unexpected": true,
                }),
            },
            ToolCallContext::new(ToolCallIdentity::default(), event_tx),
        )
        .await
        .expect_err("unknown input must be rejected before the typed handler");
    assert!(error.to_string().contains("unknown field `unexpected`"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn static_dynamic_and_hosted_tools_share_one_frozen_plan() {
    let dynamic_definition = ToolDefinition::function(
        StaticToolDefinition::new(
            ToolName::namespaced("mcp", "lookup").unwrap(),
            "Runtime-discovered lookup.",
        ),
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        }),
    );
    let dynamic = DynTool::new_executor(DynamicToolExecutor::new(
        dynamic_definition,
        ToolPolicy::read_only(),
        ToolExecution::Local,
        |_invocation| async { Ok(ToolResult::success("dynamic")) },
    ));
    let hosted = DynTool::new_executor(HostedWebSearchTool::deepseek());
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    tools
        .install(ToolInstallGroup::direct(
            ToolGroupId::new("mixed"),
            vec![ExternalLookupTool.into(), dynamic, hosted],
        ))
        .unwrap();
    let plan = tools.freeze();

    assert_eq!(
        plan.names().collect::<Vec<_>>(),
        vec!["external_lookup", "mcp__lookup", "web_search"]
    );
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let output = manager
        .execute(
            &plan,
            "mcp__lookup",
            ToolInput {
                arguments: serde_json::json!({}),
            },
            ToolCallContext::new(ToolCallIdentity::default(), event_tx),
        )
        .await
        .unwrap();
    assert_eq!(output.canonical_output(), "dynamic");
}

#[tokio::test]
async fn deferred_tools_reveal_for_next_plan_and_generation_changes_invalidate_state() {
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    tools
        .install(
            ToolInstallGroup::deferred(ToolGroupId::new("mcp"), vec![ExternalLookupTool.into()])
                .with_developer_instructions(
                    "Use the revealed lookup only for host business keys.",
                ),
        )
        .unwrap();
    let initial = tools.freeze();
    assert_eq!(initial.names().collect::<Vec<_>>(), vec!["tool_search"]);
    let (event_tx, _) = tokio::sync::broadcast::channel(8);
    let search = manager
        .execute(
            &initial,
            "tool_search",
            ToolInput {
                arguments: serde_json::json!({"query": "external lookup"}),
            },
            ToolCallContext::new(ToolCallIdentity::default(), event_tx),
        )
        .await
        .unwrap();
    let reveal = search
        .runtime_events
        .iter()
        .find_map(|event| match event {
            ToolDirective::RevealTools {
                catalog_fingerprint,
                tool_names,
            } => Some(pl_core::ToolDiscoveryState {
                catalog_fingerprint: Some(catalog_fingerprint.clone()),
                revealed_tool_names: tool_names.clone(),
            }),
            _ => None,
        })
        .expect("tool_search reveal directive");
    let revealed = tools.freeze_with_discovery(&reveal);
    assert_eq!(
        revealed.names().collect::<Vec<_>>(),
        vec!["external_lookup", "tool_search"]
    );
    assert_eq!(
        revealed.developer_instructions().collect::<Vec<_>>()[0].1,
        "Use the revealed lookup only for host business keys."
    );

    tools
        .install(ToolInstallGroup::deferred(
            ToolGroupId::new("mcp"),
            vec![
                pl_core::static_tool::<ExternalLookupInput>(StaticToolDefinition::new(
                    ToolName::bare("replacement_lookup").unwrap(),
                    "Replacement lookup.",
                ))
                .build(|input, _context| async move {
                    ToolResult::json(serde_json::json!({"key": input.key}))
                }),
            ],
        ))
        .unwrap();
    let replacement = tools.freeze_with_discovery(&reveal);
    assert_eq!(replacement.names().collect::<Vec<_>>(), vec!["tool_search"]);
    assert_ne!(
        replacement.catalog_fingerprint(),
        reveal.catalog_fingerprint.as_deref()
    );
}
