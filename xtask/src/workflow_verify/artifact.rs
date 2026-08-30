//! Live workflow wire-capture validation.
//!
//! The acceptance harness deliberately validates the protocol boundary rather
//! than a provider-specific transcript.  A mode is a preloaded skill and the
//! root agent is the same agent in every mode, so captures must contain the
//! frozen `mode.task` skill and the single `workflow_state` tool.  The validator
//! rejects the removed Task/WorkUnit/review/merge surface everywhere.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_TOOLS: &[&str] = &[
    "task_status",
    "task_transition",
    "task_spawn_executor",
    "task_request_delivery_review",
    "task_record_merge",
    "review_exit",
    "plan_exit",
];
const FORBIDDEN_PROMPT_MARKERS: &[&str] = &[
    "TaskCoordinator",
    "TaskRuntime",
    "WorkUnit",
    "<task_runtime",
    "task_status",
    "task_transition",
    "plan_exit",
];

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
    mode_id: Option<&'static str>,
    workflow_call_count: usize,
    workflow_result_count: usize,
    prompt_sections: Vec<&'static str>,
    tool_names: Vec<String>,
    tool_schema_sha256: String,
}

pub(super) fn has_captures(wire_dir: &Path) -> Result<bool> {
    let mut paths = Vec::new();
    collect_json_files(wire_dir, &mut paths)?;
    Ok(!paths.is_empty())
}

pub(super) fn finalize(
    artifact_dir: &Path,
    wire_dir: &Path,
    surface: &str,
    prompt_hash: &str,
) -> Result<()> {
    let prompt_bytes = fs::read(artifact_dir.join("fixture-prompt.md"))?;
    let artifact_prompt_hash = format!("{:x}", Sha256::digest(&prompt_bytes));
    ensure!(
        artifact_prompt_hash == prompt_hash,
        "fixture prompt artifact does not match the canonical prompt hash"
    );

    let mut paths = Vec::new();
    collect_json_files(wire_dir, &mut paths)?;
    paths.sort();
    if paths.is_empty() {
        bail!(
            "real workflow acceptance produced no provider wire captures under `{}`",
            wire_dir.display()
        );
    }
    let captures = paths
        .into_iter()
        .map(|path| capture_entry(artifact_dir, &path))
        .collect::<Result<Vec<_>>>()?;
    assert_workflow_contract(&captures)?;
    fs::write(
        artifact_dir.join("wire-request-manifest.json"),
        serde_json::to_vec_pretty(&WireManifest {
            schema_version: 1,
            surface,
            fixture_prompt_sha256: prompt_hash,
            captures,
        })?,
    )?;
    Ok(())
}

fn assert_workflow_contract(captures: &[WireManifestEntry]) -> Result<()> {
    ensure!(
        captures.iter().all(|capture| {
            capture
                .tool_names
                .iter()
                .all(|name| !FORBIDDEN_TOOLS.contains(&name.as_str()))
        }),
        "wire captures contain a removed Task/review/merge tool"
    );
    let root = captures.iter().filter(|capture| {
        capture.request_mode == "full"
            && capture.mode_id == Some("mode.task")
            && capture
                .tool_names
                .iter()
                .any(|name| name == "workflow_state")
            && capture
                .model
                .as_deref()
                .is_some_and(|model| !model.is_empty())
    });
    ensure!(
        root.clone().count() > 0,
        "no full provider request contains the frozen mode.task skill and workflow_state"
    );
    ensure!(
        root.clone().any(|capture| {
            capture.prompt_sections.contains(&"modeSkill")
                && capture.prompt_sections.contains(&"canonicalUserPrompt")
                && capture.tool_names.iter().any(|name| name == "exec")
                && capture.tool_names.iter().any(|name| {
                    matches!(
                        name.as_str(),
                        "read_file" | "apply_patch" | "workspace_file"
                    )
                })
                && capture
                    .tool_names
                    .iter()
                    .any(|name| name == "list_agent_profiles")
                && capture.tool_names.iter().any(|name| name == "spawn_agent")
        }),
        "one root request did not preserve mode.task, the user prompt, collaboration tools, and ordinary workspace tools"
    );
    ensure!(
        captures
            .iter()
            .any(|capture| capture.workflow_call_count > 0),
        "wire captures contain no workflow_state call"
    );
    ensure!(
        captures
            .iter()
            .any(|capture| capture.workflow_result_count > 0),
        "wire captures contain no workflow_state result"
    );
    ensure!(
        captures
            .iter()
            .any(|capture| capture.prompt_sections.contains(&"workflowProjection")),
        "no subsequent request contains the derived pl.workflow projection"
    );
    Ok(())
}

