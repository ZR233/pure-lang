use std::sync::Arc;

use pl_core::{ToolApprovalCallback, ToolApprovalDecision, ToolApprovalRequest};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::dto::{ToolApprovalRequestPayload, ToolApprovalResolvedPayload};
use crate::state::{ApprovalWaiter, ApprovalWaiters};

pub fn approval_callback(
    approvals: ApprovalWaiters,
    app: AppHandle,
    session_id: String,
) -> ToolApprovalCallback {
    Arc::new(move |request: ToolApprovalRequest| {
        let approvals = approvals.clone();
        let app = app.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            approvals.lock().await.insert(
                request.id.clone(),
                ApprovalWaiter {
                    session_id: session_id.clone(),
                    sender: tx,
                },
            );
            let _ = app.emit(
                "studio-tool-approval-requested",
                ToolApprovalRequestPayload {
                    approval_id: request.id,
                    session_id,
                    name: request.name,
                    arguments: request.arguments,
                    working_directory: request.working_directory,
                    parent_subagent_id: request.parent_subagent_id,
                },
            );

            rx.await.unwrap_or_else(|_| ToolApprovalDecision::Denied {
                reason: "approval channel closed".to_string(),
            })
        })
    })
}

pub async fn resolve_tool_approval(
    approval_id: String,
    decision: ToolApprovalDecision,
    app: AppHandle,
    approvals: ApprovalWaiters,
) {
    if let Some(waiter) = approvals.lock().await.remove(&approval_id) {
        let _ = waiter.sender.send(decision.clone());
    }
    let (decision_label, reason) = match decision {
        ToolApprovalDecision::Approved => ("approved".to_string(), None),
        ToolApprovalDecision::Denied { reason } => ("denied".to_string(), Some(reason)),
    };
    let _ = app.emit(
        "studio-tool-approval-resolved",
        ToolApprovalResolvedPayload {
            approval_id,
            decision: decision_label,
            reason,
        },
    );
}

pub async fn deny_session_approvals(
    session_id: &str,
    reason: &str,
    app: &AppHandle,
    approvals: ApprovalWaiters,
) {
    let denied = {
        let mut approvals = approvals.lock().await;
        let approval_ids: Vec<String> = approvals
            .iter()
            .filter(|(_, waiter)| waiter.session_id == session_id)
            .map(|(approval_id, _)| approval_id.clone())
            .collect();
        approval_ids
            .into_iter()
            .filter_map(|approval_id| {
                approvals
                    .remove(&approval_id)
                    .map(|waiter| (approval_id, waiter))
            })
            .collect::<Vec<_>>()
    };

    for (approval_id, waiter) in denied {
        let decision = ToolApprovalDecision::Denied {
            reason: reason.to_string(),
        };
        let _ = waiter.sender.send(decision);
        let _ = app.emit(
            "studio-tool-approval-resolved",
            ToolApprovalResolvedPayload {
                approval_id,
                decision: "denied".to_string(),
                reason: Some(reason.to_string()),
            },
        );
    }
}
