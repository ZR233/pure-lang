use std::path::PathBuf;

use pl_protocol::AgentStatus;
use serde::Serialize;

use super::ToolOutput;
use super::truncation::OutputTruncation;
use crate::agent::AgentRecord;
use crate::core::compact_text;
use crate::provider_error::is_provider_429_error;

pub(crate) const RECOVERABLE_SUBAGENT_429_MARKER: &str = "recoverableSubagentProvider429";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecoverableSubagentFailure {
    pub agent_id: String,
    pub path: String,
    pub task: String,
    pub error: String,
}

impl RecoverableSubagentFailure {
    fn from_record(record: &AgentRecord) -> Option<Self> {
        let error = record.error.as_deref()?;
        if !matches!(
            record.status,
            AgentStatus::Errored | AgentStatus::Interrupted
        ) || !is_recoverable_subagent_capacity_error(error)
        {
            return None;
        }
        Some(Self {
            agent_id: record.id.clone(),
            path: record.path.clone(),
            task: record.task.clone(),
            error: error.to_string(),
        })
    }
}

pub(super) fn is_recoverable_subagent_capacity_error(error: &str) -> bool {
    is_provider_429_error(error)
}

pub(super) fn recoverable_subagent_tool_output(task: &str, error: &str) -> ToolOutput {
    ToolOutput {
        description: recoverable_subagent_message(task, error),
        truncated: empty_truncation(),
        output_file: PathBuf::new(),
        exit_code: None,
        timed_out: false,
        runtime_events: Vec::new(),
    }
}

pub(super) fn recoverable_subagent_message(task: &str, error: &str) -> String {
    format!(
        "{RECOVERABLE_SUBAGENT_429_MARKER}: subagent is unavailable because the provider returned 429 concurrency/rate-limit capacity. Continue this task in the current agent without spawning or retrying another subagent.\nTask: {}\nError: {}",
        compact_text(task),
        compact_text(error)
    )
}

pub(super) fn recoverable_subagent_failures(
    records: &[AgentRecord],
) -> Vec<RecoverableSubagentFailure> {
    records
        .iter()
        .filter_map(RecoverableSubagentFailure::from_record)
        .collect()
}

pub(super) fn recoverable_subagent_failures_message(count: usize) -> String {
    format!(
        "{RECOVERABLE_SUBAGENT_429_MARKER}: {count} subagent(s) are unavailable because the provider returned 429 concurrency/rate-limit capacity. Stop creating or retrying subagents and continue the remaining work in the current agent."
    )
}

fn empty_truncation() -> OutputTruncation {
    OutputTruncation::empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn detects_recoverable_provider_429_errors() {
        assert!(is_recoverable_subagent_capacity_error(
            "API error 429 Too Many Requests: concurrency limit reached"
        ));
        assert!(is_recoverable_subagent_capacity_error(
            "provider returned status 429"
        ));
        assert!(!is_recoverable_subagent_capacity_error("Too Many Requests"));
        assert!(!is_recoverable_subagent_capacity_error(
            "API error 500 internal server error"
        ));
        assert!(!is_recoverable_subagent_capacity_error(
            "local tool failed with code 1429"
        ));
    }

    #[test]
    fn extracts_recoverable_failures_from_agent_records() {
        let records = vec![
            AgentRecord {
                id: "agent-1".to_string(),
                path: "/root/a".to_string(),
                parent_path: Some("/root".to_string()),
                role: "executor".to_string(),
                task: "inspect a".to_string(),
                status: AgentStatus::Errored,
                summary: None,
                error: Some("API error 429 Too Many Requests".to_string()),
                reason: Some("providerError".to_string()),
                budget_limit_kind: None,
                budget_usage: None,
                depth: 1,
                updated_at: 1,
            },
            AgentRecord {
                id: "agent-2".to_string(),
                path: "/root/b".to_string(),
                parent_path: Some("/root".to_string()),
                role: "executor".to_string(),
                task: "inspect b".to_string(),
                status: AgentStatus::Completed,
                summary: Some("done".to_string()),
                error: None,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
                depth: 1,
                updated_at: 1,
            },
        ];

        assert_eq!(
            recoverable_subagent_failures(&records),
            vec![RecoverableSubagentFailure {
                agent_id: "agent-1".to_string(),
                path: "/root/a".to_string(),
                task: "inspect a".to_string(),
                error: "API error 429 Too Many Requests".to_string(),
            }]
        );
    }
}
