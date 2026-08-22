//! Reviewer 角色的分步响应脚本。

use anyhow::Result;

use super::super::sse::tool_call;

pub(super) fn reviewer_response(step: usize) -> Result<(&'static str, String)> {
    let response = match step % 3 {
        0 => (
            "list_files(design)",
            tool_call(
                &format!("review-list-design-{step}"),
                "list_files",
                serde_json::json!({"path": "design"}),
            ),
        ),
        1 => (
            "read_file(design)",
            tool_call(
                &format!("review-read-design-{step}"),
                "read_file",
                serde_json::json!({"path": "design/task-flow.md"}),
            ),
        ),
        2 => (
            "review_exit(pass)",
            tool_call(
                &format!("review-pass-{step}"),
                "review_exit",
                serde_json::json!({
                    "verdict": "pass",
                    "summary": "Implementation matches the reviewed offline Task contract.",
                    "designReferences": [{"path": "design/task-flow.md", "section": "Offline Task Flow"}],
                    "findings": [],
                    "fileReviews": [{"path": "src/feature.txt", "reviewed": true}]
                }),
            ),
        ),
        _ => unreachable!(),
    };
    Ok(response)
}
