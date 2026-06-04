use serde::Deserialize;

use crate::tool::WorkspaceAccess;
use crate::turn::{
    CompileMode, PermissionMode, ToolApprovalDecision, ToolApprovalPolicy, ToolApprovalRequest,
    TurnOptions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    Approved { workspace_access: WorkspaceAccess },
    NeedsUserApproval { workspace_access: WorkspaceAccess },
    NeedsAiReview { workspace_access: WorkspaceAccess },
    Denied { reason: String },
}

pub(crate) fn decide_tool_permission(
    options: &TurnOptions,
    mode: CompileMode,
    request: &ToolApprovalRequest,
    requested_access: WorkspaceAccess,
) -> PermissionDecision {
    if matches!(options.tool_approval_policy, ToolApprovalPolicy::DenyAll) {
        return PermissionDecision::Denied {
            reason: "tool execution denied by policy".to_string(),
        };
    }

    if matches!(options.permission_mode, PermissionMode::FullAccess) {
        return PermissionDecision::Approved {
            workspace_access: WorkspaceAccess::ExternalAllowed,
        };
    }

    if matches!(options.tool_approval_policy, ToolApprovalPolicy::Manual)
        || (mode == CompileMode::Plan && request.name == "bash")
    {
        return PermissionDecision::NeedsUserApproval {
            workspace_access: requested_access,
        };
    }

    match options.permission_mode {
        PermissionMode::RequestApproval if requested_access.allows_external() => {
            PermissionDecision::NeedsUserApproval {
                workspace_access: requested_access,
            }
        }
        PermissionMode::AutoReview if requested_access.allows_external() => {
            PermissionDecision::NeedsAiReview {
                workspace_access: requested_access,
            }
        }
        PermissionMode::FullAccess => PermissionDecision::Approved {
            workspace_access: WorkspaceAccess::ExternalAllowed,
        },
        PermissionMode::RequestApproval | PermissionMode::AutoReview => {
            PermissionDecision::Approved {
                workspace_access: WorkspaceAccess::WorkspaceOnly,
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReviewerDecision {
    decision: String,
    reason: Option<String>,
}

pub(crate) fn parse_reviewer_decision(content: &str) -> Result<ToolApprovalDecision, String> {
    let parsed: ReviewerDecision = serde_json::from_str(content.trim())
        .map_err(|error| format!("reviewer returned invalid JSON: {error}"))?;
    match parsed.decision.as_str() {
        "approved" => Ok(ToolApprovalDecision::Approved),
        "denied" => Ok(ToolApprovalDecision::Denied {
            reason: parsed
                .reason
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| "AI reviewer denied the tool call".to_string()),
        }),
        decision => Err(format!(
            "reviewer returned unsupported decision: {decision}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn request(name: &str) -> ToolApprovalRequest {
        ToolApprovalRequest {
            id: "call-1".to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({}),
            working_directory: None,
            parent_agent_id: None,
        }
    }

    #[test]
    fn permission_modes_decide_external_access() {
        let read = request("read_file");
        let write = request("write_file");
        let bash = request("bash");

        let request_approval =
            TurnOptions::default().with_permission_mode(PermissionMode::RequestApproval);
        assert_eq!(
            decide_tool_permission(
                &request_approval,
                CompileMode::Auto,
                &write,
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::Approved {
                workspace_access: WorkspaceAccess::WorkspaceOnly
            }
        );
        assert_eq!(
            decide_tool_permission(
                &request_approval,
                CompileMode::Auto,
                &read,
                WorkspaceAccess::ExternalAllowed,
            ),
            PermissionDecision::NeedsUserApproval {
                workspace_access: WorkspaceAccess::ExternalAllowed
            }
        );

        let auto_review = TurnOptions::default().with_permission_mode(PermissionMode::AutoReview);
        assert_eq!(
            decide_tool_permission(
                &auto_review,
                CompileMode::Auto,
                &bash,
                WorkspaceAccess::ExternalAllowed,
            ),
            PermissionDecision::NeedsAiReview {
                workspace_access: WorkspaceAccess::ExternalAllowed
            }
        );

        let full_access = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        assert_eq!(
            decide_tool_permission(
                &full_access,
                CompileMode::Auto,
                &bash,
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::Approved {
                workspace_access: WorkspaceAccess::ExternalAllowed
            }
        );
    }

    #[test]
    fn legacy_manual_and_deny_all_still_wrap_permission_mode() {
        let read = request("read_file");
        let mut manual = TurnOptions::default();
        manual.tool_approval_policy = ToolApprovalPolicy::Manual;
        assert_eq!(
            decide_tool_permission(
                &manual,
                CompileMode::Auto,
                &read,
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::NeedsUserApproval {
                workspace_access: WorkspaceAccess::WorkspaceOnly
            }
        );

        assert_eq!(
            decide_tool_permission(
                &TurnOptions::deny_all(),
                CompileMode::Auto,
                &read,
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::Denied {
                reason: "tool execution denied by policy".to_string()
            }
        );
    }

    #[test]
    fn plan_mode_bash_requires_user_approval_without_full_access() {
        let options = TurnOptions::default().with_permission_mode(PermissionMode::RequestApproval);
        assert_eq!(
            decide_tool_permission(
                &options,
                CompileMode::Plan,
                &request("bash"),
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::NeedsUserApproval {
                workspace_access: WorkspaceAccess::WorkspaceOnly
            }
        );
    }

    #[test]
    fn full_access_approves_plan_bash_without_user_approval() {
        let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        assert_eq!(
            decide_tool_permission(
                &options,
                CompileMode::Plan,
                &request("bash"),
                WorkspaceAccess::WorkspaceOnly,
            ),
            PermissionDecision::Approved {
                workspace_access: WorkspaceAccess::ExternalAllowed
            }
        );
    }

    #[test]
    fn reviewer_decision_requires_strict_json() {
        assert_eq!(
            parse_reviewer_decision(r#"{"decision":"approved","reason":"ok"}"#).unwrap(),
            ToolApprovalDecision::Approved
        );
        assert_eq!(
            parse_reviewer_decision(r#"{"decision":"denied","reason":"too broad"}"#).unwrap(),
            ToolApprovalDecision::Denied {
                reason: "too broad".to_string()
            }
        );
        assert!(parse_reviewer_decision("approved").is_err());
        assert!(parse_reviewer_decision(r#"{"decision":"maybe"}"#).is_err());
    }
}
