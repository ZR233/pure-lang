use std::collections::{HashMap, VecDeque};

use tokio_util::sync::CancellationToken;

use super::{AgentMailboxMessage, AgentStatusUpdate};
use crate::CoreSession;
use crate::agent::{AgentPath, AgentRecord, AgentStatus};

pub(super) fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
pub(super) struct AgentEntry {
    pub(super) record: AgentRecord,
    pub(super) session: CoreSession,
    pub(super) mailbox: VecDeque<AgentMailboxMessage>,
    pub(super) cancellation_token: Option<CancellationToken>,
}

impl AgentEntry {
    pub(super) fn new(record: AgentRecord) -> Self {
        Self {
            record,
            session: CoreSession::new(),
            mailbox: VecDeque::new(),
            cancellation_token: None,
        }
    }

    pub(super) fn apply_status_transition(&mut self, transition: AgentStatusTransition) -> bool {
        if self.record.status.is_final() {
            return false;
        }
        match transition {
            AgentStatusTransition::Update(update) => self.apply_status_update(update),
            AgentStatusTransition::QueueFollowup => {
                self.record.status = AgentStatus::Queued;
                clear_status_details(&mut self.record);
            }
            AgentStatusTransition::QueueMessage => {
                if !matches!(
                    self.record.status,
                    AgentStatus::Queued | AgentStatus::Running
                ) {
                    self.record.status = AgentStatus::Waiting;
                }
            }
            AgentStatusTransition::Shutdown { reason } => {
                self.record.status = AgentStatus::Shutdown;
                self.record.reason = Some(reason);
            }
        }
        self.record.updated_at = unix_seconds();
        true
    }

    fn apply_status_update(&mut self, update: AgentStatusUpdate) {
        self.record.status = update.status;
        if clears_status_details(update.status) {
            clear_status_details(&mut self.record);
        }
        if update.summary.is_some() {
            self.record.summary = update.summary;
        }
        if update.error.is_some() {
            self.record.error = update.error;
        }
        if update.reason.is_some() {
            self.record.reason = update.reason;
        }
        if update.budget_limit_kind.is_some() {
            self.record.budget_limit_kind = update.budget_limit_kind;
        }
        if update.budget_usage.is_some() {
            self.record.budget_usage = update.budget_usage;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum AgentStatusTransition {
    Update(AgentStatusUpdate),
    QueueFollowup,
    QueueMessage,
    Shutdown { reason: String },
}

#[derive(Debug)]
pub(super) struct AgentControlState {
    pub(super) agents: HashMap<String, AgentEntry>,
    pub(super) path_to_id: HashMap<String, String>,
    pub(super) next_id: u64,
    pub(super) max_agents: usize,
    pub(super) max_depth: u32,
    pub(super) activity_seq: u64,
    pub(super) observed_activity_seq: u64,
}

impl Default for AgentControlState {
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

impl AgentControlState {
    pub(super) fn mark_activity(&mut self) {
        self.activity_seq = self.activity_seq.saturating_add(1);
    }

    pub(super) fn has_final_agent(&self) -> bool {
        self.agents.values().any(|entry| {
            !entry.record.path.as_str().eq(AgentPath::ROOT) && entry.record.status.is_final()
        })
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

pub(super) fn descendant_ids(state: &AgentControlState, agent_id: &str) -> Vec<String> {
    let Some(root) = state.agents.get(agent_id) else {
        return Vec::new();
    };
    let prefix = format!("{}/", root.record.path);
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

fn clears_status_details(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Queued | AgentStatus::Running | AgentStatus::Completed
    )
}

fn clear_status_details(record: &mut AgentRecord) {
    record.error = None;
    record.reason = None;
    record.budget_limit_kind = None;
    record.budget_usage = None;
}
