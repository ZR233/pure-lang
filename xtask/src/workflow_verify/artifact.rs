//! Live workflow wire-capture validation.
//!
//! The acceptance harness deliberately validates the protocol boundary rather
//! than a provider-specific transcript.  A mode is a preloaded skill and the
//! root agent is the same agent in every mode, so captures must contain the
//! frozen Mode Skill and the mode-specific workflow contract. The validator
//! rejects the removed Task/WorkUnit/review/merge surface everywhere.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
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
    simple_fixture_prompt_sha256: &'a str,
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
    complete_call_count: usize,
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
    simple_prompt_hash: &str,
) -> Result<()> {
    let prompt_bytes = fs::read(artifact_dir.join("fixture-prompt.md"))?;
    let artifact_prompt_hash = format!("{:x}", Sha256::digest(&prompt_bytes));
    ensure!(
        artifact_prompt_hash == prompt_hash,
        "fixture prompt artifact does not match the canonical prompt hash"
    );
    let simple_prompt_bytes = fs::read(artifact_dir.join("simple-fixture-prompt.md"))?;
    let artifact_simple_prompt_hash = format!("{:x}", Sha256::digest(&simple_prompt_bytes));
    ensure!(
        artifact_simple_prompt_hash == simple_prompt_hash,
        "simple fixture prompt artifact does not match the canonical prompt hash"
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
    let mut active_mode = None;
    let mut captures = Vec::with_capacity(paths.len());
    for path in paths {
        let mut capture = capture_entry(artifact_dir, &path)?;
        if capture.mode_id.is_some() {
            active_mode = capture.mode_id;
        } else {
            capture.mode_id = active_mode;
        }
        captures.push(capture);
    }
    assert_completion_receipts(artifact_dir, surface)?;
    assert_workflow_contract(&captures)?;
    fs::write(
        artifact_dir.join("wire-request-manifest.json"),
        serde_json::to_vec_pretty(&WireManifest {
            schema_version: 2,
            surface,
            fixture_prompt_sha256: prompt_hash,
            simple_fixture_prompt_sha256: simple_prompt_hash,
            captures,
        })?,
    )?;
    Ok(())
}

fn assert_completion_receipts(artifact_dir: &Path, surface: &str) -> Result<()> {
    if surface == "gui" {
        return assert_gui_completion_receipts(artifact_dir);
    }
    for mode_id in ["mode.simple", "mode.task"] {
        let path = artifact_dir.join(format!(
            "completion-receipt-{}.json",
            mode_id.replace('.', "-")
        ));
        let receipt: Value = serde_json::from_slice(&fs::read(&path).with_context(|| {
            format!("missing {mode_id} completion receipt `{}`", path.display())
        })?)
        .with_context(|| format!("invalid {mode_id} completion receipt `{}`", path.display()))?;
        ensure!(
            receipt.get("tool").and_then(Value::as_str) == Some("complete"),
            "{mode_id} completion receipt does not identify the complete tool"
        );
        ensure!(
            receipt.get("modeId").and_then(Value::as_str) == Some(mode_id),
            "{mode_id} completion receipt has the wrong mode identity"
        );
        ensure!(
            receipt.pointer("/receipt/status").and_then(Value::as_str) == Some("completed"),
            "{mode_id} completion receipt is not successful"
        );
        ensure!(
            receipt
                .pointer("/receipt/summary")
                .and_then(Value::as_str)
                .is_some_and(|summary| !summary.trim().is_empty()),
            "{mode_id} completion receipt has an empty summary"
        );
    }
    Ok(())
}

fn assert_gui_completion_receipts(artifact_dir: &Path) -> Result<()> {
    for attempt in [1, 2, 3] {
        let path = artifact_dir.join(format!("gui-attempt-{attempt}-workflow-receipt.json"));
        let receipt: Value = serde_json::from_slice(
            &fs::read(&path)
                .with_context(|| format!("missing GUI completion receipt `{}`", path.display()))?,
        )
        .with_context(|| format!("invalid GUI completion receipt `{}`", path.display()))?;
        ensure!(
            receipt.pointer("/complete/name").and_then(Value::as_str) == Some("complete")
                && receipt.pointer("/complete/status").and_then(Value::as_str) == Some("succeeded"),
            "GUI attempt {attempt} has no successful complete tool receipt"
        );
    }
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
    for mode_id in ["mode.simple", "mode.task"] {
        let root = captures.iter().filter(|capture| {
            capture.request_mode == "full"
                && capture.mode_id == Some(mode_id)
                && capture
                    .model
                    .as_deref()
                    .is_some_and(|model| !model.is_empty())
        });
        ensure!(
            root.clone().count() > 0,
            "no full provider request contains the frozen {mode_id} skill"
        );
        ensure!(
            root.clone().any(|capture| {
                capture.prompt_sections.contains(&"modeSkill")
                    && capture.prompt_sections.contains(&"canonicalUserPrompt")
                    && capture.tool_names.iter().any(|name| name == "complete")
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
            "one root request did not preserve {mode_id}, completion, collaboration tools, and ordinary workspace tools"
        );
        if mode_id == "mode.task" {
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| capture.workflow_call_count > 0),
                "mode.task wire captures contain no workflow_state call"
            );
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| capture.workflow_result_count > 0),
                "mode.task wire captures contain no workflow_state result"
            );
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| { capture.prompt_sections.contains(&"workflowProjection") }),
                "mode.task has no subsequent request containing the derived pl.workflow projection"
            );
        } else {
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .all(|capture| {
                        capture.workflow_call_count == 0
                            && capture.workflow_result_count == 0
                            && !capture.prompt_sections.contains(&"workflowProjection")
                    }),
                "mode.simple wire captures contain workflow calls, results, or projection"
            );
        }
    }
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
    let complete_call_count = function_call_count(body, "complete");
    let mode_id = if prompt_text.contains("<preloaded_mode_skill name=\"mode.simple\"") {
        Some("mode.simple")
    } else if prompt_text.contains("<preloaded_mode_skill name=\"mode.task\"") {
        Some("mode.task")
    } else {
        None
    };
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
        complete_call_count,
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
    let unsuccessful_call_ids = unsuccessful_tool_call_ids(body);
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
        let call_id = call
            .get("call_id")
            .or_else(|| call.get("id"))
            .or_else(|| call.pointer("/function/id"))
            .and_then(Value::as_str);
        ensure!(
            call_id.is_none_or(|call_id| !unsuccessful_call_ids.contains(call_id)),
            "workflow_state call `{}` failed or was rejected; live prompt acceptance requires every workflow_state call to be accepted",
            call_id.unwrap_or("unknown")
        );
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
        pl_core::tool::validate_workflow_state_wire_arguments(arguments)
            .map_err(|error| anyhow::anyhow!("invalid workflow_state call arguments: {error}"))?;
    }
    Ok(())
}

