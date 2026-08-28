use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireManifest<'a> {
    schema_version: u32,
    surface: &'a str,
    fixture_prompt_sha256: &'a str,
    captures: Vec<WireManifestEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireManifestEntry {
    file: String,
    protocol: String,
    request_mode: String,
    model: Option<String>,
    role: &'static str,
    review_scope: Option<&'static str>,
    workstream: Option<&'static str>,
    prompt_sections: Vec<&'static str>,
    tool_names: Vec<String>,
    tool_schema_sha256: String,
}

pub(super) fn finalize(
    artifact_dir: &Path,
    wire_dir: &Path,
    surface: &str,
    prompt_hash: &str,
) -> Result<()> {
    let prompt_bytes = fs::read(artifact_dir.join("fixture-prompt.md"))?;
    let artifact_prompt_hash = format!("{:x}", Sha256::digest(&prompt_bytes));
    anyhow::ensure!(
        artifact_prompt_hash == prompt_hash,
        "fixture prompt artifact does not match the canonical prompt hash"
    );
    let mut paths = Vec::new();
    collect_json_files(wire_dir, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        bail!(
            "real Task acceptance produced no final provider wire captures under `{}`",
            wire_dir.display()
        );
    }

    let captures = paths
        .into_iter()
        .map(|path| capture_entry(artifact_dir, &path))
        .collect::<Result<Vec<_>>>()?;
    assert_complete_role_coverage(&captures)?;
    let manifest = WireManifest {
        schema_version: 1,
        surface,
        fixture_prompt_sha256: prompt_hash,
        captures,
    };
    fs::write(
        artifact_dir.join("wire-request-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

fn assert_complete_role_coverage(captures: &[WireManifestEntry]) -> Result<()> {
    // The first planner request carries the immutable prompt foundation. Task
    // phase facts and hot transcript history only exist after task_status and
    // other tools have completed, so validate that later lifecycle boundary
    // independently instead of requiring impossible future context in request 1.
    require_complete_capture(
        captures,
        "planner",
        None,
        None,
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskPlannerRole",
            "workspaceInstructions",
            "skills",
            "canonicalUserPrompt",
        ],
    )?;
    require_complete_capture(
        captures,
        "planner",
        None,
        None,
        &[
            "canonicalUserPrompt",
            "taskPhaseContext",
            "toolCallHistory",
            "toolResultHistory",
        ],
    )?;
    require_complete_capture(
        captures,
        "executor",
        None,
        Some("normalization"),
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskExecutorRole",
            "workspaceInstructions",
            "skills",
            "workingContext",
            "executorHandoff",
        ],
    )?;
    require_complete_capture(
        captures,
        "executor",
        None,
        Some("validation"),
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskExecutorRole",
            "workspaceInstructions",
            "skills",
            "workingContext",
            "executorHandoff",
        ],
    )?;
    require_complete_capture(
        captures,
        "reviewer",
        Some("delivery"),
        Some("normalization"),
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskReviewerRole",
            "workspaceInstructions",
            "skills",
            "workingContext",
            "deliveryReviewHandoff",
        ],
    )?;
    require_complete_capture(
        captures,
        "reviewer",
        Some("delivery"),
        Some("validation"),
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskReviewerRole",
            "workspaceInstructions",
            "skills",
            "workingContext",
            "deliveryReviewHandoff",
        ],
    )?;
    require_complete_capture(
        captures,
        "reviewer",
        Some("integrated"),
        None,
        &[
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskReviewerRole",
            "workspaceInstructions",
            "skills",
            "workingContext",
            "integratedReviewHandoff",
        ],
    )?;
    Ok(())
}

fn require_complete_capture(
    captures: &[WireManifestEntry],
    role: &str,
    scope: Option<&str>,
    workstream: Option<&str>,
    required_sections: &[&str],
) -> Result<()> {
    let matched = captures.iter().any(|capture| {
        capture.request_mode == "full"
            && capture.role == role
            && capture.review_scope == scope
            && workstream.is_none_or(|workstream| capture.workstream == Some(workstream))
            && capture
                .model
                .as_deref()
                .is_some_and(|model| !model.is_empty())
            && !capture.tool_names.is_empty()
            && (role != "reviewer" || reviewer_tools_are_read_only(&capture.tool_names))
            && required_sections
                .iter()
                .all(|section| capture.prompt_sections.contains(section))
    });
    if !matched {
        bail!(
            "wire captures contain no complete {role}{}{} request with sections {:?}",
            scope.map_or(String::new(), |scope| format!("/{scope}")),
            workstream.map_or(String::new(), |workstream| format!("/{workstream}")),
            required_sections
        );
    }
    Ok(())
}

