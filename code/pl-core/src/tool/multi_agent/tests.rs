use pl_protocol::{AgentStatus, InteractionResolution};
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex};

use super::ForkTurns;
use super::ResumeAgentTool;
use super::agent_tool_records;
use super::child_agent_options;
use super::fork_session;
use super::json_output;
use super::types::{
    AgentToolRuntime, ListAgentsResult, SendInputResult, SpawnAgentResult, WaitAgentResult,
};
use crate::agent::AgentInputTurnMode;
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
fn agent_control_status_kind_maps_to_protocol_status() {
    use crate::tool::AgentControlStatusKind;

    assert_eq!(
        AgentControlStatusKind::Queued.to_agent_status(),
        AgentStatus::Queued
    );
    assert_eq!(
        AgentControlStatusKind::Running.to_agent_status(),
        AgentStatus::Running
    );
    assert_eq!(
        AgentControlStatusKind::Waiting.to_agent_status(),
        AgentStatus::Waiting
    );
    assert_eq!(
        AgentControlStatusKind::Completed.to_agent_status(),
        AgentStatus::Completed
    );
    assert_eq!(
        AgentControlStatusKind::Errored.to_agent_status(),
        AgentStatus::Errored
    );
    assert_eq!(
        AgentControlStatusKind::Interrupted.to_agent_status(),
        AgentStatus::Interrupted
    );
    assert_eq!(
        AgentControlStatusKind::Shutdown.to_agent_status(),
        AgentStatus::Shutdown
    );
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
fn agent_control_send_input_request_exposes_shared_turn_mode() {
    let queue_only: crate::tool::AgentControlSendInputRequest =
        serde_json::from_value(serde_json::json!({ "target": "agent-1", "message": "hold" }))
            .unwrap();
    let trigger_turn: crate::tool::AgentControlSendInputRequest = serde_json::from_value(
        serde_json::json!({ "target": "agent-1", "message": "go", "triggerTurn": true }),
    )
    .unwrap();
    let interrupt: crate::tool::AgentControlSendInputRequest = serde_json::from_value(
        serde_json::json!({ "target": "agent-1", "message": "stop", "interrupt": true }),
    )
    .unwrap();

    assert_eq!(queue_only.turn_mode(), AgentInputTurnMode::QueueOnly);
    assert_eq!(trigger_turn.turn_mode(), AgentInputTurnMode::TriggerTurn);
    assert_eq!(interrupt.turn_mode(), AgentInputTurnMode::Interrupt);
}

#[test]
fn agent_control_spawn_request_exposes_shared_agent_type_policy() {
    let omitted: crate::tool::AgentControlSpawnRequest =
        serde_json::from_value(serde_json::json!({ "taskName": "inspect", "message": "inspect" }))
            .unwrap();
    let reviewer: crate::tool::AgentControlSpawnRequest = serde_json::from_value(
        serde_json::json!({ "taskName": "review", "message": "review", "agentType": "reviewer" }),
    )
    .unwrap();
    let worker_alias: crate::tool::AgentControlSpawnRequest = serde_json::from_value(
        serde_json::json!({ "taskName": "work", "message": "work", "agentType": "worker" }),
    )
    .unwrap();
    let default_alias: crate::tool::AgentControlSpawnRequest = serde_json::from_value(
        serde_json::json!({ "taskName": "work", "message": "work", "agentType": " default " }),
    )
    .unwrap();

    assert_eq!(
        omitted.agent_type_policy(),
        crate::tool::AgentControlAgentTypePolicy {
            kind: crate::tool::AgentControlAgentType::Executor,
            role_profile_requested: false,
        }
    );
    assert_eq!(
        reviewer.agent_type_policy(),
        crate::tool::AgentControlAgentTypePolicy {
            kind: crate::tool::AgentControlAgentType::Reviewer,
            role_profile_requested: true,
        }
    );
    assert_eq!(
        worker_alias.agent_type_policy(),
        crate::tool::AgentControlAgentTypePolicy {
            kind: crate::tool::AgentControlAgentType::Executor,
            role_profile_requested: false,
        }
    );
    assert_eq!(
        default_alias.agent_type_policy(),
        crate::tool::AgentControlAgentTypePolicy {
            kind: crate::tool::AgentControlAgentType::Executor,
            role_profile_requested: false,
        }
    );
}

#[test]
fn agent_control_spawn_request_normalizes_initial_message() {
    let empty: crate::tool::AgentControlSpawnRequest =
        serde_json::from_value(serde_json::json!({ "taskName": "inspect", "message": "   \n\t" }))
            .unwrap();
    let message: crate::tool::AgentControlSpawnRequest = serde_json::from_value(
        serde_json::json!({ "taskName": "inspect", "message": "  keep spacing  " }),
    )
    .unwrap();

    assert_eq!(empty.initial_message(), None);
    assert_eq!(
        message.initial_message(),
        Some("  keep spacing  ".to_string())
    );
}

#[test]
fn agent_control_wait_request_normalizes_timeout_duration() {
    let default_timeout: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({})).unwrap();
    let negative_timeout: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({ "timeoutMs": -1 })).unwrap();
    let too_small_timeout: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({ "timeoutMs": 10 })).unwrap();
    let explicit_timeout: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({ "timeoutMs": 1250 })).unwrap();

    assert_eq!(
        default_timeout.timeout_duration(),
        std::time::Duration::from_secs(30)
    );
    assert_eq!(
        negative_timeout.timeout_duration(),
        std::time::Duration::from_secs(30)
    );
    assert_eq!(
        too_small_timeout.timeout_duration(),
        std::time::Duration::from_millis(100)
    );
    assert_eq!(
        explicit_timeout.timeout_duration(),
        std::time::Duration::from_millis(1250)
    );
}

