use pl_protocol::AgentStatus;
use pl_protocol::UserInputResponse;
use pretty_assertions::assert_eq;
use std::sync::Arc;

use super::child_agent_options;
use super::json_output;
use super::types::{ListAgentsResult, WaitAgentResult};
use crate::agent::AgentRecord;
use crate::tool::recoverable::{
    recoverable_subagent_failures, recoverable_subagent_failures_message,
};

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
        agents,
        recoverable_failures,
    })
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(value["timedOut"], false);
    assert_eq!(
        value["message"],
        "recoverableSubagentProvider429: 1 subagent(s) are unavailable because the provider returned 429 concurrency/rate-limit capacity. Stop creating or retrying subagents and continue the remaining work in the current agent."
    );
    assert_eq!(value["recoverableFailures"][0]["agentId"], "agent-1");
    assert_eq!(value["recoverableFailures"][0]["path"], "/root/agent-1");
    assert_eq!(value["agents"].as_array().unwrap().len(), 2);
    assert_eq!(value["agents"][0]["status"], "errored");
    assert_eq!(value["agents"][1]["status"], "completed");
}

#[test]
fn list_agents_result_keeps_agents_and_recoverable_failures() {
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
        agents,
        recoverable_failures,
    })
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output.description).unwrap();

    assert_eq!(value["agents"].as_array().unwrap().len(), 2);
    assert_eq!(value["recoverableFailures"][0]["agentId"], "agent-1");
    assert_eq!(
        value["recoverableFailures"][0]["error"],
        "provider returned status 429"
    );
}

#[test]
fn child_agent_options_inherit_user_input_callback() {
    let callback: crate::UserInputCallback =
        Arc::new(|_request| Box::pin(async { UserInputResponse::default() }));
    let parent = crate::TurnOptions::default().with_user_input_callback(callback);

    let child = child_agent_options(&parent);

    assert!(child.user_input_callback.is_some());
}
