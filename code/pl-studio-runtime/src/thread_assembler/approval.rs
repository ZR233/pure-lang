//! Studio permission modes and review policy over core's typed invocation grants.
use super::StudioThreadTools;
use crate::approval::{
    PermissionMode, ToolApprovalDecision, ToolApprovalRequest, ToolReviewCallback,
    ToolReviewRequest,
};
use pl_core::{
    context::OpaquePayload,
    tool::{
        execution_policy::{ExecutionGrant, ExecutionPolicy, ExecutionPolicyHandle},
        opaque::{CallContext, ToolError},
    },
};
use pl_tool::workspace::ToolWorkspace;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ApprovalHost {
    Local,
    Remote,
}

pub(crate) struct StudioApprovalOptions {
    pub mode: PermissionMode,
    pub host: ApprovalHost,
    pub workspace: ToolWorkspace,
    pub reviewer: Option<ToolReviewCallback>,
}
impl std::fmt::Debug for StudioApprovalOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StudioApprovalOptions")
            .field("mode", &self.mode)
            .field("host", &self.host)
            .field("workspace", &self.workspace)
            .field("reviewer", &self.reviewer.is_some())
            .finish()
    }
}

#[derive(Debug)]
struct StudioExecutionPolicy {
    tool_id: String,
    options: Arc<StudioApprovalOptions>,
}

#[derive(Debug, thiserror::Error)]
#[error("tool permission denied: {0}")]
struct PermissionDenied(String);

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ApprovalPrompt {
    pub name: String,
    pub arguments: OpaquePayload,
    pub working_directory: Option<String>,
}

impl ExecutionPolicy for StudioExecutionPolicy {
    async fn authorize(
        &self,
        input: &OpaquePayload,
        context: &CallContext,
    ) -> Result<ExecutionGrant, ToolError> {
        if context.cancellation.is_cancelled() {
            return Err(ToolError::new(pl_core::thread::ThreadError::Cancelled));
        }
        let may_escape = matches!(self.options.host, ApprovalHost::Local)
            && self
                .options
                .workspace
                .workspace()
                .boundary()
                .allows_host_paths();
        let external_grant =
            || ExecutionGrant::default().with_capability(pl_tool::approval::HOST_WORKSPACE_ACCESS);
        if self.options.mode == PermissionMode::FullAccess {
            return Ok(if may_escape {
                external_grant()
            } else {
                ExecutionGrant::default()
            });
        }
        let name = self.tool_id.clone();
        let raw = input.content().to_owned();
        let root = self.options.workspace.root().to_owned();
        let host = self.options.host;
        let assessment = tokio::task::spawn_blocking(move || match host {
            ApprovalHost::Local => pl_tool::approval::assess(&name, &raw, &root),
            ApprovalHost::Remote => pl_tool::approval::assess_remote(&name, &raw, &root),
        })
        .await
        .map_err(ToolError::new)?;
        if !assessment.requested_access.allows_external() {
            return Ok(ExecutionGrant::default());
        }
        if !may_escape {
            return Err(ToolError::new(PermissionDenied(
                "the assigned workspace is confined".into(),
            )));
        }
        match self.options.mode {
            PermissionMode::FullAccess => Ok(external_grant()),
            PermissionMode::RequestApproval => {
                let prompt = ApprovalPrompt {
                    name: self.tool_id.clone(),
                    arguments: input.clone(),
                    working_directory: assessment.working_directory.clone(),
                };
                let payload = OpaquePayload::new(
                    "pl.studio.tool-approval",
                    1,
                    serde_json::to_string(&prompt).map_err(ToolError::new)?,
                )
                .map_err(ToolError::new)?;
                let access = context.tasks.as_ref().ok_or_else(|| {
                    ToolError::new(pl_core::thread::ThreadError::TaskAccessDenied)
                })?;
                match access
                    .request_execution_permission(payload, context.cancellation.clone())
                    .await
                    .map_err(ToolError::new)?
                {
                    pl_core::thread::permissions::PermissionDecision::Allow => Ok(external_grant()),
                    pl_core::thread::permissions::PermissionDecision::Deny => {
                        Err(ToolError::new(PermissionDenied("denied by user".into())))
                    }
                }
            }
            PermissionMode::AutoReview => {
                let reviewer = self.options.reviewer.as_ref().ok_or_else(|| {
                    ToolError::new(PermissionDenied(
                        "automatic reviewer is not configured".into(),
                    ))
                })?;
                let decision = reviewer(ToolReviewRequest {
                    tool: ToolApprovalRequest {
                        id: context.call_id.clone(),
                        name: self.tool_id.clone(),
                        arguments: serde_json::Value::String(input.content().to_owned()),
                        working_directory: assessment.working_directory,
                        parent_agent_id: Some(context.thread_id.clone()),
                    },
                    permission_mode: self.options.mode,
                    workspace_access: assessment.requested_access,
                    workspace_root: self.options.workspace.root().to_owned(),
                    cancellation_token: Some(context.cancellation.clone()),
                })
                .await;
                match decision {
                    ToolApprovalDecision::Approved => Ok(external_grant()),
                    ToolApprovalDecision::Denied { reason } => {
                        Err(ToolError::new(PermissionDenied(reason)))
                    }
                }
            }
        }
    }
}

impl StudioThreadTools {
    pub(crate) fn with_approval(
        mut self,
        options: StudioApprovalOptions,
        previous: &std::collections::BTreeMap<String, ExecutionPolicyHandle>,
    ) -> Self {
        let options = Arc::new(options);
        self.registrations = self
            .registrations
            .into_iter()
            .map(|tool| {
                let policy = StudioExecutionPolicy {
                    tool_id: tool.tool_id().to_owned(),
                    options: options.clone(),
                };
                let handle = previous
                    .get(tool.tool_id())
                    .cloned()
                    .unwrap_or_else(|| ExecutionPolicyHandle::new(policy));
                self.approval_policies
                    .insert(tool.tool_id().to_owned(), handle.clone());
                tool.with_execution_policy(handle)
            })
            .collect();
        self
    }
}