fn capture_entry(artifact_dir: &Path, path: &Path) -> Result<WireManifestEntry> {
    let capture: Value = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("failed to read `{}`", path.display()))?,
    )
    .with_context(|| format!("invalid wire capture `{}`", path.display()))?;
    ensure!(
        capture.get("schemaVersion").and_then(Value::as_u64) == Some(1),
        "wire capture has an unsupported schemaVersion"
    );
    let protocol = capture["protocol"]
        .as_str()
        .context("wire capture has no protocol")?;
    ensure!(
        matches!(
            protocol,
            "responsesHttp" | "chatCompletions" | "responsesWebSocket"
        ),
        "wire capture has an unsupported protocol `{protocol}`"
    );
    let request_mode = capture["requestMode"]
        .as_str()
        .context("wire capture has no requestMode")?;
    ensure!(
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
    let message_texts = wire_message_texts(body);
    let prompt_text = prompt_text(body, &message_texts);
    ensure!(
        FORBIDDEN_PROMPT_MARKERS
            .iter()
            .all(|marker| !prompt_text.contains(marker)),
        "wire capture contains a removed planner/Task prompt marker"
    );
    validate_workflow_call_arguments(body)?;
    let prompt_sections = detected_prompt_sections(body, &prompt_text, &message_texts);
    let workflow_call_count = function_call_count(body, "workflow_state");
    let workflow_result_count = workflow_result_count(body);
    let mode_id = prompt_text
        .contains("name: mode.task")
        .then_some("mode.task")
        .or_else(|| prompt_text.contains("mode.task").then_some("mode.task"));
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
        mode_id,
        workflow_call_count,
        workflow_result_count,
        prompt_sections,
        tool_names,
        tool_schema_sha256: format!("{:x}", Sha256::digest(tools_json)),
    })
}

fn function_call_count(body: &Value, name: &str) -> usize {
    let responses = body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter(|item| tool_name(item) == Some(name))
        .count();
    let chat = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|item| tool_name(item) == Some(name))
        .count();
    responses + chat
}

fn workflow_result_count(body: &Value) -> usize {
    let responses = body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| {
            let kind = item.get("type").and_then(Value::as_str);
            kind == Some("function_call_output")
                && item
                    .get("output")
                    .and_then(Value::as_str)
                    .is_some_and(|output| {
                        output.contains("workflow_state")
                            || output.contains("operationRevision")
                            || output.contains("currentStageId")
                    })
        })
        .count();
    let chat = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .filter(|message| {
            message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|output| {
                    output.contains("operationRevision") || output.contains("currentStageId")
                })
        })
        .count();
    responses + chat
}

fn validate_workflow_call_arguments(body: &Value) -> Result<()> {
    let response_calls = body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"));
    let chat_calls = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| {
            message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        });
    for call in response_calls
        .chain(chat_calls)
        .filter(|call| tool_name(call) == Some("workflow_state"))
    {
        let raw = call
            .get("arguments")
            .or_else(|| call.pointer("/function/arguments"))
            .context("workflow_state call has no arguments")?;
        let arguments = match raw {
            Value::String(raw) => serde_json::from_str::<Value>(raw)
                .context("workflow_state call arguments are not valid JSON")?,
            Value::Object(_) => raw.clone(),
            _ => bail!("workflow_state call arguments are not an object"),
        };
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .context("workflow_state call has no action")?;
        let required = match action {
            "compile" => &["expectedRevision", "expectedRunId", "definition"][..],
            "status" => &[][..],
            "transition" => &[
                "expectedRunId",
                "expectedRevision",
                "expectedStageId",
                "toStageId",
                "reason",
                "completion",
            ][..],
            "supersede" => &[
                "expectedRunId",
                "expectedRevision",
                "expectedStageId",
                "reason",
                "definition",
            ][..],
            other => bail!("workflow_state call uses unknown action `{other}`"),
        };
        ensure!(
            required.iter().all(|field| arguments.get(*field).is_some()),
            "workflow_state {action} call is missing required CAS or payload fields"
        );
    }
    Ok(())
}

fn tool_name(tool: &Value) -> Option<&str> {
    tool.get("name")
        .and_then(Value::as_str)
        .or_else(|| tool.pointer("/function/name").and_then(Value::as_str))
}

