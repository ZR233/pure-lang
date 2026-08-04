pub use crate::attachment::MaterializedAttachment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub visibility: ThreadVisibility,
    pub parent_thread_id: Option<String>,
    pub root_thread_id: String,
    pub thread_kind: ThreadKind,
    pub agent_path: String,
    pub role: String,
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub runtime_updated_at: Option<i64>,
}

impl From<ThreadRecord> for pl_protocol::Thread {
    fn from(value: ThreadRecord) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            title: value.title,
            mode: match crate::StudioMode::from_label(&value.mode) {
                crate::StudioMode::Simple => pl_protocol::ThreadMode::Simple,
                crate::StudioMode::Task => pl_protocol::ThreadMode::Task,
            },
            root_thread_id: value.root_thread_id,
            parent_thread_id: value.parent_thread_id,
            role: value.role,
            agent_path: value.agent_path,
            status: match value.status.as_str() {
                "running" => pl_protocol::ThreadStatus::Running,
                "waiting" => pl_protocol::ThreadStatus::Waiting,
                "completed" => pl_protocol::ThreadStatus::Completed,
                "failed" => pl_protocol::ThreadStatus::Failed,
                "closed" => pl_protocol::ThreadStatus::Closed,
                _ => pl_protocol::ThreadStatus::Idle,
            },
            created_at: value.created_at,
            updated_at: value.updated_at,
            archived: value.visibility == ThreadVisibility::Archived,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadKind {
    Root,
    Agent,
}

impl ThreadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadVisibility {
    Active,
    Archived,
}

impl ThreadVisibility {
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
    pub thread_id: String,
    pub item_id: Option<String>,
    pub media_type: String,
    pub filename: Option<String>,
    pub storage_path: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: i64,
}
