use std::sync::Arc;

use futures::FutureExt;
use pl_core::{
    AgentToolSet, GlobalToolInheritance, Tool, ToolCallContext, ToolCallIdentity, ToolEffect,
    ToolGroupId, ToolInput, ToolManager, ToolResult,
};

#[derive(Debug)]
struct ExternalLookupTool;

impl Tool for ExternalLookupTool {
    fn name(&self) -> &str {
        "external_lookup"
    }

    fn description(&self) -> &str {
        "Look up a value from an embedding application."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {"key": {"type": "string"}},
            "required": ["key"],
            "additionalProperties": false,
        })
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        _context: ToolCallContext,
    ) -> futures::future::BoxFuture<'a, pl_core::Result<ToolResult>> {
        async move {
            ToolResult::json(serde_json::json!({
                "found": true,
                "key": input.arguments["key"],
            }))
        }
        .boxed()
    }
}

fn external_agent_tools(manager: &ToolManager) -> AgentToolSet {
    manager.agent_tool_set("external-agent", GlobalToolInheritance::Isolated)
}

#[tokio::test]
async fn external_tool_is_registered_snapshotted_and_executed_only_from_the_plan() {
    let manager = ToolManager::new();
    let tools = external_agent_tools(&manager);
    tools
        .install(
            ToolGroupId::new("embedding-application"),
            vec![Arc::new(ExternalLookupTool)],
        )
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
        .install(
            ToolGroupId::new("embedding-application"),
            vec![Arc::new(ExternalLookupTool)],
        )
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
