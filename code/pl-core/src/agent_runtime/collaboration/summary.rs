//! agent 快照与 wait 消息的紧凑 JSON 投影。

use serde_json::{Value, json};

use super::super::state::unix_timestamp;
use super::super::{AgentDirectoryWaitMessage, AgentSnapshot, AgentState};
use super::support::agent_path;

pub(super) fn compact_agent(snapshot: &AgentSnapshot, all: &[AgentSnapshot]) -> Value {
    json!({
        "identity": snapshot.identity.id,
        "path": agent_path(&snapshot.identity.id, all),
        "role": snapshot.identity.role,
        "state": snapshot.state,
        "lastTurnOutcome": snapshot.last_turn,
        "progress": snapshot.progress,
        "updatedAt": snapshot.updated_at,
        "summaryAgeSeconds": summary_age_seconds(snapshot),
    })
}

pub(super) fn compact_wait_message(
    message: &AgentDirectoryWaitMessage,
    all: &[AgentSnapshot],
) -> Value {
    let progress = message.message.as_ref().map(|progress| {
        json!({
            "stage": progress.report.stage,
            "summary": progress.report.summary,
            "nextStep": progress.report.next_step,
        })
    });
    json!({
        "agentId": message.identity.id,
        "path": agent_path(&message.identity.id, all),
        "role": message.identity.role,
        "message": progress,
        "state": {
            "agent": message.state,
            "lastTurnOutcome": message.last_turn_outcome,
        },
    })
}

pub(super) fn summary_age_seconds(snapshot: &AgentSnapshot) -> i64 {
    unix_timestamp()
        .saturating_sub(
            snapshot
                .progress
                .as_ref()
                .map_or(snapshot.updated_at, |progress| progress.updated_at),
        )
        .max(0)
}

pub(super) fn session_read_requires_age_gate(state: &AgentState) -> bool {
    state.is_operational() && !state.is_idle()
}

#[cfg(test)]
mod tests {
    use crate::agent_runtime::{
        AgentCommand, AgentDirectoryWaitMessage, AgentIdentity, AgentProgressCheckpoint,
        AgentProgressReport, AgentProgressStage, AgentState, ThreadId, TurnId,
    };
    use crate::model_config::AgentRoleId;

    use super::*;

    #[test]
    fn wait_message_projection_contains_only_latest_delta() {
        let agent_id = ThreadId::new("executor").unwrap();
        let message = AgentDirectoryWaitMessage {
            identity: AgentIdentity {
                id: agent_id,
                parent_id: None,
                role: AgentRoleId::new("executor").unwrap(),
                depth: 0,
            },
            state: AgentState::idle(),
            message: Some(AgentProgressCheckpoint {
                report: AgentProgressReport {
                    stage: AgentProgressStage::Verifying,
                    summary: "验证完成".to_string(),
                    next_step: "等待审查".to_string(),
                    revision: 3,
                },
                updated_at: 123,
            }),
            last_turn_outcome: None,
        };

        let output = compact_wait_message(&message, &[]);

        assert_eq!(output["agentId"], "executor");
        assert_eq!(output["path"], serde_json::json!(["executor"]));
        assert_eq!(output["message"]["stage"], "verifying");
        assert_eq!(output["message"]["summary"], "验证完成");
        assert!(output["message"].get("revision").is_none());
        assert!(output.get("agents").is_none());
        assert!(output["state"]["lastTurnOutcome"].is_null());
        assert!(output["state"].get("turnOutcome").is_none());

        let terminal = AgentDirectoryWaitMessage {
            identity: message.identity,
            state: AgentState::idle(),
            message: None,
            last_turn_outcome: None,
        };
        let terminal_output = compact_wait_message(&terminal, &[]);
        assert!(terminal_output["message"].is_null());
        assert_eq!(terminal_output["state"]["agent"]["kind"], "idle");
        assert!(terminal_output["state"]["lastTurnOutcome"].is_null());
    }

    #[test]
    fn read_session_age_gate_only_applies_while_agent_has_active_work() {
        let turn_id = TurnId::new("turn-running").unwrap();
        let queued = AgentState::idle()
            .decide(AgentCommand::Queue {
                turn_id: turn_id.clone(),
            })
            .unwrap()
            .next_state;
        let running = queued
            .decide(AgentCommand::Start { turn_id })
            .unwrap()
            .next_state;
        assert!(session_read_requires_age_gate(&running));
        assert!(!session_read_requires_age_gate(&AgentState::idle()));
    }
}
