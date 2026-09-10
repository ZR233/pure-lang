pub use pl_tool::attachment::MaterializedAttachment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub ssh_server_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRecord {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub mode: pl_protocol::ThreadModeId,
    pub created_at: i64,
    pub updated_at: i64,
    pub visibility: ThreadVisibility,
    pub parent_thread_id: Option<String>,
    pub root_thread_id: String,
    pub thread_kind: ThreadKind,
    pub agent_path: String,
    pub role: String,
    pub status: pl_protocol::ThreadStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub runtime_updated_at: Option<i64>,
}

impl ThreadRecord {
    /// 从目录事实构造记录（创建命令返回值用）；runtime 派生列取缺省值。
    pub(in crate::studio) fn from_directory_thread(thread: pl_protocol::Thread) -> Self {
        let thread_kind = if thread.parent_thread_id.is_some() {
            ThreadKind::Agent
        } else {
            ThreadKind::Root
        };
        Self {
            thread_kind,
            visibility: if thread.archived {
                ThreadVisibility::Archived
            } else {
                ThreadVisibility::Active
            },
            id: thread.id,
            project_id: thread.project_id,
            title: thread.title,
            mode: thread.mode,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
            parent_thread_id: thread.parent_thread_id,
            root_thread_id: thread.root_thread_id,
            agent_path: thread.agent_path,
            role: thread.role,
            status: thread.status,
            summary: None,
            error: None,
            runtime_updated_at: None,
        }
    }
}

impl From<ThreadRecord> for pl_protocol::Thread {
    fn from(value: ThreadRecord) -> Self {
        Self {
            id: value.id,
            project_id: value.project_id,
            title: value.title,
            mode: value.mode,
            root_thread_id: value.root_thread_id,
            parent_thread_id: value.parent_thread_id,
            role: value.role,
            agent_path: value.agent_path,
            status: value.status,
            created_at: value.created_at,
            updated_at: value.updated_at,
            archived: value.visibility == ThreadVisibility::Archived,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRecord {
    pub id: String,
    pub thread_id: String,
    pub modality: pl_protocol::studio::StudioAttachmentModality,
    pub media_type: String,
    pub filename: Option<String>,
    pub storage_path: String,
    pub byte_size: u64,
    pub content_sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: i64,
}

impl AttachmentRecord {
    pub(crate) fn session_payload(&self) -> anyhow::Result<pl_core::context::OpaquePayload> {
        Ok(pl_core::context::OpaquePayload::new(
            "studio.attachment",
            1,
            serde_json::to_string(self)?,
        )?)
    }

    pub(crate) fn from_session_entry(
        entry: &pl_core::storage::SessionEntry,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            entry.type_id == "studio.attachment" && entry.schema_version == 1,
            "unsupported attachment payload {} version {}",
            entry.type_id,
            entry.schema_version
        );
        let record: Self = serde_json::from_str(&entry.payload)?;
        anyhow::ensure!(
            record.thread_id == entry.session_id
                && entry.id == format!("pl.resource.{}", record.id),
            "attachment payload ownership differs from its entry"
        );
        Ok(record)
    }
}

/// Product directory status; runtime state is projected from the Thread journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct DirectoryState {
    pub kind: pl_protocol::ThreadStatus,
    pub error: Option<String>,
}
impl DirectoryState {
    pub(in crate::studio) fn kind_label(&self) -> &'static str {
        use pl_protocol::ThreadStatus;
        match self.kind {
            ThreadStatus::Idle => "idle",
            ThreadStatus::Queued => "queued",
            ThreadStatus::Running => "running",
            ThreadStatus::WaitingTool => "waitingTool",
            ThreadStatus::WaitingInteraction => "waitingInteraction",
            ThreadStatus::Cancelling => "cancelling",
            ThreadStatus::Closing => "closing",
            ThreadStatus::Closed => "closed",
            ThreadStatus::Faulted => "faulted",
        }
    }

    pub(in crate::studio) fn idle() -> Self {
        Self {
            kind: pl_protocol::ThreadStatus::Idle,
            error: None,
        }
    }
}