fn capture_entry(artifact_dir: &Path, path: &Path) -> Result<WireManifestEntry> {
    let capture: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read `{}`", path.display()))?,
    )
    .with_context(|| format!("invalid wire capture `{}`", path.display()))?;
    anyhow::ensure!(
        capture.get("schemaVersion").and_then(Value::as_u64) == Some(1),
        "wire capture has an unsupported schemaVersion"
    );
    let protocol = capture["protocol"]
        .as_str()
        .context("wire capture has no protocol")?;
    anyhow::ensure!(
        matches!(
            protocol,
            "responsesHttp" | "chatCompletions" | "responsesWebSocket"
        ),
        "wire capture has an unsupported protocol `{protocol}`"
    );
    let request_mode = capture["requestMode"]
        .as_str()
        .context("wire capture has no requestMode")?;
    anyhow::ensure!(
        matches!(request_mode, "full" | "incremental"),
        "wire capture has an unsupported requestMode `{request_mode}`"
    );
    let body = capture
        .get("wireBody")
        .context("wire capture has no wireBody")?;
    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut tool_names = tools
        .iter()
        .filter_map(tool_name)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    tool_names.sort();
    tool_names.dedup();
    let role = if tool_names.iter().any(|name| name == "task_transition") {
        "planner"
    } else if tool_names.iter().any(|name| name == "report_completion") {
        "executor"
    } else if tool_names.iter().any(|name| name == "review_exit") {
        "reviewer"
    } else {
        "unknown"
    };
    let message_texts = wire_message_texts(body);
    let prompt_text = prompt_text(body, &message_texts);
    let review_scope = if role == "reviewer"
        && message_texts
            .iter()
            .any(|text| text.contains("## 审查范围\n\nIntegrated\n"))
    {
        Some("integrated")
    } else if role == "reviewer"
        && message_texts
            .iter()
            .any(|text| text.contains("## 审查范围\n\nDelivery\n"))
    {
        Some("delivery")
    } else {
        None
    };
    let prompt_sections =
        detected_prompt_sections(body, &prompt_text, &message_texts, review_scope);
    let workstream = detected_workstream(role, review_scope, &message_texts);
    let tools_json = serde_json::to_vec(&tools)?;
    let relative = path
        .strip_prefix(artifact_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(WireManifestEntry {
        file: relative,
        protocol: protocol.to_owned(),
        request_mode: request_mode.to_owned(),
        model: body["model"].as_str().map(ToOwned::to_owned),
        role,
        review_scope,
        workstream,
        prompt_sections,
        tool_names,
        tool_schema_sha256: format!("{:x}", Sha256::digest(tools_json)),
    })
}

fn reviewer_tools_are_read_only(tool_names: &[String]) -> bool {
    const FORBIDDEN: &[&str] = &[
        "apply_patch",
        "write_file",
        "delete_file",
        "task_transition",
        "task_spawn_executor",
        "task_request_delivery_review",
        "task_record_merge",
        "report_completion",
        "spawn_agent",
        "send_message",
    ];
    tool_names.iter().any(|name| name == "review_exit")
        && !tool_names
            .iter()
            .any(|name| FORBIDDEN.contains(&name.as_str()))
}

fn detected_workstream(
    role: &str,
    review_scope: Option<&str>,
    message_texts: &[&str],
) -> Option<&'static str> {
    let value = match (role, review_scope) {
        ("executor", _) => message_texts.iter().find_map(|text| {
            first_json_value_after(
                text,
                "## Task executor handoff [studio.task_executor_handoff]",
            )
        }),
        ("reviewer", Some("delivery")) => message_texts
            .iter()
            .find_map(|text| first_json_value_after(text, "### 目标 WorkUnit")),
        _ => None,
    }?;
    let blueprint = value.get("blueprint").unwrap_or(&value);
    match blueprint
        .get("taskName")
        .or_else(|| blueprint.get("title"))
        .and_then(Value::as_str)
    {
        Some("normalization-workstream") => Some("normalization"),
        Some("validation-workstream") => Some("validation"),
        _ => workstream_from_scope_hints(blueprint),
    }
}

