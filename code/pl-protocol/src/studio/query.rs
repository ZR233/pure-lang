//! Studio 查询参数与健康探测响应。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadPageQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl ThreadPageQuery {
    pub const DEFAULT_LIMIT: u32 = 50;
    pub const MAX_LIMIT: u32 = 200;

    pub fn limit(&self) -> usize {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT) as usize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
}

/// Project-scoped directory query; filtering precedes keyset pagination.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadDirectoryQuery {
    pub project_id: Option<String>,
    pub search: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub filter: ThreadDirectoryFilter,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ThreadDirectoryFilter {
    #[default]
    All,
    Running,
    Attention,
}

impl ThreadDirectoryQuery {
    pub fn matches(&self, thread: &crate::Thread, project_matches: bool) -> bool {
        use crate::ThreadStatus;
        thread.parent_thread_id.is_none()
            && thread.archived == self.archived
            && self
                .project_id
                .as_ref()
                .is_none_or(|id| *id == thread.project_id)
            && (project_matches
                || self.search.as_ref().is_none_or(|text| {
                    thread
                        .title
                        .to_lowercase()
                        .contains(&text.trim().to_lowercase())
                }))
            && match self.filter {
                ThreadDirectoryFilter::All => true,
                ThreadDirectoryFilter::Running => matches!(
                    thread.status,
                    ThreadStatus::Queued
                        | ThreadStatus::Running
                        | ThreadStatus::WaitingTool
                        | ThreadStatus::Cancelling
                ),
                ThreadDirectoryFilter::Attention => matches!(
                    thread.status,
                    ThreadStatus::WaitingInteraction | ThreadStatus::Faulted
                ),
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameProjectRequest {
    pub name: String,
}