#[test]
fn agent_control_wait_request_selects_explicit_targets_or_all_defaults() {
    let default_wait: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({})).unwrap();
    let explicit_wait: crate::tool::AgentControlWaitRequest =
        serde_json::from_value(serde_json::json!({
            "target": "agent-1",
            "targets": ["agent-2", "agent-1"]
        }))
        .unwrap();

    assert_eq!(
        default_wait.targets_or_all(["agent-1".to_string(), "agent-2".to_string()]),
        vec!["agent-1".to_string(), "agent-2".to_string()]
    );
    assert_eq!(
        explicit_wait.targets_or_all(["agent-3".to_string()]),
        vec!["agent-1".to_string(), "agent-2".to_string()]
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
fn agent_control_list_request_filters_records_and_compacts_text() {
    let request: crate::tool::AgentControlListRequest =
        serde_json::from_value(serde_json::json!({ "pathPrefix": "/root/agent" })).unwrap();
    let long_task = format!("  {}  ", "a".repeat(260));
    let long_summary = format!("  {}  ", "b".repeat(260));
    let long_error = format!("  {}  ", "c".repeat(260));
    let matching = crate::tool::AgentControlAgentRecord::new(
        "/root/agent-1",
        AgentStatus::Running,
        "executor",
        long_task,
        Some(long_summary),
        Some(long_error),
    );
    let hidden = crate::tool::AgentControlAgentRecord::new(
        "/root/other",
        AgentStatus::Completed,
        "reviewer",
        "done",
        None,
        None,
    );

    let output = request.into_list_output([matching.clone(), hidden]);

    assert_eq!(output.agents, vec![matching]);
    assert_eq!(output.agents[0].task, format!("{}...", "a".repeat(240)));
    assert_eq!(
        output.agents[0].summary,
        Some(format!("{}...", "b".repeat(240)))
    );
    assert_eq!(
        output.agents[0].error,
        Some(format!("{}...", "c".repeat(240)))
    );
}

#[test]
fn agent_control_target_request_builds_message_output() {
    let request: crate::tool::AgentControlTargetRequest =
        serde_json::from_value(serde_json::json!({ "target": "agent-1" })).unwrap();

    assert_eq!(
        request.into_message_output(AgentStatus::Shutdown),
        crate::tool::AgentControlMessageOutput {
            target: "agent-1".to_string(),
            status: AgentStatus::Shutdown,
        }
    );
}

#[test]
fn agent_control_outputs_have_shared_constructors() {
    let spawned = crate::tool::AgentControlSpawnOutput::new(
        "agent-1",
        "inspect",
        "/root/agent-1",
        AgentStatus::Queued,
        Some("turn-1".to_string()),
    );
    let waited =
        crate::tool::AgentControlWaitOutput::message("no managed sub-agents to wait for", false);

    assert_eq!(
        spawned,
        crate::tool::AgentControlSpawnOutput {
            agent_id: "agent-1".to_string(),
            task_name: "inspect".to_string(),
            path: "/root/agent-1".to_string(),
            status: AgentStatus::Queued,
            turn_id: Some("turn-1".to_string()),
        }
    );
    assert_eq!(
        waited,
        crate::tool::AgentControlWaitOutput {
            message: "no managed sub-agents to wait for".to_string(),
            timed_out: false,
        }
    );
}

#[test]
fn spawn_agent_result_serializes_turn_metadata() {
    let output = json_output(SpawnAgentResult {
        agent_id: "agent-1".to_string(),
        task_name: "inspect".to_string(),
        path: "/root/agent-1".to_string(),
        status: AgentStatus::Queued,
        turn_id: None,
    })
    .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
        serde_json::json!({
            "agentId": "agent-1",
            "taskName": "inspect",
            "path": "/root/agent-1",
            "status": "queued",
            "turnId": null
        })
    );
}