fn first_json_value_after(text: &str, marker: &str) -> Option<Value> {
    let after_marker = text.get(text.find(marker)? + marker.len()..)?;
    let json_start = after_marker.find('{')?;
    let json = after_marker.get(json_start..)?;
    serde_json::Deserializer::from_str(json)
        .into_iter::<Value>()
        .next()?
        .ok()
}

fn workstream_from_scope_hints(value: &Value) -> Option<&'static str> {
    let hints = value
        .pointer("/scope/scopeHints")
        .or_else(|| value.get("scopeHints"))?
        .as_array()?;
    let has = |path: &str| hints.iter().any(|hint| hint.as_str() == Some(path));
    match (has("src/normalize.rs"), has("src/validate.rs")) {
        (true, false) => Some("normalization"),
        (false, true) => Some("validation"),
        (false, false) | (true, true) => None,
    }
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| tool.pointer("/function/name").and_then(Value::as_str))
}

fn wire_message_texts(body: &Value) -> Vec<&str> {
    body.get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect()
}

fn prompt_text(body: &Value, message_texts: &[&str]) -> String {
    std::iter::once(
        body.get("instructions")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .chain(message_texts.iter().copied())
    .collect::<Vec<_>>()
    .join("\n")
}

fn detected_prompt_sections(
    body: &Value,
    prompt_text: &str,
    message_texts: &[&str],
    review_scope: Option<&str>,
) -> Vec<&'static str> {
    let candidates = [
        ("baseInstructions", "你是 Pure-Lang 的工程协作代理"),
        (
            "globalDeveloperContext",
            "TASK_LIVE_GLOBAL_DEVELOPER_CONTEXT",
        ),
        ("globalUserContext", "TASK_LIVE_GLOBAL_USER_CONTEXT"),
        ("taskPlannerRole", "Task 模式由 root planner"),
        (
            "taskExecutorRole",
            "你当前是 Task root planner 创建的 executor",
        ),
        ("taskReviewerRole", "你当前是 Task runtime 创建的 reviewer"),
        ("workspaceInstructions", "AGENTS.md"),
        ("skills", "task-fixture-rust"),
        ("canonicalUserPrompt", "Normalization workstream"),
    ];
    let mut detected = candidates
        .into_iter()
        .filter_map(|(section, marker)| prompt_text.contains(marker).then_some(section))
        .collect::<Vec<_>>();
    let input = body
        .get("input")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let has_type = |kind: &str| {
        input
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some(kind))
    };
    let has_task_phase = input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && item
                .get("output")
                .and_then(Value::as_str)
                .is_some_and(|output| output.contains("\"completionGate\""))
    });
    if has_task_phase {
        detected.push("taskPhaseContext");
    }
    if has_type("function_call") {
        detected.push("toolCallHistory");
    }
    if has_type("function_call_output") {
        detected.push("toolResultHistory");
    }
    if message_texts
        .iter()
        .any(|text| text.contains("# Current working context"))
        || review_scope.is_some()
    {
        detected.push("workingContext");
    }
    if message_texts.iter().any(|text| {
        text.contains("## Task executor handoff [studio.task_executor_handoff]")
            && first_json_value_after(
                text,
                "## Task executor handoff [studio.task_executor_handoff]",
            )
            .and_then(|value| value.get("blueprint").cloned())
            .is_some()
    }) {
        detected.push("executorHandoff");
    }
    match review_scope {
        Some("delivery") => detected.push("deliveryReviewHandoff"),
        Some("integrated") => detected.push("integratedReviewHandoff"),
        Some(_) | None => {}
    }
    detected
}

