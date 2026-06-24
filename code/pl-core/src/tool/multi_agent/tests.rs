use pl_protocol::{AgentStatus, InteractionResolution};
use pretty_assertions::assert_eq;
use std::sync::Arc;

use super::ForkTurns;
use super::agent_tool_records;
use super::child_agent_options;
use super::fork_session;
use super::json_output;
use super::tools::followup_prompt;
use super::types::{AgentToolRecord, ListAgentsResult, WaitAgentResult};
use crate::agent::{AgentMailboxMessage, AgentRecord};
use crate::tool::recoverable::{
    recoverable_subagent_failures, recoverable_subagent_failures_message,
};
use crate::tool::{Tool, ToolRuntimeLockPolicy};

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
fn wait_agent_result_serializes_recoverable_failures() {
    let agents = vec![
        agent_record(
            "agent-1",
            AgentStatus::Errored,
            Some("API error 429 Too Many Requests"),
        ),
        agent_record("agent-2", AgentStatus::Completed, None),
    ];
    let recoverable_failures = recoverable_subagent_failures(&agents);
    let output = json_output(WaitAgentResult {
        message: recoverable_subagent_failures_message(recoverable_failures.len()),
        timed_out: false,
        agents: agent_tool_records(&agents, false),
        recoverable_failures,
    })
    .unwrap();
    let result = serde_json::from_str::<WaitAgentResult>(&output.description).unwrap();

    assert_eq!(
        result,
        WaitAgentResult {
            message: "recoverableSubagentProvider429: 1 subagent(s) are unavailable because the provider returned 429 concurrency/rate-limit capacity. Stop creating or retrying subagents and continue the remaining work in the current agent.".to_string(),
            timed_out: false,
            agents: agent_tool_records(&agents, false),
            recoverable_failures: recoverable_subagent_failures(&agents),
        }
    );
    assert!(matches!(result.agents[0], AgentToolRecord::Compact(_)));
}

#[test]
fn list_agents_result_round_trips_compact_agents_and_recoverable_failures() {
    let agents = vec![
        agent_record(
            "agent-1",
            AgentStatus::Interrupted,
            Some("provider returned status 429"),
        ),
        agent_record("agent-2", AgentStatus::Completed, None),
    ];
    let recoverable_failures = recoverable_subagent_failures(&agents);
    let output = json_output(ListAgentsResult {
        agents: agent_tool_records(&agents, false),
        recoverable_failures,
    })
    .unwrap();
    let result = serde_json::from_str::<ListAgentsResult>(&output.description).unwrap();

    assert_eq!(
        result,
        ListAgentsResult {
            agents: agent_tool_records(&agents, false),
            recoverable_failures: recoverable_subagent_failures(&agents),
        }
    );
    assert!(matches!(result.agents[0], AgentToolRecord::Compact(_)));

    let detailed_output = json_output(ListAgentsResult {
        agents: agent_tool_records(&agents, true),
        recoverable_failures: Vec::new(),
    })
    .unwrap();
    let detailed = serde_json::from_str::<ListAgentsResult>(&detailed_output.description).unwrap();
    assert!(matches!(detailed.agents[0], AgentToolRecord::Detailed(_)));
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
fn followup_prompt_includes_queued_messages_in_order() {
    let prompt = followup_prompt(vec![
        AgentMailboxMessage {
            sender_path: "/root".to_string(),
            message: "context".to_string(),
            trigger_turn: false,
        },
        AgentMailboxMessage {
            sender_path: "/root".to_string(),
            message: "resume".to_string(),
            trigger_turn: true,
        },
    ]);

    assert_eq!(
        prompt,
        "Queued message from /root:\ncontext\n\nFollow-up task:\nresume"
    );
}

#[test]
fn wait_agent_does_not_hold_runtime_lock() {
    let wait_agent = super::WaitAgentTool;

    assert_eq!(
        wait_agent.runtime_lock_policy(),
        ToolRuntimeLockPolicy::None
    );
}