#[test]
fn send_input_result_serializes_queue_metadata() {
    let output = json_output(SendInputResult {
        target: "/root/agent-1".to_string(),
        status: AgentStatus::Running,
        interrupt: false,
        queued: true,
        turn_id: Some("turn-1".to_string()),
    })
    .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
        serde_json::json!({
            "target": "/root/agent-1",
            "status": "running",
            "interrupt": false,
            "queued": true,
            "turnId": "turn-1"
        })
    );
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

#[test]
fn agent_control_kind_schemas_match_tool_schemas() {
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

    assert_eq!(
        crate::tool::AgentControlToolKind::SpawnAgent.input_schema(),
        super::SpawnAgentTool {
            runtime: runtime.clone()
        }
        .input_schema()
    );
    assert_eq!(
        crate::tool::AgentControlToolKind::SendInput.input_schema(),
        super::SendInputTool { runtime }.input_schema()
    );
    assert_eq!(
        crate::tool::AgentControlToolKind::WaitAgent.input_schema(),
        super::WaitAgentTool.input_schema()
    );
    assert_eq!(
        crate::tool::AgentControlToolKind::ListAgents.input_schema(),
        super::ListAgentsTool.input_schema()
    );
    assert_eq!(
        crate::tool::AgentControlToolKind::CloseAgent.input_schema(),
        super::CloseAgentTool.input_schema()
    );
    assert_eq!(
        crate::tool::AgentControlToolKind::ResumeAgent.input_schema(),
        ResumeAgentTool.input_schema()
    );
}

#[derive(Debug, Clone)]
struct FakeHostAgentControlBackend;

impl crate::tool::AgentControlBackend for FakeHostAgentControlBackend {
    type Error = DisplayAgentControlError;

    async fn spawn_agent(
        &self,
        request: crate::tool::AgentControlSpawnRequest,
    ) -> std::result::Result<crate::tool::AgentControlSpawnOutput, Self::Error> {
        assert_eq!(request.skill_mentions, vec!["rust".to_string()]);
        Ok(crate::tool::AgentControlSpawnOutput {
            agent_id: "agent-1".to_string(),
            task_name: request.task_name,
            path: "/root/agent-1".to_string(),
            status: AgentStatus::Queued,
            turn_id: Some("turn-1".to_string()),
        })
    }

    async fn send_input(
        &self,
        request: crate::tool::AgentControlSendInputRequest,
    ) -> std::result::Result<crate::tool::AgentControlSendInputOutput, Self::Error> {
        Ok(crate::tool::AgentControlSendInputOutput {
            target: request.target,
            status: AgentStatus::Running,
            interrupt: request.interrupt,
            queued: false,
            turn_id: Some("turn-2".to_string()),
        })
    }

    async fn wait_agent(
        &self,
        _request: crate::tool::AgentControlWaitRequest,
    ) -> std::result::Result<crate::tool::AgentControlWaitOutput, Self::Error> {
        Ok(crate::tool::AgentControlWaitOutput {
            message: "observed".to_string(),
            timed_out: false,
        })
    }

    async fn list_agents(
        &self,
        _request: crate::tool::AgentControlListRequest,
    ) -> std::result::Result<crate::tool::AgentControlListOutput, Self::Error> {
        Ok(crate::tool::AgentControlListOutput {
            agents: vec![crate::tool::AgentControlAgentRecord {
                path: "/root/agent-1".to_string(),
                status: AgentStatus::Queued,
                role: "executor".to_string(),
                task: "inspect".to_string(),
                summary: None,
                error: None,
            }],
        })
    }

    async fn close_agent(
        &self,
        request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        Ok(crate::tool::AgentControlMessageOutput {
            target: request.target,
            status: AgentStatus::Shutdown,
        })
    }

    async fn resume_agent(
        &self,
        request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        Ok(crate::tool::AgentControlMessageOutput {
            target: request.target,
            status: AgentStatus::Queued,
        })
    }
}

#[derive(Debug, Clone)]
struct DisplayAgentControlError(&'static str);

impl std::fmt::Display for DisplayAgentControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Debug, Clone)]
struct FailingHostAgentControlBackend;

impl crate::tool::AgentControlBackend for FailingHostAgentControlBackend {
    type Error = DisplayAgentControlError;

