//! Generic runtime recovery diagnostics bridge.

use crate::api::studio::types::*;
use pl_studio_runtime::StudioRecoveryIssue;

pub(crate) fn bridge_recovery_issue(issue: StudioRecoveryIssue) -> BridgeStudioRecoveryIssueDto {
    BridgeStudioRecoveryIssueDto {
        id: issue.id,
        scope: match issue.scope {
            pl_studio_runtime::StudioRecoveryIssueScope::Application => {
                BridgeRecoveryIssueScope::Application
            }
            pl_studio_runtime::StudioRecoveryIssueScope::Project => {
                BridgeRecoveryIssueScope::Project
            }
            pl_studio_runtime::StudioRecoveryIssueScope::Thread => BridgeRecoveryIssueScope::Thread,
        },
        category: match issue.category {
            pl_studio_runtime::StudioRecoveryIssueCategory::ProcessLease => {
                BridgeRecoveryIssueCategory::ProcessLease
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::AgentState => {
                BridgeRecoveryIssueCategory::AgentState
            }
            pl_studio_runtime::StudioRecoveryIssueCategory::Repository => {
                BridgeRecoveryIssueCategory::Repository
            }
        },
        available_actions: vec![match issue.action {
            pl_studio_runtime::StudioRecoveryIssueAction::Retry => BridgeRecoveryIssueAction::Retry,
            pl_studio_runtime::StudioRecoveryIssueAction::CleanupThread => {
                BridgeRecoveryIssueAction::CleanupThread
            }
            pl_studio_runtime::StudioRecoveryIssueAction::RemoveProject => {
                BridgeRecoveryIssueAction::RemoveProject
            }
        }],
        project_id: issue.project_id,
        thread_id: issue.thread_id,
        detail: issue.message,
    }
}