fn collect_json_files(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .with_context(|| format!("failed to read capture directory `{}`", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_json_files(&path, output)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_chat_and_responses_tool_names() {
        assert_eq!(
            tool_name(&serde_json::json!({"name": "task_status"})),
            Some("task_status")
        );
        assert_eq!(
            tool_name(&serde_json::json!({"function": {"name": "review_exit"}})),
            Some("review_exit")
        );
    }

    #[test]
    fn role_sections_must_coexist_in_the_same_full_wire_request() {
        let planner_sections = [
            "baseInstructions",
            "globalDeveloperContext",
            "globalUserContext",
            "taskPlannerRole",
            "workspaceInstructions",
            "skills",
            "canonicalUserPrompt",
        ];
        let captures = planner_sections
            .iter()
            .map(|section| WireManifestEntry {
                file: format!("{section}.json"),
                protocol: "responsesHttp".to_string(),
                request_mode: "full".to_string(),
                model: Some("model".to_string()),
                role: "planner",
                review_scope: None,
                workstream: None,
                prompt_sections: vec![section],
                tool_names: vec!["task_transition".to_string()],
                tool_schema_sha256: "hash".to_string(),
            })
            .collect::<Vec<_>>();

        assert!(
            require_complete_capture(&captures, "planner", None, None, &planner_sections).is_err()
        );
    }

    #[test]
    fn reviewer_capture_rejects_workspace_and_task_write_tools() {
        assert!(reviewer_tools_are_read_only(&["review_exit".to_string()]));
        assert!(!reviewer_tools_are_read_only(&[
            "review_exit".to_string(),
            "apply_patch".to_string(),
        ]));
        assert!(!reviewer_tools_are_read_only(&[
            "review_exit".to_string(),
            "task_record_merge".to_string(),
        ]));
    }

    #[test]
    fn executor_workstream_comes_from_the_handoff_blueprint_only() {
        let body = serde_json::json!({
            "instructions": "root prompt mentions src/normalize.rs and src/validate.rs",
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{
                        "type": "input_text",
                        "text": concat!(
                            "# Current working context\n\n",
                            "## Task executor handoff [studio.task_executor_handoff]\n",
                            "{\"blueprint\":{\"taskName\":\"normalization-workstream\",",
                            "\"scope\":{\"scopeHints\":[\"src/normalize.rs\",",
                            "\"tests/normalize.rs\"],\"outOfScope\":[\"src/validate.rs\"]}}}"
                        )
                    }]
                }
            ],
            "tools": [{"name": "report_completion", "description": "validate and normalize"}]
        });
        let texts = wire_message_texts(&body);
        assert_eq!(
            detected_workstream("executor", None, &texts),
            Some("normalization")
        );
    }

    #[test]
    fn delivery_workstream_comes_from_the_target_work_unit_only() {
        let body = serde_json::json!({
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": concat!(
                        "# 审查任务\nBoth src/normalize.rs and src/validate.rs appear in the plan.\n",
                        "## 审查范围\n\nDelivery\n\n### 目标 WorkUnit\n\n",
                        "{\"scopeHints\":[\"src/validate.rs\",\"tests/validate.rs\"]}\n\n",
                        "### 其他 WorkUnit\n{\"scopeHints\":[\"src/normalize.rs\"]}"
                    )
                }]
            }]
        });
        let texts = wire_message_texts(&body);
        assert_eq!(
            detected_workstream("reviewer", Some("delivery"), &texts),
            Some("validation")
        );
    }

    #[test]
    fn tool_schema_text_does_not_impersonate_prompt_sections() {
        let body = serde_json::json!({
            "instructions": "base",
            "input": [],
            "tools": [{
                "name": "tool",
                "description": "acceptanceCriteria Delivery Integrated function_call_output"
            }]
        });
        let texts = wire_message_texts(&body);
        let sections = detected_prompt_sections(&body, &prompt_text(&body, &texts), &texts, None);
        assert!(!sections.contains(&"executorHandoff"));
        assert!(!sections.contains(&"deliveryReviewHandoff"));
        assert!(!sections.contains(&"integratedReviewHandoff"));
        assert!(!sections.contains(&"toolResultHistory"));
    }

    #[test]
    #[ignore = "set PURE_TASK_ARTIFACT_REPLAY to a completed live artifact directory"]
    fn replays_a_completed_live_wire_manifest() {
        let artifact_dir = PathBuf::from(
            std::env::var("PURE_TASK_ARTIFACT_REPLAY")
                .expect("PURE_TASK_ARTIFACT_REPLAY must name an artifact directory"),
        );
        let prompt_hash = std::fs::read_to_string(artifact_dir.join("fixture-prompt.sha256"))
            .expect("fixture prompt hash");
        finalize(
            &artifact_dir,
            &artifact_dir.join("wire"),
            "replay",
            prompt_hash.trim(),
        )
        .expect("wire manifest replay");
    }
}