    async fn spawn_agent(
        &self,
        _request: crate::tool::AgentControlSpawnRequest,
    ) -> std::result::Result<crate::tool::AgentControlSpawnOutput, Self::Error> {
        Err(DisplayAgentControlError("spawn blocked"))
    }

    async fn send_input(
        &self,
        _request: crate::tool::AgentControlSendInputRequest,
    ) -> std::result::Result<crate::tool::AgentControlSendInputOutput, Self::Error> {
        unreachable!("send_input is not used by this test")
    }

    async fn wait_agent(
        &self,
        _request: crate::tool::AgentControlWaitRequest,
    ) -> std::result::Result<crate::tool::AgentControlWaitOutput, Self::Error> {
        unreachable!("wait_agent is not used by this test")
    }

    async fn list_agents(
        &self,
        _request: crate::tool::AgentControlListRequest,
    ) -> std::result::Result<crate::tool::AgentControlListOutput, Self::Error> {
        unreachable!("list_agents is not used by this test")
    }

    async fn close_agent(
        &self,
        _request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        unreachable!("close_agent is not used by this test")
    }

    async fn resume_agent(
        &self,
        _request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        unreachable!("resume_agent is not used by this test")
    }
}

#[derive(Debug, Clone)]
struct RecordingHostAgentControlBackend {
    forked_messages: Arc<Mutex<Option<Vec<pl_protocol::Message>>>>,
}

impl crate::tool::AgentControlBackend for RecordingHostAgentControlBackend {
    type Error = DisplayAgentControlError;

    async fn spawn_agent(
        &self,
        request: crate::tool::AgentControlSpawnRequest,
    ) -> std::result::Result<crate::tool::AgentControlSpawnOutput, Self::Error> {
        *self.forked_messages.lock().expect("record forked messages") =
            request.forked_messages.clone();
        Ok(crate::tool::AgentControlSpawnOutput {
            agent_id: "agent-1".to_string(),
            task_name: request.task_name,
            path: "/root/agent-1".to_string(),
            status: AgentStatus::Queued,
            turn_id: None,
        })
    }

    async fn send_input(
        &self,
        _request: crate::tool::AgentControlSendInputRequest,
    ) -> std::result::Result<crate::tool::AgentControlSendInputOutput, Self::Error> {
        unreachable!("send_input is not used by this test")
    }

    async fn wait_agent(
        &self,
        _request: crate::tool::AgentControlWaitRequest,
    ) -> std::result::Result<crate::tool::AgentControlWaitOutput, Self::Error> {
        unreachable!("wait_agent is not used by this test")
    }

    async fn list_agents(
        &self,
        _request: crate::tool::AgentControlListRequest,
    ) -> std::result::Result<crate::tool::AgentControlListOutput, Self::Error> {
        unreachable!("list_agents is not used by this test")
    }

    async fn close_agent(
        &self,
        _request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        unreachable!("close_agent is not used by this test")
    }

    async fn resume_agent(
        &self,
        _request: crate::tool::AgentControlTargetRequest,
    ) -> std::result::Result<crate::tool::AgentControlMessageOutput, Self::Error> {
        unreachable!("resume_agent is not used by this test")
    }
}

#[derive(Debug, Clone)]
struct DenyTargetAgentControlPolicy {
    calls: Arc<Mutex<Vec<String>>>,
}

impl crate::tool::AgentControlPolicy for DenyTargetAgentControlPolicy {
    type Error = DisplayAgentControlError;

    async fn check_tool(
        &self,
        kind: crate::tool::AgentControlToolKind,
    ) -> std::result::Result<(), Self::Error> {
        self.calls
            .lock()
            .expect("policy calls")
            .push(format!("tool:{}", kind.name()));
        Ok(())
    }

    async fn check_target(
        &self,
        kind: crate::tool::AgentControlToolKind,
        target: &str,
    ) -> std::result::Result<(), Self::Error> {
        self.calls
            .lock()
            .expect("policy calls")
            .push(format!("target:{}:{target}", kind.name()));
        Err(DisplayAgentControlError("target blocked by policy"))
    }
}

#[tokio::test]
async fn host_agent_control_backend_display_error_maps_to_tool_error() {
    let spawn_tool = crate::tool::AgentControlTool::new(
        crate::tool::AgentControlToolKind::SpawnAgent,
        Arc::new(FailingHostAgentControlBackend),
    );
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
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(crate::CoreSession::new()),
    };

    let error = spawn_tool
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({
                    "taskName": "inspect",
                    "message": "check this",
                    "agentType": "executor"
                }),
                session_id: "session-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .expect_err("backend should fail");

    assert!(matches!(
        error,
        pl_protocol::PureError::ToolExecutionFailed { tool, error }
            if tool == "spawn_agent" && error == "spawn blocked"
    ));
}

