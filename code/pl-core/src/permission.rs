use serde::Deserialize;

use crate::turn::{
    CompileMode, PermissionMode, ToolApprovalDecision, ToolApprovalPolicy, ToolApprovalRequest,
    TurnOptions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionDecision {
    Approved,
    NeedsUserApproval,
    NeedsAiReview,
    Denied { reason: String },
}

pub(crate) fn decide_tool_permission(
    options: &TurnOptions,
    mode: CompileMode,
    request: &ToolApprovalRequest,
) -> PermissionDecision {
    if matches!(options.tool_approval_policy, ToolApprovalPolicy::DenyAll) {
        return PermissionDecision::Denied {
            reason: "tool execution denied by policy".to_string(),
        };
    }

    if matches!(options.tool_approval_policy, ToolApprovalPolicy::Manual)
        || (mode == CompileMode::Plan && request.name == "bash")
    {
        return PermissionDecision::NeedsUserApproval;
    }

    match options.permission_mode {
        PermissionMode::RequestApproval if is_high_risk_tool(&request.name) => {
            PermissionDecision::NeedsUserApproval
        }
        PermissionMode::AutoReview if is_high_risk_tool(&request.name) => {
            PermissionDecision::NeedsAiReview
        }
        PermissionMode::RequestApproval
        | PermissionMode::AutoReview
        | PermissionMode::WorkspaceWrite
        | PermissionMode::FullAccess => PermissionDecision::Approved,
    }
}

pub(crate) fn is_high_risk_tool(name: &str) -> bool {
    matches!(
        name,
        "bash"
            | "write_file"
            | "create_directory"
            | "delete_path"
            | "copy_path"
            | "move_path"
            | "apply_patch"
            | "skill_manage"
    )
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
    fn permission_modes_decide_high_risk_tools() {
        let read = request("read_file");
        let write = request("write_file");
        let bash = request("bash");

        let request_approval =
            TurnOptions::default().with_permission_mode(PermissionMode::RequestApproval);
        assert_eq!(
            decide_tool_permission(&request_approval, CompileMode::Auto, &read),
            PermissionDecision::Approved
        );
        assert_eq!(
            decide_tool_permission(&request_approval, CompileMode::Auto, &write),
            PermissionDecision::NeedsUserApproval
        );

        let auto_review = TurnOptions::default().with_permission_mode(PermissionMode::AutoReview);
        assert_eq!(
            decide_tool_permission(&auto_review, CompileMode::Auto, &bash),
            PermissionDecision::NeedsAiReview
        );

        let workspace = TurnOptions::default().with_permission_mode(PermissionMode::WorkspaceWrite);
        assert_eq!(
            decide_tool_permission(&workspace, CompileMode::Auto, &write),
            PermissionDecision::Approved
        );

        let full_access = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        assert_eq!(
            decide_tool_permission(&full_access, CompileMode::Auto, &bash),
            PermissionDecision::Approved
        );
    }

    #[test]
    fn legacy_manual_and_deny_all_still_wrap_permission_mode() {
        let read = request("read_file");
        let mut manual = TurnOptions::default();
        manual.tool_approval_policy = ToolApprovalPolicy::Manual;
        assert_eq!(
            decide_tool_permission(&manual, CompileMode::Auto, &read),
            PermissionDecision::NeedsUserApproval
        );

        assert_eq!(
            decide_tool_permission(&TurnOptions::deny_all(), CompileMode::Auto, &read),
            PermissionDecision::Denied {
                reason: "tool execution denied by policy".to_string()
            }
        );
    }

    #[test]
    fn plan_mode_bash_requires_user_approval_even_with_full_access() {
        let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        assert_eq!(
            decide_tool_permission(&options, CompileMode::Plan, &request("bash")),
            PermissionDecision::NeedsUserApproval
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
