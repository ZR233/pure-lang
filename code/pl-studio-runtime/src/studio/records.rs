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
    HandoffOrigin,
    Archived,
}

impl SessionVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::HandoffOrigin => "handoffOrigin",
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
