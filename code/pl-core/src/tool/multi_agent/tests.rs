use pl_protocol::{AgentStatus, InteractionResolution};
use pretty_assertions::assert_eq;
use std::sync::Arc;

use super::ForkTurns;
use super::agent_tool_records;
use super::child_agent_options;
use super::fork_session;
use super::json_output;
use super::types::{AgentToolRuntime, ListAgentsResult, WaitAgentResult};
use crate::agent::AgentRecord;
use crate::tool::{Tool, ToolContext, ToolRuntimeLockPolicy, WorkspaceAccess};
use crate::turn::CompileMode;

fn agent_record(id: &str, status: AgentStatus, error: Option<&str>) -> AgentRecord {
    AgentRecord {
        id: id.to_string(),
        path: format!("/root/{id}"),
        parent_path: Some("/root".to_string()),
        role: "executor".to_string(),
        task: format!("inspect {id}"),
        status,
        summary: None,
        error: error.map(str::to_string),
        reason: None,
        budget_limit_kind: None,
        budget_usage: None,
        depth: 1,
        updated_at: 1,
    }
}

#[test]
fn wait_agent_result_serializes_activity_message() {
    let output = json_output(WaitAgentResult {
        message: "wait_agent observed agent activity.".to_string(),
        timed_out: false,
    })
    .unwrap();
    let result = serde_json::from_str::<WaitAgentResult>(&output.description).unwrap();

    assert_eq!(
        result,
        WaitAgentResult {
            message: "wait_agent observed agent activity.".to_string(),
            timed_out: false,
        }
    );
}

#[test]
fn list_agents_result_round_trips_compact_agents() {
    let agents = vec![
        agent_record(
            "agent-1",
            AgentStatus::Interrupted,
            Some("provider returned status 429"),
        ),
        agent_record("agent-2", AgentStatus::Completed, None),
    ];
    let output = json_output(ListAgentsResult {
        agents: agent_tool_records(&agents),
    })
    .unwrap();
    let result = serde_json::from_str::<ListAgentsResult>(&output.description).unwrap();

    assert_eq!(
        result,
        ListAgentsResult {
            agents: agent_tool_records(&agents),
        }
    );
    assert_eq!(result.agents[0].path, "/root/agent-1");
    assert_eq!(result.agents[0].status, AgentStatus::Interrupted);
}

#[test]
fn fork_turns_filters_tool_history_and_reasoning() {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};

    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_calls".to_string(), "[]".to_string());
    let session = crate::CoreSession::from_messages(vec![
        Message {
            role: MessageRole::System,
            content: MessageContent::Text("system".to_string()),
            reasoning_content: Some("hidden".to_string()),
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("first".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("final one".to_string()),
            reasoning_content: Some("reasoning".to_string()),
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("calling tool".to_string()),
            reasoning_content: None,
            metadata: tool_metadata,
        },
        Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("large output".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("second".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
    ]);

    let forked = fork_session(&session, ForkTurns::All);

    assert_eq!(forked.len(), 4);
    assert_eq!(forked.messages()[0].role, MessageRole::System);
    assert_eq!(forked.messages()[2].role, MessageRole::Assistant);
    assert!(forked.messages().iter().all(|message| {
        message.reasoning_content.is_none() && !message.metadata.contains_key("tool_calls")
    }));
    assert!(
        !forked
            .messages()
            .iter()
            .any(|message| message.role == MessageRole::Tool)
    );
}

#[test]
fn fork_turns_last_n_keeps_recent_user_turns() {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};

    let session = crate::CoreSession::from_messages(vec![
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("first".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("first answer".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("second".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
    ]);

    let forked = fork_session(&session, ForkTurns::Last(1));

    assert_eq!(forked.len(), 1);
    assert_eq!(
        forked.messages()[0].content,
        MessageContent::Text("second".to_string())
    );
}

#[test]
fn child_agent_options_inherit_interaction_callback() {
    let callback: crate::InteractionCallback = Arc::new(|_interaction| {
        Box::pin(async {
            InteractionResolution::UserInput {
                answers: Default::default(),
            }
        })
    });
    let parent = crate::TurnOptions::default().with_interaction_callback(callback);

    let child = child_agent_options(&parent);

    assert!(child.interaction_callback.is_some());
}

#[test]
fn wait_agent_does_not_hold_runtime_lock() {
    let wait_agent = super::WaitAgentTool;

    assert_eq!(
        wait_agent.runtime_lock_policy(),
        ToolRuntimeLockPolicy::None
    );
}

#[derive(Debug)]
struct NoopAgentToolRegistrar;

impl crate::AgentToolRegistrar for NoopAgentToolRegistrar {
    fn register_tools<'a>(
        &'a self,
        _core: &'a mut crate::PureCore,
        _workspace_root: std::path::PathBuf,
        _workspace_instructions: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[test]
fn child_agent_run_spec_inherits_parent_tool_registrar() {
    let mut provider_info = pl_model::ProviderInfo::openai(Some("http://example.invalid".into()));
    provider_info.default_model = "test-model".to_string();
    let runtime = AgentToolRuntime::new(
        pl_model::create_provider(provider_info).unwrap(),
        None,
        None,
        None,
        None,
        None,
    );
    let registrar: Arc<dyn crate::AgentToolRegistrar> = Arc::new(NoopAgentToolRegistrar);
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let context = ToolContext {
        event_tx,
        options: crate::TurnOptions::default(),
        workspace_access: WorkspaceAccess::WorkspaceOnly,
        mode: CompileMode::Auto,
        workspace_root: std::env::temp_dir(),
        workspace_instructions: None,
        instruction_snapshot: None,
        provider_call_id: None,
        active_subagent: None,
        agent_supervisor: crate::AgentSupervisor::default(),
        agent_tool_registrar: Some(registrar.clone()),
        lsp_runtime: None,
        parent_session: Arc::new(crate::CoreSession::new()),
    };

    let run_spec = runtime.run_config(
        &context,
        crate::TurnOptions::default(),
        "call-1".to_string(),
        "work".to_string(),
        crate::CoreSession::new(),
    );

    assert!(Arc::ptr_eq(
        run_spec.tool_registrar.as_ref().expect("tool registrar"),
        &registrar
    ));
}
