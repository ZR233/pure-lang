//! Product permission modes, approval requests and automatic review contracts.
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
/// 会话级权限模式。
///
/// Pure v1 只实现本地策略层，不提供 OS 沙箱。该模式决定 workspace 外访问
/// 是请求用户审批、请求 reviewer 审批，还是直接放行。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    #[default]
    RequestApproval,
    AutoReview,
    FullAccess,
}

impl PermissionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::RequestApproval => "request-approval",
            Self::AutoReview => "auto-review",
            Self::FullAccess => "full-access",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "request-approval" => Some(Self::RequestApproval),
            "auto-review" => Some(Self::AutoReview),
            "full-access" => Some(Self::FullAccess),
            _ => None,
        }
    }

    pub fn allows_workspace_escape(self) -> bool {
        matches!(self, Self::FullAccess)
    }

    pub fn is_default(&self) -> bool {
        matches!(self, Self::RequestApproval)
    }
}

/// 单次工具调用审批请求。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub working_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
}

/// 单次工具调用审批结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// Immutable inputs supplied to a host-owned automatic reviewer.
#[derive(Debug, Clone)]
pub struct ToolReviewRequest {
    pub tool: ToolApprovalRequest,
    pub permission_mode: PermissionMode,
    pub workspace_access: pl_tool::approval::WorkspaceAccess,
    pub workspace_root: std::path::PathBuf,
    pub cancellation_token: Option<CancellationToken>,
}

/// Produces an explicit framework decision; response payload parsing belongs to the host.
/// Reviewers must honor the supplied cancellation token and finish request cleanup and accounting
/// before returning. The owner waits for that completion and ignores approvals after cancellation.
pub type ToolReviewCallback =
    Arc<dyn Fn(ToolReviewRequest) -> BoxFuture<'static, ToolApprovalDecision> + Send + Sync>;