#[tokio::test]
async fn host_agent_control_policy_denies_target_before_backend() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let send_tool = crate::tool::AgentControlTool::with_policy(
        crate::tool::AgentControlToolKind::SendInput,
        Arc::new(FakeHostAgentControlBackend),
        Arc::new(DenyTargetAgentControlPolicy {
            calls: Arc::clone(&calls),
        }),
    );
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
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(crate::CoreSession::new()),
    };

    let error = send_tool
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({
                    "target": "/root/blocked",
                    "message": "continue"
                }),
                session_id: "session-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .expect_err("policy should deny target before backend");

    assert!(error.to_string().contains("target blocked by policy"));
    assert_eq!(
        calls.lock().expect("policy calls").clone(),
        vec![
            "tool:send_input".to_string(),
            "target:send_input:/root/blocked".to_string(),
        ]
    );
}

#[tokio::test]
async fn host_agent_control_tool_uses_shared_schema_parse_output_and_lock_policy() {
    let spawn_tool = crate::tool::AgentControlTool::new(
        crate::tool::AgentControlToolKind::SpawnAgent,
        Arc::new(FakeHostAgentControlBackend),
    );

    assert_eq!(spawn_tool.name(), "spawn_agent");
    assert!(
        spawn_tool
            .input_schema()
            .pointer("/properties/taskName")
            .is_some()
    );
    assert_eq!(
        spawn_tool.runtime_lock_policy(),
        ToolRuntimeLockPolicy::Shared
    );

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
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(crate::CoreSession::new()),
    };
    let output = spawn_tool
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({
                    "taskName": "inspect_runtime",
                    "message": "inspect",
                    "agentType": "explorer",
                    "skillMentions": ["rust"]
                }),
                session_id: "session-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            context.clone(),
        )
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
        serde_json::json!({
            "agentId": "agent-1",
            "taskName": "inspect_runtime",
            "path": "/root/agent-1",
            "status": "queued",
            "turnId": "turn-1"
        })
    );

    let send_tool = crate::tool::AgentControlTool::new(
        crate::tool::AgentControlToolKind::SendInput,
        Arc::new(FakeHostAgentControlBackend),
    );
    assert_eq!(
        send_tool.runtime_lock_policy(),
        ToolRuntimeLockPolicy::Exclusive
    );
    let output = send_tool
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({
                    "target": "/root/agent-1",
                    "message": "continue",
                    "triggerTurn": true,
                    "interrupt": true
                }),
                session_id: "session-1".to_string(),
                tool_id: "call-2".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.description).unwrap(),
        serde_json::json!({
            "target": "/root/agent-1",
            "status": "running",
            "interrupt": true,
            "queued": false,
            "turnId": "turn-2"
        })
    );
}

#[tokio::test]
async fn host_spawn_agent_receives_pl_core_filtered_fork_history() {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};

    let mut tool_metadata = HashMap::new();
    tool_metadata.insert("tool_calls".to_string(), "[]".to_string());
    let parent_session = crate::CoreSession::from_messages(vec![
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("first".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("first answer".to_string()),
            reasoning_content: Some("hidden reasoning".to_string()),
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
            content: MessageContent::Text("tool output".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::User,
            content: MessageContent::Text("second".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        },
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text("second answer".to_string()),
            reasoning_content: Some("more hidden reasoning".to_string()),
            metadata: HashMap::new(),
        },
    ]);
    let recorded = Arc::new(Mutex::new(None));
    let spawn_tool = crate::tool::AgentControlTool::new(
        crate::tool::AgentControlToolKind::SpawnAgent,
        Arc::new(RecordingHostAgentControlBackend {
            forked_messages: Arc::clone(&recorded),
        }),
    );
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
        agent_tool_registrar: None,
        lsp_runtime: None,
        parent_session: Arc::new(parent_session),
    };

    spawn_tool
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({
                    "taskName": "inspect_runtime",
                    "message": "inspect",
                    "forkTurns": "1"
                }),
                session_id: "session-1".to_string(),
                tool_id: "call-1".to_string(),
                revision_base: 0,
            },
            context,
        )
        .await
        .unwrap();

    let forked_messages = recorded
        .lock()
        .expect("recorded forked messages")
        .clone()
        .expect("forked messages");

    assert_eq!(
        forked_messages,
        vec![
            Message {
                role: MessageRole::User,
                content: MessageContent::Text("second".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            },
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text("second answer".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            },
        ]
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
