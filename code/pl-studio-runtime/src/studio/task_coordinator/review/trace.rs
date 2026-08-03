use std::collections::BTreeMap;
use std::path::{Component, Path};

use crate::{MessageRole, ToolResultMetadata};
use anyhow::{Context, Result, bail};
use pl_core::path_safety::validate_existing_path_async;

#[derive(Debug)]
pub(super) struct ReviewTrace {
    pub(super) read_design: BTreeMap<String, String>,
}

pub(super) async fn validate_review_trace(
    session: &crate::AgentSession,
    workspace: &Path,
) -> Result<ReviewTrace> {
    let mut locator_seen = false;
    let mut read_design = BTreeMap::<String, String>::new();
    for message in session.messages() {
        if message.role != MessageRole::Tool {
            continue;
        }
        let metadata =
            ToolResultMetadata::from_metadata(&message.metadata).map_err(anyhow::Error::msg)?;
        let output = crate::message_content_text(&message.content);
        if matches!(metadata.tool_name.as_str(), "search_files" | "list_files")
            && successful_output(&output)
        {
            locator_seen = true;
            continue;
        }
        if metadata.tool_name != "read_file" || !successful_read_output(&output) {
            continue;
        }
        if !locator_seen {
            // A premature read cannot satisfy the gate, but it must not poison the
            // entire round: the reviewer may still locate and re-read the document.
            continue;
        }
        let arguments: serde_json::Value = serde_json::from_str(
            metadata
                .tool_call_arguments
                .as_deref()
                .context("read_file history has no structured arguments")?,
        )?;
        let path = arguments
            .get("path")
            .and_then(serde_json::Value::as_str)
            .context("read_file history has no path")?;
        if !is_design_read_candidate(path) {
            continue;
        }
        if arguments
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|cwd| !matches!(cwd, "" | "."))
        {
            bail!("reviewer design read must use workspace-root relative paths");
        }
        let normalized = validate_design_read_path(workspace, path).await?;
        let returned: serde_json::Value = serde_json::from_str(&output)?;
        let text = returned.get("text").and_then(serde_json::Value::as_str);
        if returned.get("path").and_then(serde_json::Value::as_str) != Some(path) || text.is_none()
        {
            bail!("read_file history does not contain a successful matching design result");
        }
        read_design
            .entry(normalized)
            .or_default()
            .push_str(text.unwrap_or_default());
    }
    if !locator_seen {
        bail!("reviewer must use search_files or list_files before review_exit");
    }
    if read_design.is_empty() {
        bail!("reviewer must successfully read at least one relevant design document");
    }
    Ok(ReviewTrace { read_design })
}

fn is_design_read_candidate(path: &str) -> bool {
    path == "design" || path.starts_with("design/") || path.starts_with("design\\")
}

fn successful_output(output: &str) -> bool {
    let trimmed = output.trim();
    !trimmed.is_empty()
        && !trimmed.to_ascii_lowercase().starts_with("error")
        && !trimmed.contains("\"success\":false")
}

fn successful_read_output(output: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .is_some_and(|value| {
            value
                .get("text")
                .and_then(serde_json::Value::as_str)
                .is_some()
        })
}

async fn validate_design_read_path(workspace: &Path, raw: &str) -> Result<String> {
    if raw.is_empty() || raw.trim() != raw || raw.contains('\\') {
        bail!("reviewer design path must be normalized workspace-relative");
    }
    let path = Path::new(raw);
    let components = path.components().collect::<Vec<_>>();
    if path.is_absolute()
        || components.len() < 2
        || !matches!(components.first(), Some(Component::Normal(part)) if *part == "design")
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("reviewer design path must be within design/**");
    }
    let normalized = components
        .iter()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized != raw {
        bail!("reviewer design path is not normalized");
    }
    let current = workspace.join(path);
    validate_existing_path_async(workspace, &current)
        .await
        .with_context(|| format!("reviewer design path is unsafe or does not exist: `{raw}`"))?;
    if !current.is_file() {
        bail!("reviewer design reference is not a file");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[tokio::test]
    async fn read_without_locator_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "pure-review-trace-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("design")).unwrap();
        std::fs::write(root.join("design/guide.md"), "# Guide\n").unwrap();
        let session = crate::AgentSession::from_messages(vec![crate::tool_result_history_message(
            "call-read".to_string(),
            "read_file".to_string(),
            r#"{"path":"design/guide.md"}"#.to_string(),
            r##"{"path":"design/guide.md","text":"# Guide\n"}"##.to_string(),
        )]);

        let error = validate_review_trace(&session, &root).await.unwrap_err();

        assert!(error.to_string().contains("search_files or list_files"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn premature_read_can_recover_by_locating_and_reading_again() {
        let root = std::env::temp_dir().join(format!(
            "pure-review-trace-recovery-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("design")).unwrap();
        std::fs::write(root.join("design/guide.md"), "# Guide\n").unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn live() {}\n").unwrap();
        let read = || {
            crate::tool_result_history_message(
                "call-read".to_string(),
                "read_file".to_string(),
                r#"{"path":"design/guide.md"}"#.to_string(),
                r##"{"path":"design/guide.md","text":"# Guide\n"}"##.to_string(),
            )
        };
        let session = crate::AgentSession::from_messages(vec![
            read(),
            crate::tool_result_history_message(
                "call-list".to_string(),
                "list_files".to_string(),
                r#"{"path":"design"}"#.to_string(),
                r#"{"files":["design/guide.md"]}"#.to_string(),
            ),
            read(),
            crate::tool_result_history_message(
                "call-read-source".to_string(),
                "read_file".to_string(),
                r#"{"path":"src/lib.rs"}"#.to_string(),
                r##"{"path":"src/lib.rs","text":"pub fn live() {}\n"}"##.to_string(),
            ),
        ]);

        let trace = validate_review_trace(&session, &root).await.unwrap();

        assert_eq!(trace.read_design["design/guide.md"], "# Guide\n");
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn source_read_may_use_a_worktree_cwd_after_design_read() {
        let root = std::env::temp_dir().join(format!(
            "pure-review-trace-source-cwd-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("design")).unwrap();
        std::fs::write(root.join("design/guide.md"), "# Guide\n").unwrap();
        let session = crate::AgentSession::from_messages(vec![
            crate::tool_result_history_message(
                "call-list".to_string(),
                "list_files".to_string(),
                r#"{"cwd":".","path":"design"}"#.to_string(),
                r#"{"files":["design/guide.md"]}"#.to_string(),
            ),
            crate::tool_result_history_message(
                "call-read-design".to_string(),
                "read_file".to_string(),
                r#"{"cwd":".","path":"design/guide.md"}"#.to_string(),
                r##"{"path":"design/guide.md","text":"# Guide\n"}"##.to_string(),
            ),
            crate::tool_result_history_message(
                "call-read-source".to_string(),
                "read_file".to_string(),
                r#"{"cwd":".pure/worktrees/task/agent","path":"src/lib.rs"}"#.to_string(),
                r##"{"path":"src/lib.rs","text":"pub fn live() {}\n"}"##.to_string(),
            ),
        ]);

        let trace = validate_review_trace(&session, &root).await.unwrap();

        assert_eq!(trace.read_design["design/guide.md"], "# Guide\n");
        std::fs::remove_dir_all(root).ok();
    }
}