fn unsuccessful_tool_call_ids(body: &Value) -> HashSet<String> {
    let mut unsuccessful = HashSet::new();
    if let Some(input) = body.get("input").and_then(Value::as_array) {
        for item in input {
            if item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item
                    .get("output")
                    .and_then(Value::as_str)
                    .is_some_and(workflow_tool_result_unsuccessful)
                && let Some(call_id) = item.get("call_id").and_then(Value::as_str)
            {
                unsuccessful.insert(call_id.to_owned());
            }
        }
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for message in messages {
            if message.get("role").and_then(Value::as_str) == Some("tool")
                && message
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(workflow_tool_result_unsuccessful)
                && let Some(call_id) = message.get("tool_call_id").and_then(Value::as_str)
            {
                unsuccessful.insert(call_id.to_owned());
            }
        }
    }
    unsuccessful
}

fn workflow_tool_result_unsuccessful(content: &str) -> bool {
    content.starts_with("Tool execution error")
        || serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|result| result.get("accepted").and_then(Value::as_bool))
            == Some(false)
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
        ("modeSkill", "<preloaded_mode_skill name=\"mode.simple\""),
        ("modeSkill", "<preloaded_mode_skill name=\"mode.task\""),
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
    fn workflow_contract_requires_both_mode_contracts() {
        let captures = vec![valid_capture("mode.task"), valid_capture("mode.simple")];
        assert!(assert_workflow_contract(&captures).is_ok());
    }

    #[test]
    fn workflow_contract_rejects_removed_tools() {
        let mut task = valid_capture("mode.task");
        task.tool_names.push("task_transition".to_string());
        let captures = vec![task, valid_capture("mode.simple")];
        assert!(assert_workflow_contract(&captures).is_err());
    }

    #[test]
    fn workflow_contract_requires_simple_mode_without_workflow() {
        let captures = vec![valid_capture("mode.simple"), valid_capture("mode.task")];
        assert!(assert_workflow_contract(&captures).is_ok());
    }

    fn valid_capture(mode_id: &'static str) -> WireManifestEntry {
        let task = mode_id == "mode.task";
        let mut tool_names = vec![
            "apply_patch".to_string(),
            "complete".to_string(),
            "exec".to_string(),
            "list_agent_profiles".to_string(),
            "read_file".to_string(),
            "spawn_agent".to_string(),
        ];
        if task {
            tool_names.push("workflow_state".to_string());
        }
        WireManifestEntry {
            file: format!("{mode_id}.json"),
            protocol: "responsesHttp".to_string(),
            request_mode: "full".to_string(),
            model: Some("model".to_string()),
            mode_id: Some(mode_id),
            workflow_call_count: if task { 1 } else { 0 },
            workflow_result_count: if task { 1 } else { 0 },
            complete_call_count: 1,
            prompt_sections: if task {
                vec!["modeSkill", "canonicalUserPrompt", "workflowProjection"]
            } else {
                vec!["modeSkill", "canonicalUserPrompt"]
            },
            tool_names,
            tool_schema_sha256: "hash".to_string(),
        }
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
    fn compile_call_accepts_camel_case_cas_without_terminal_run_id() {
        let body = serde_json::json!({
            "input": [{
                "type": "function_call",
                "name": "workflow_state",
                "arguments": serde_json::json!({
                    "action": "compile",
                    "expectedRevision": 0,
                    "definition": {
                        "title": "Task",
                        "goal": "Complete task",
                        "initialStageId": "working",
                        "stages": [
                            {"id": "working", "title": "Working", "instructions": "Do the work"},
                            {"id": "completed", "title": "Completed", "instructions": "", "terminal": true}
                        ],
                        "transitions": [
                            {"fromStageId": "working", "toStageId": "completed", "when": "Work is verified"}
                        ]
                    }
                }).to_string()
            }]
        });

        validate_workflow_call_arguments(&body)
            .expect("a first compile has no previous workflow run id");
    }

    #[test]
    fn workflow_call_validation_rejects_wrong_types_and_unknown_fields() {
        for arguments in [
            serde_json::json!({
                "action": "compile",
                "expectedRevision": null,
                "definition": {}
            }),
            serde_json::json!({
                "action": "compile",
                "expectedRevision": 0,
                "definition": {
                    "title": "Task",
                    "goal": "Goal",
                    "initialStageId": "done",
                    "stages": [{"id": "done", "title": "Done", "instructions": "", "terminal": true}],
                    "transitions": [],
                    "unexpected": true
                }
            }),
            serde_json::json!({
                "action": "compile",
                "expected_revision": 0,
                "definition": {}
            }),
        ] {
            let body = serde_json::json!({
                "input": [{
                    "type": "function_call",
                    "name": "workflow_state",
                    "arguments": arguments.to_string()
                }]
            });
            assert!(
                validate_workflow_call_arguments(&body).is_err(),
                "{arguments}"
            );
        }
    }

    #[test]
    fn workflow_call_validation_accepts_a_partial_transition_from_a_retry() {
        let body = serde_json::json!({
            "input": [{
                "type": "function_call",
                "name": "workflow_state",
                "arguments": serde_json::json!({
                    "action": "transition",
                    "expectedRunId": "run-1",
                    "expectedRevision": 1,
                    "expectedStageId": "planning",
                    "toStageId": "awaiting_confirmation",
                    "reason": "Plan is ready",
                    "completion": {
                        "evidence": ["plan is complete"]
                    }
                }).to_string()
            }]
        });

        validate_workflow_call_arguments(&body)
            .expect("a retryable partial transition should be shape-validated");
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
            std::fs::read_to_string(artifact_dir.join("simple-fixture-prompt.sha256"))
                .expect("simple fixture prompt hash")
                .trim(),
        )
        .expect("wire manifest replay");
    }
}
