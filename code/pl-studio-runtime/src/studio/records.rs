pub use crate::attachment::MaterializedAttachment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub visibility: SessionVisibility,
    pub parent_session_id: Option<String>,
    pub root_session_id: String,
    pub session_kind: SessionKind,
    pub owner_agent_id: String,
    pub owner_role: String,
    pub agent_status: String,
    pub agent_summary: Option<String>,
    pub agent_error: Option<String>,
    pub agent_updated_at: Option<i64>,
    pub instruction_snapshot: Option<crate::InstructionSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Root,
    Agent,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionVisibility {
    Active,
    Archived,
}

impl SessionVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRecord {
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub media_type: String,
    pub filename: Option<String>,
    pub storage_path: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionHistoryItemRecord {
    pub sequence: i64,
    pub item_id: String,
    pub turn_id: String,
    pub item_kind: String,
    pub payload: crate::SessionEventEnvelope,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionHistoryTurnRecord {
    pub turn_sequence: i64,
    pub turn_id: String,
    pub status: String,
    pub model: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub items: Vec<SessionHistoryItemRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionHistoryPageRecord {
    pub turns: Vec<SessionHistoryTurnRecord>,
    pub next_before_turn_sequence: Option<i64>,
    pub has_more: bool,
}
