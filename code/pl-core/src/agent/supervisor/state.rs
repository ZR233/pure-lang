use std::collections::{HashMap, VecDeque};

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::snapshot::unix_seconds;
use super::{AgentMessage, AgentPath, AgentRecord, AgentStatus};
use crate::CoreSession;

#[derive(Debug)]
pub(super) struct AgentEntry {
    pub(super) record: AgentRecord,
    pub(super) session: CoreSession,
    pub(super) mailbox: VecDeque<AgentMessage>,
    pub(super) cancellation_token: Option<CancellationToken>,
    pub(super) task: Option<JoinHandle<()>>,
}

impl AgentEntry {
    pub(super) fn new(record: AgentRecord) -> Self {
        Self {
            record,
            session: CoreSession::new(),
            mailbox: VecDeque::new(),
            cancellation_token: None,
            task: None,
        }
    }
}

#[derive(Debug)]
pub(super) struct AgentSupervisorState {
    pub(super) agents: HashMap<String, AgentEntry>,
    pub(super) path_to_id: HashMap<String, String>,
    pub(super) next_id: u64,
    pub(super) max_agents: usize,
    pub(super) max_depth: u32,
    pub(super) activity_seq: u64,
    pub(super) observed_activity_seq: u64,
}

impl Default for AgentSupervisorState {
    fn default() -> Self {
        let mut state = Self {
            agents: HashMap::new(),
            path_to_id: HashMap::new(),
            next_id: 0,
            max_agents: 16,
            max_depth: 3,
            activity_seq: 0,
            observed_activity_seq: 0,
        };
        let root = AgentRecord {
            id: "agent-root".to_string(),
            path: AgentPath::ROOT.to_string(),
            parent_path: None,
            role: "root".to_string(),
            task: "root".to_string(),
            status: AgentStatus::Running,
            summary: None,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            depth: 0,
            updated_at: unix_seconds(),
        };
        state.path_to_id.insert(root.path.clone(), root.id.clone());
        state.agents.insert(root.id.clone(), AgentEntry::new(root));
        state
    }
}

impl AgentSupervisorState {
    pub(super) fn mark_activity(&mut self) {
        self.activity_seq = self.activity_seq.saturating_add(1);
    }

    pub(super) fn agent_records(&self, path_prefix: Option<&str>) -> Vec<AgentRecord> {
        let prefix = path_prefix
            .map(str::trim)
            .filter(|prefix| !prefix.is_empty());
        let mut agents: Vec<_> = self
            .agents
            .values()
            .map(|entry| entry.record.clone())
            .filter(|agent| {
                !agent.path.as_str().eq(AgentPath::ROOT)
                    && prefix.is_none_or(|prefix| path_matches_prefix(&agent.path, prefix))
            })
            .collect();
        agents.sort_by(|left, right| left.path.cmp(&right.path));
        agents
    }
}

pub(super) fn descendant_ids(state: &AgentSupervisorState, agent_id: &str) -> Vec<String> {
    let Some(root) = state.agents.get(agent_id) else {
        return Vec::new();
    };
    let path = &root.record.path;
    let prefix = format!("{path}/");
    state
        .agents
        .iter()
        .filter_map(|(id, entry)| {
            (id != agent_id && entry.record.path.starts_with(&prefix)).then_some(id.clone())
        })
        .collect()
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}
