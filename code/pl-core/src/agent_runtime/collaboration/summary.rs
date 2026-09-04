//! agent 快照与 wait 消息的紧凑 JSON 投影。

use serde_json::{Value, json};

use super::super::state::unix_timestamp;
use super::super::{AgentDirectoryWaitMessage, AgentDirectoryWaitReason, AgentSnapshot};
use super::support::agent_path;
use super::{TOOL_READ_AGENT_SESSION, TOOL_SEND_MESSAGE};

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

pub(super) fn wait_guidance(reason: AgentDirectoryWaitReason) -> Option<Value> {
    matches!(reason, AgentDirectoryWaitReason::BudgetLimited).then(|| {
        json!({
            "summary": "A child agent reached its turn budget and is paused without post-budget compaction or automatic continuation.",
            "inspectWith": TOOL_READ_AGENT_SESSION,
            "continueWith": TOOL_SEND_MESSAGE,
            "nextStep": "Inspect the paused agent's durable Timeline. If its progress is healthy and work remains, send a concrete continuation message; otherwise close or re-dispatch it."
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::agent_runtime::{
        AgentDirectoryWaitMessage, AgentIdentity, AgentProgressCheckpoint, AgentProgressReport,
        AgentProgressStage, AgentState, ThreadId,
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
    fn budget_wait_guidance_names_inspection_and_explicit_continuation_tools() {
        let guidance = wait_guidance(AgentDirectoryWaitReason::BudgetLimited).unwrap();
        assert_eq!(guidance["inspectWith"], "read_agent_session");
        assert_eq!(guidance["continueWith"], "send_message");
        assert!(wait_guidance(AgentDirectoryWaitReason::Terminal).is_none());
    }
}
