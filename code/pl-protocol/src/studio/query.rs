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

/// 会话 Timeline 分页查询；HTTP 与 FRB 指向同一 HistoryReader 窗口语义。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelinePageQuery {
    #[serde(default)]
    pub kind: TimelinePageKind,
    /// 分页锚点：既接受分页返回的版本化游标 token，也接受原始 item identity（`around`）。
    pub item_id: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TimelinePageKind {
    #[default]
    Latest,
    Before,
    After,
    Around,
}

impl TimelinePageQuery {
    pub const DEFAULT_LIMIT: u32 = 100;
    pub const MAX_LIMIT: u32 = 100;

    pub fn limit(&self) -> usize {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT) as usize
    }

    /// 把 wire 查询转换为 canonical 分页请求；缺少锚点或锚点多余都明确拒绝。
    ///
    /// # Errors
    /// Returns a message naming the offending parameter.
    pub fn timeline_query(&self) -> Result<crate::TimelineQuery, String> {
        let Some(item_id) = self.item_id.as_deref() else {
            return match self.kind {
                TimelinePageKind::Latest => Ok(crate::TimelineQuery::Latest),
                _ => Err("timeline kind requires itemId".to_owned()),
            };
        };
        let item_id = item_id.to_owned();
        match self.kind {
            TimelinePageKind::Before => Ok(crate::TimelineQuery::Before { item_id }),
            TimelinePageKind::After => Ok(crate::TimelineQuery::After { item_id }),
            TimelinePageKind::Around => Ok(crate::TimelineQuery::Around { item_id }),
            TimelinePageKind::Latest => {
                Err("timeline itemId is only valid with kind before, after or around".to_owned())
            }
        }
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

/// Fields needed to apply the same directory query to hot Threads and cold catalog entries.
#[derive(Debug, Clone, Copy)]
pub struct ThreadDirectoryMatchFields<'a> {
    pub parent_thread_id: Option<&'a str>,
    pub archived: bool,
    pub project_id: &'a str,
    pub title: &'a str,
    pub status: crate::ThreadStatus,
}

impl<'a> From<&'a crate::Thread> for ThreadDirectoryMatchFields<'a> {
    fn from(thread: &'a crate::Thread) -> Self {
        Self {
            parent_thread_id: thread.parent_thread_id.as_deref(),
            archived: thread.archived,
            project_id: &thread.project_id,
            title: &thread.title,
            status: thread.status,
        }
    }
}

impl ThreadDirectoryQuery {
    pub fn matches(&self, thread: &crate::Thread, project_matches: bool) -> bool {
        self.matches_fields(thread.into(), project_matches)
    }

    pub fn matches_fields(
        &self,
        thread: ThreadDirectoryMatchFields<'_>,
        project_matches: bool,
    ) -> bool {
        use crate::ThreadStatus;
        thread.parent_thread_id.is_none()
            && thread.archived == self.archived
            && self
                .project_id
                .as_ref()
                .is_none_or(|id| id.as_str() == thread.project_id)
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
