use std::collections::BTreeMap;
use std::path::{Component, Path};

use crate::MessageRole;
use anyhow::{Context, Result, bail};
use pl_core::path_safety::validate_existing_path_async;
use serde_json::Value;

use super::super::ReviewExitViolation;

#[derive(Debug)]
pub(super) struct ReviewTrace {
    pub(super) read_design: BTreeMap<String, String>,
    pub(super) violations: Vec<ReviewExitViolation>,
}

pub(super) async fn inspect_review_trace(
    session: &crate::AgentSession,
    workspace: &Path,
    design_required: bool,
) -> Result<ReviewTrace> {
    let arguments_by_call_id = tool_call_arguments_by_id(session.messages());
    let mut locator_seen = false;
    let mut read_design = BTreeMap::<String, String>::new();
    let mut violations = Vec::new();
    for message in session.messages() {
        if message.role != MessageRole::Tool {
            continue;
        }
        let record = message
            .tool_result
            .as_ref()
            .context("tool result history has no typed record")?;
        let output = crate::message_content_text(&message.content);
        let arguments = arguments_by_call_id
            .get(&record.call_id)
            .or_else(|| arguments_by_call_id.get(&record.item_id));
        if record.name == "list_files" && successful_output(&output) {
            locator_seen = true;
            continue;
        }
        if record.name == "exec"
            && successful_exec_output(&output)
            && arguments.is_some_and(is_ripgrep_locator)
        {
            locator_seen = true;
            continue;
        }
        if record.name != "read_file" || !successful_read_output(&output) {
            continue;
        }
        if !locator_seen {
            // A premature read cannot satisfy the gate, but it must not poison the
            // entire round: the reviewer may still locate and re-read the document.
            continue;
        }
        let arguments = arguments.context("read_file history has no structured arguments")?;
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .context("read_file history has no path")?;
        if !is_design_read_candidate(path) {
            continue;
        }
        if arguments
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|cwd| !matches!(cwd, "" | "."))
        {
            violations.push(ReviewExitViolation {
                code: "designReadCwdInvalid".to_string(),
                message: "design read 必须使用 workspace 根目录相对路径".to_string(),
                location: Some(path.to_string()),
            });
            continue;
        }
        let normalized = match validate_design_read_path(workspace, path).await {
            Ok(normalized) => normalized,
            Err(error) => {
                violations.push(ReviewExitViolation {
                    code: "designReadPathInvalid".to_string(),
                    message: error.to_string(),
                    location: Some(path.to_string()),
                });
                continue;
            }
        };
        let returned: serde_json::Value = serde_json::from_str(&output)?;
        let text = returned.get("text").and_then(serde_json::Value::as_str);
        if returned.get("path").and_then(serde_json::Value::as_str) != Some(path) || text.is_none()
        {
            violations.push(ReviewExitViolation {
                code: "designReadResultMismatch".to_string(),
                message: "read_file 历史没有与请求匹配的成功 design 结果".to_string(),
                location: Some(path.to_string()),
            });
            continue;
        }
        read_design
            .entry(normalized)
            .or_default()
            .push_str(text.unwrap_or_default());
    }
    append_trace_gate_violations(locator_seen, design_required, &read_design, &mut violations);
    Ok(ReviewTrace {
        read_design,
        violations,
    })
}

fn append_trace_gate_violations(
    locator_seen: bool,
    design_required: bool,
    read_design: &BTreeMap<String, String>,
    violations: &mut Vec<ReviewExitViolation>,
) {
    if !locator_seen {
        violations.push(ReviewExitViolation {
            code: "locatorMissing".to_string(),
            message: "review_exit 前必须使用 list_files 或 exec 执行 rg/rg --files 定位文件"
                .to_string(),
            location: None,
        });
    }
    if design_required && read_design.is_empty() {
        violations.push(ReviewExitViolation {
            code: "designReadMissing".to_string(),
            message: "必须成功读取至少一个相关 design 文档".to_string(),
            location: None,
        });
    }
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

fn successful_exec_output(output: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .is_some_and(|value| {
            value.get("status").and_then(serde_json::Value::as_str) == Some("completed")
                && value.get("exitCode").and_then(serde_json::Value::as_i64) == Some(0)
        })
}

/// assistant 侧 typed 工具调用记录按 call_id/item_id 提供参数。
fn tool_call_arguments_by_id(messages: &[crate::Message]) -> BTreeMap<String, serde_json::Value> {
    let mut arguments = BTreeMap::new();
    for message in messages {
        for call in message.tool_calls.iter().flatten() {
            arguments.insert(call.call_id.clone(), call.arguments.clone());
            arguments.insert(call.item_id.clone(), call.arguments.clone());
        }
    }
    arguments
}

fn is_ripgrep_locator(arguments: &serde_json::Value) -> bool {
    let Some(command) = arguments
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
    else {
        return false;
    };
    let command = command.strip_prefix('&').map(str::trim).unwrap_or(command);
    command
        .split_whitespace()
        .next()
        .map(|executable| executable.trim_matches(['\'', '"']))
        .is_some_and(|executable| matches!(executable, "rg" | "rg.exe"))
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
    use super::*;

    #[test]
    fn design_read_gate_depends_on_workspace_design_requirement() {
        let mut without_design = Vec::new();
        append_trace_gate_violations(true, false, &BTreeMap::new(), &mut without_design);
        assert!(without_design.is_empty());

        let mut with_design = Vec::new();
        append_trace_gate_violations(true, true, &BTreeMap::new(), &mut with_design);
        assert_eq!(with_design.len(), 1);
        assert_eq!(with_design[0].code, "designReadMissing");
    }
}