fn wire_message_texts(body: &Value) -> Vec<&str> {
    let responses = body
        .get("input")
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
        .filter_map(|content| content.get("text").and_then(Value::as_str));
    let chat = body
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| match message.get("content") {
            Some(Value::String(text)) => vec![text.as_str()],
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect(),
            Some(Value::Null)
            | Some(Value::Bool(_))
            | Some(Value::Number(_))
            | Some(Value::Object(_))
            | None => Vec::new(),
        });
    responses.chain(chat).collect()
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
) -> Vec<&'static str> {
    let candidates = [
        ("baseInstructions", "你是 Pure-Lang 的工程协作代理"),
        ("modeSkill", "mode.task"),
        ("workspaceInstructions", "AGENTS.md"),
        ("canonicalUserPrompt", "normalize_key"),
        ("canonicalUserPrompt", "validate_key"),
    ];
    let mut detected = candidates
        .into_iter()
        .filter_map(|(section, marker)| prompt_text.contains(marker).then_some(section))
        .collect::<Vec<_>>();
    detected.sort_unstable();
    detected.dedup();
    let input = body
        .get("input")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && item
                .get("output")
                .and_then(Value::as_str)
                .is_some_and(|output| {
                    output.contains("operationRevision") || output.contains("currentStageId")
                })
    }) {
        detected.push("workflowResult");
    }
    if message_texts.iter().any(|text| {
        text.contains("pl.workflow")
            || text.contains("# Current workflow")
            || (text.contains("# Current working context") && text.contains("currentStage"))
    }) {
        detected.push("workflowProjection");
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
    fn recognizes_tool_names_without_old_task_aliases() {
        assert_eq!(
            tool_name(&serde_json::json!({"name": "workflow_state"})),
            Some("workflow_state")
        );
        assert_eq!(
            tool_name(&serde_json::json!({"function": {"name": "workflow_state"}})),
            Some("workflow_state")
        );
    }

    #[test]
    fn workflow_contract_requires_mode_skill_and_projection() {
        let captures = vec![WireManifestEntry {
            file: "capture.json".to_string(),
            protocol: "responsesHttp".to_string(),
            request_mode: "full".to_string(),
            model: Some("model".to_string()),
            mode_id: Some("mode.task"),
            workflow_call_count: 1,
            workflow_result_count: 1,
            prompt_sections: vec!["modeSkill", "canonicalUserPrompt", "workflowProjection"],
            tool_names: vec![
                "apply_patch".to_string(),
                "exec".to_string(),
                "list_agent_profiles".to_string(),
                "spawn_agent".to_string(),
                "workflow_state".to_string(),
            ],
            tool_schema_sha256: "hash".to_string(),
        }];
        assert!(assert_workflow_contract(&captures).is_ok());
    }

    #[test]
    fn workflow_contract_rejects_removed_tools() {
        let captures = vec![WireManifestEntry {
            file: "capture.json".to_string(),
            protocol: "responsesHttp".to_string(),
            request_mode: "full".to_string(),
            model: Some("model".to_string()),
            mode_id: Some("mode.task"),
            workflow_call_count: 1,
            workflow_result_count: 1,
            prompt_sections: vec!["modeSkill", "canonicalUserPrompt", "workflowProjection"],
            tool_names: vec![
                "apply_patch".to_string(),
                "exec".to_string(),
                "list_agent_profiles".to_string(),
                "spawn_agent".to_string(),
                "workflow_state".to_string(),
                "task_transition".to_string(),
            ],
            tool_schema_sha256: "hash".to_string(),
        }];
        assert!(assert_workflow_contract(&captures).is_err());
    }

    #[test]
    fn tool_schema_text_does_not_impersonate_workflow_projection() {
        let body = serde_json::json!({
            "instructions": "mode.task",
            "input": [],
            "tools": [{"name": "tool", "description": "pl.workflow currentStageId"}]
        });
        let texts = wire_message_texts(&body);
        let sections = detected_prompt_sections(&body, &prompt_text(&body, &texts), &texts);
        assert!(!sections.contains(&"workflowProjection"));
    }

    #[test]
    #[ignore = "set PURE_WORKFLOW_ARTIFACT_REPLAY to a completed live artifact directory"]
    fn replays_a_completed_live_wire_manifest() {
        let artifact_dir = PathBuf::from(
            std::env::var("PURE_WORKFLOW_ARTIFACT_REPLAY")
                .expect("PURE_WORKFLOW_ARTIFACT_REPLAY must name an artifact directory"),
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
