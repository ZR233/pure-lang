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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unchanged_catalog_refresh_keeps_frozen_call_authority_but_new_policy_revokes_it() {
        use pl_core::{
            model::*,
            thread::*,
            tool::opaque::{Registration, Tool},
        };
        struct Calls;
        impl ModelSession for Calls {
            async fn prepare(
                &mut self,
                request: ModelRequest,
            ) -> Result<PreparedModelCall, ModelError> {
                Ok(PreparedModelCall::new(async move {
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        private_context: None,
                        usage: Default::default(),
                        tool_calls: vec![ModelToolCall {
                            call_id: "call".into(),
                            tool_id: "exec".into(),
                            arguments: OpaquePayload::text("command"),
                        }],
                    })
                }))
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }
        #[derive(Debug)]
        struct Echo;
        impl Tool for Echo {
            async fn execute(
                &self,
                _: OpaquePayload,
                _: CallContext,
            ) -> Result<pl_core::tool::ToolOutput, ToolError> {
                Ok(pl_core::tool::ToolOutput::new(
                    OpaquePayload::text("executed"),
                    Vec::new(),
                ))
            }
        }
        let root = tempfile::tempdir().unwrap();
        let workspace = ToolWorkspace::new(pl_tool::workspace::AgentWorkspace::local(root.path()));
        let make = |previous: &std::collections::BTreeMap<String, ExecutionPolicyHandle>| {
            StudioThreadTools::selected(
                crate::resource_store::FileResourceStore::new(root.path().join("resources")),
                vec![
                    Registration::new("exec".into(), OpaquePayload::text("declaration"), Echo)
                        .unwrap(),
                ],
            )
            .with_approval(
                StudioApprovalOptions {
                    mode: PermissionMode::FullAccess,
                    host: ApprovalHost::Local,
                    workspace: workspace.clone(),
                    reviewer: None,
                },
                previous,
            )
        };
        for preserve in [true, false] {
            let thread =
                ThreadHandle::start("refresh-policy".into(), DynModelSession::new(Calls)).unwrap();
            let original = make(&Default::default());
            let policies = original.approval_policies.clone();
            thread
                .register_tools(original.into_registrations())
                .await
                .unwrap();
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: Default::default(),
                })
                .await
                .unwrap();
            let current = if preserve {
                make(&policies)
            } else {
                make(&Default::default())
            };
            thread
                .register_tools(current.into_registrations())
                .await
                .unwrap();
            let result = thread.execute_tool("call".into(), Default::default()).await;
            if preserve {
                assert!(
                    result.is_ok(),
                    "unchanged policy revoked frozen call: {result:?}"
                );
            } else {
                assert!(
                    matches!(&result, Err(ThreadError::Tool(error)) if matches!(error.source.downcast_ref::<ThreadError>(), Some(ThreadError::ToolPermissionRevoked))),
                    "new policy must revoke the old call: {result:?}"
                );
            }
            thread.close().await.unwrap();
        }
    }

    fn context() -> CallContext {
        CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: Default::default(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        }
    }

    #[tokio::test]
    async fn automatic_review_grants_external_scope_only_after_an_explicit_approved_decision() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let input = OpaquePayload::new(
            "application/json",
            1,
            serde_json::json!({"path":outside.path().join("file.txt"),"approved":true}).to_string(),
        )
        .unwrap();
        let denied = StudioExecutionPolicy {
            tool_id: "read_file".into(),
            options: Arc::new(StudioApprovalOptions {
                mode: PermissionMode::AutoReview,
                host: ApprovalHost::Local,
                workspace: ToolWorkspace::new(pl_tool::workspace::AgentWorkspace::local(
                    root.path(),
                )),
                reviewer: Some(Arc::new(|_| {
                    Box::pin(async {
                        ToolApprovalDecision::Denied {
                            reason: "review denied".into(),
                        }
                    })
                })),
            }),
        };
        assert!(denied.authorize(&input, &context()).await.is_err());
        let allowed = StudioExecutionPolicy {
            tool_id: "read_file".into(),
            options: Arc::new(StudioApprovalOptions {
                mode: PermissionMode::AutoReview,
                host: ApprovalHost::Local,
                workspace: ToolWorkspace::new(pl_tool::workspace::AgentWorkspace::local(
                    root.path(),
                )),
                reviewer: Some(Arc::new(|_| {
                    Box::pin(async { ToolApprovalDecision::Approved })
                })),
            }),
        };
        assert!(
            allowed
                .authorize(&input, &context())
                .await
                .unwrap()
                .contains(pl_tool::approval::HOST_WORKSPACE_ACCESS)
        );
    }
}
