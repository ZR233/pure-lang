//! Live workflow wire-capture validation.
//!
//! The acceptance harness deliberately validates the protocol boundary rather
//! than a provider-specific transcript. The root agent is the same agent in
//! every mode, so captures must contain the registered Thread Mode prompt and
//! the mode-specific workflow contract. The validator
//! rejects the removed Task/WorkUnit/review/merge surface everywhere.

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::WorkflowAcceptanceScope;

const FORBIDDEN_TOOLS: &[&str] = &[
    "workflow_state",
    "task_status",
    "task_transition",
    "task_spawn_executor",
    "task_request_delivery_review",
    "task_record_merge",
    "review_exit",
    "plan_exit",
    "submit_plan",
];
const PLAN_TOOLS: &[&str] = &[
    "plan_current",
    "plan_next",
    "plan_history",
    "plan_submit",
    "plan_restart",
];
const WORKFLOW_TOOLS: &[&str] = &[
    "workflow_current",
    "workflow_next",
    "workflow_graph",
    "workflow_history",
    "workflow_transition",
    "workflow_restart",
];
const FORBIDDEN_PROMPT_MARKERS: &[&str] = &[
    "TaskCoordinator",
    "TaskRuntime",
    "WorkUnit",
    "<preloaded_mode_skill",
    "<task_runtime",
    "task_status",
    "task_transition",
    "plan_exit",
    "workflow_state.compile",
    "workflow_state.supersede",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireManifest<'a> {
    schema_version: u32,
    surface: &'a str,
    scope: &'a str,
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
    workflow_transition_call_count: usize,
    request_user_input_call_count: usize,
    plan_current_call_count: usize,
    plan_submit_call_count: usize,
    plan_submit_call_ids: Vec<String>,
    complete_call_count: usize,
    prompt_sections: Vec<&'static str>,
    tool_names: Vec<String>,
    tool_descriptions: BTreeMap<String, String>,
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
    scope: WorkflowAcceptanceScope,
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
    match scope {
        WorkflowAcceptanceScope::Full => {
            assert_completion_receipts(artifact_dir, surface)?;
            assert_workflow_contract(&captures)?;
        }
        WorkflowAcceptanceScope::PlanOnly => {
            assert_gui_plan_only_receipt(artifact_dir)?;
            assert_plan_only_contract(&captures)?;
        }
    }
    fs::write(
        artifact_dir.join("wire-request-manifest.json"),
        serde_json::to_vec_pretty(&WireManifest {
            schema_version: 3,
            surface,
            scope: scope.driver_value(),
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

fn assert_gui_plan_only_receipt(artifact_dir: &Path) -> Result<()> {
    let path = artifact_dir.join("gui-attempt-1-workflow-receipt.json");
    let receipt: Value = serde_json::from_slice(
        &fs::read(&path)
            .with_context(|| format!("missing Plan-only GUI receipt `{}`", path.display()))?,
    )
    .with_context(|| format!("invalid Plan-only GUI receipt `{}`", path.display()))?;
    ensure!(
        receipt.get("scope").and_then(Value::as_str) == Some("plan-only"),
        "Plan-only GUI receipt has the wrong scope"
    );
    ensure!(
        receipt
            .pointer("/completed/planState")
            .and_then(Value::as_str)
            == Some("approved"),
        "Plan-only GUI receipt has no approved canonical Plan"
    );
    ensure!(
        receipt.get("complete").is_some_and(Value::is_null),
        "Plan-only GUI receipt unexpectedly contains complete"
    );
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
            "no full provider request contains the registered {mode_id} prompt"
        );
        ensure!(
            root.clone().any(|capture| {
                capture.prompt_sections.contains(&"threadModePrompt")
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
            let first_root = root
                .clone()
                .next()
                .context("mode.task has no first full provider request")?;
            ensure!(
                first_root.prompt_sections.contains(&"threadModePrompt")
                    && first_root.prompt_sections.contains(&"workflowProjection")
                    && first_root.prompt_sections.contains(&"initialPlanningState"),
                "the first mode.task provider request does not contain the registered Mode prompt and initial planning state"
            );
            ensure!(
                first_root.workflow_call_count == 0
                    && first_root.workflow_result_count == 0
                    && first_root.complete_call_count == 0,
                "the first mode.task provider request already contains tool calls or results"
            );
            ensure!(
                WORKFLOW_TOOLS.iter().all(|required| {
                    first_root
                        .tool_names
                        .iter()
                        .any(|actual| actual == required)
                }),
                "the first mode.task provider request does not expose every registered workflow tool"
            );
            ensure!(
                PLAN_TOOLS.iter().all(|required| {
                    first_root
                        .tool_names
                        .iter()
                        .any(|actual| actual == required)
                }),
                "the first mode.task provider request does not expose every registered Plan tool"
            );
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| capture.workflow_call_count > 0),
                "mode.task wire captures contain no workflow tool call"
            );
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| capture.workflow_result_count > 0),
                "mode.task wire captures contain no workflow tool result"
            );
            ensure!(
                captures
                    .iter()
                    .filter(|capture| capture.mode_id == Some(mode_id))
                    .any(|capture| { capture.prompt_sections.contains(&"workflowProjection") }),
                "mode.task has no request containing the derived pl.workflow projection"
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

fn assert_plan_only_contract(captures: &[WireManifestEntry]) -> Result<()> {
    ensure!(
        captures.iter().all(|capture| {
            capture
                .tool_names
                .iter()
                .all(|name| !FORBIDDEN_TOOLS.contains(&name.as_str()))
        }),
        "Plan-only wire captures contain a removed Task/review/merge tool"
    );
    ensure!(
        captures
            .iter()
            .all(|capture| capture.mode_id != Some("mode.simple")),
        "Plan-only wire captures unexpectedly contain mode.simple"
    );
    let root = captures.iter().filter(|capture| {
        capture.request_mode == "full"
            && capture.mode_id == Some("mode.task")
            && capture
                .model
                .as_deref()
                .is_some_and(|model| !model.is_empty())
    });
    let first_root = root
        .clone()
        .next()
        .context("Plan-only acceptance has no full mode.task provider request")?;
    ensure!(
        first_root.prompt_sections.contains(&"threadModePrompt")
            && first_root.prompt_sections.contains(&"canonicalUserPrompt")
            && first_root.prompt_sections.contains(&"workflowProjection")
            && first_root.prompt_sections.contains(&"initialPlanningState"),
        "the first Plan-only request does not contain Task Mode and initial planning context"
    );
    ensure!(
        first_root.workflow_call_count == 0
            && first_root.plan_current_call_count == 0
            && first_root.plan_submit_call_count == 0
            && first_root.request_user_input_call_count == 0
            && first_root.complete_call_count == 0,
        "the first Plan-only provider request already contains tool calls or results"
    );
    ensure!(
        PLAN_TOOLS.iter().all(|required| {
            first_root
                .tool_names
                .iter()
                .any(|actual| actual == required)
        }),
        "the first Plan-only provider request does not expose every Plan tool"
    );
    let ask_description = first_root
        .tool_descriptions
        .get("request_user_input")
        .context("the first Plan-only request has no request_user_input description")?;
    ensure!(
        ask_description.contains("Never use this tool to ask whether to implement")
            && ask_description.contains("plan_submit instead"),
        "request_user_input description does not exclude Plan approval"
    );
    let submit_description = first_root
        .tool_descriptions
        .get("plan_submit")
        .context("the first Plan-only request has no plan_submit description")?;
    ensure!(
        submit_description.contains("only tool for asking the user to approve implementation")
            && submit_description.contains("request_user_input or final text"),
        "plan_submit description does not own implementation approval"
    );
    ensure!(
        captures
            .iter()
            .all(|capture| capture.request_user_input_call_count == 0),
        "Plan-only acceptance called request_user_input"
    );
    ensure!(
        captures
            .iter()
            .any(|capture| capture.plan_current_call_count > 0),
        "Plan-only acceptance never called plan_current"
    );
    ensure!(
        captures
            .iter()
            .flat_map(|capture| capture.plan_submit_call_ids.iter())
            .collect::<HashSet<_>>()
            .len()
            >= 2,
        "Plan-only acceptance did not submit both the initial and revised Plan"
    );
    ensure!(
        captures
            .iter()
            .all(|capture| capture.workflow_transition_call_count == 0),
        "Plan-only acceptance transitioned the workflow"
    );
    ensure!(
        captures
            .iter()
            .all(|capture| capture.complete_call_count == 0),
        "Plan-only acceptance called complete"
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
    let tool_descriptions = tools
        .iter()
        .filter_map(|tool| {
            let name = tool_name(tool)?;
            matches!(name, "request_user_input" | "plan_submit").then(|| {
                tool_description(tool)
                    .map(|description| (name.to_string(), description.to_string()))
            })?
        })
        .collect::<BTreeMap<_, _>>();
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
    let workflow_call_count = WORKFLOW_TOOLS
        .iter()
        .map(|name| function_call_count(body, name))
        .sum();
    let workflow_result_count = workflow_result_count(body);
    let workflow_transition_call_count = function_call_count(body, "workflow_transition");
    let request_user_input_call_count = function_call_count(body, "request_user_input");
    let plan_current_call_count = function_call_count(body, "plan_current");
    let plan_submit_call_count = function_call_count(body, "plan_submit");
    let plan_submit_call_ids = function_call_ids(body, "plan_submit");
    let complete_call_count = function_call_count(body, "complete");
    let mode_id = if prompt_text.contains("<preloaded_thread_mode_prompt modeId=\"mode.simple\"") {
        Some("mode.simple")
    } else if prompt_text.contains("<preloaded_thread_mode_prompt modeId=\"mode.task\"") {
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
        workflow_transition_call_count,
        request_user_input_call_count,
        plan_current_call_count,
        plan_submit_call_count,
        plan_submit_call_ids,
        complete_call_count,
        prompt_sections,
        tool_names,
        tool_descriptions,
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

fn function_call_ids(body: &Value, name: &str) -> Vec<String> {
    let responses = body
        .get("input")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"));
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
        });
    let mut call_ids = responses
        .chain(chat)
        .filter(|item| tool_name(item) == Some(name))
        .filter_map(|item| {
            item.get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    call_ids.sort_unstable();
    call_ids.dedup();
    call_ids
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
                        output.contains("currentStateId")
                            || output.contains("graphRevision")
                            || output.contains("operationRevision")
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
                    output.contains("operationRevision")
                        || output.contains("currentStateId")
                        || output.contains("graphRevision")
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
        .filter(|call| tool_name(call).is_some_and(|name| WORKFLOW_TOOLS.contains(&name)))
    {
        let call_id = call
            .get("call_id")
            .or_else(|| call.get("id"))
            .or_else(|| call.pointer("/function/id"))
            .and_then(Value::as_str);
        ensure!(
            call_id.is_none_or(|call_id| !unsuccessful_call_ids.contains(call_id)),
            "workflow tool call `{}` failed or was rejected; live prompt acceptance requires every workflow call to be accepted",
            call_id.unwrap_or("unknown")
        );
        let raw = call
            .get("arguments")
            .or_else(|| call.pointer("/function/arguments"))
            .context("workflow call has no arguments")?;
        let arguments = match raw {
            Value::String(raw) => serde_json::from_str::<Value>(raw)
                .context("workflow call arguments are not valid JSON")?,
            Value::Object(_) => raw.clone(),
            _ => bail!("workflow call arguments are not an object"),
        };
        match tool_name(call).expect("filtered workflow tool") {
            "workflow_transition" => pl_core::validate_workflow_transition_arguments(arguments),
            "workflow_restart" => pl_core::validate_workflow_restart_arguments(arguments),
            _ => {
                ensure!(
                    arguments.as_object().is_some_and(serde_json::Map::is_empty),
                    "workflow query tools accept only an empty object"
                );
                continue;
            }
        }
        .map_err(|error| anyhow::anyhow!("invalid workflow call arguments: {error}"))?;
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

fn tool_description(tool: &Value) -> Option<&str> {
    tool.get("description").and_then(Value::as_str).or_else(|| {
        tool.pointer("/function/description")
            .and_then(Value::as_str)
    })
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
        (
            "threadModePrompt",
            "<preloaded_thread_mode_prompt modeId=\"mode.simple\"",
        ),
        (
            "threadModePrompt",
            "<preloaded_thread_mode_prompt modeId=\"mode.task\"",
        ),
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
                    output.contains("operationRevision") || output.contains("currentStateId")
                })
    }) {
        detected.push("workflowResult");
    }
    if message_texts.iter().any(|text| {
        text.contains("pl.workflow")
            || text.contains("# Current workflow")
            || (text.contains("# Current working context") && text.contains("currentState"))
    }) {
        detected.push("workflowProjection");
    }
    if message_texts.iter().any(|text| {
        text.contains("pl.workflow")
            && text.contains("\"currentState\"")
            && text.contains("\"id\": \"planning\"")
    }) {
        detected.push("initialPlanningState");
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
            tool_name(&serde_json::json!({"name": "workflow_transition"})),
            Some("workflow_transition")
        );
        assert_eq!(
            tool_name(&serde_json::json!({"function": {"name": "workflow_current"}})),
            Some("workflow_current")
        );
    }

    #[test]
    fn workflow_contract_requires_both_mode_contracts() {
        let captures = valid_contract_captures();
        assert!(assert_workflow_contract(&captures).is_ok());
    }

    #[test]
    fn plan_only_contract_uses_plan_confirmation_without_general_question() {
        let captures = valid_plan_only_captures();

        assert!(assert_plan_only_contract(&captures).is_ok());
    }

    #[test]
    fn plan_only_contract_rejects_request_user_input() {
        let mut captures = valid_plan_only_captures();
        captures[1].request_user_input_call_count = 1;

        assert!(assert_plan_only_contract(&captures).is_err());
    }

    #[test]
    fn plan_only_contract_requires_two_distinct_plan_submissions() {
        let mut captures = valid_plan_only_captures();
        captures[1].plan_submit_call_ids = vec!["plan-1".to_string()];

        assert!(assert_plan_only_contract(&captures).is_err());
    }

    #[test]
    fn workflow_contract_rejects_removed_tools() {
        let mut task = initial_task_capture();
        task.tool_names.push("task_transition".to_string());
        let captures = vec![
            task,
            progressed_task_capture(),
            valid_capture("mode.simple"),
        ];
        assert!(assert_workflow_contract(&captures).is_err());
    }

    #[test]
    fn workflow_contract_requires_simple_mode_without_workflow() {
        let mut captures = valid_contract_captures();
        let simple = captures
            .iter_mut()
            .find(|capture| capture.mode_id == Some("mode.simple"))
            .unwrap();
        simple.workflow_call_count = 1;
        assert!(assert_workflow_contract(&captures).is_err());
    }

    fn valid_contract_captures() -> Vec<WireManifestEntry> {
        vec![
            initial_task_capture(),
            progressed_task_capture(),
            valid_capture("mode.simple"),
        ]
    }

    fn valid_plan_only_captures() -> Vec<WireManifestEntry> {
        let initial = initial_task_capture();
        let mut progressed = valid_capture("mode.task");
        progressed.workflow_call_count = 0;
        progressed.workflow_result_count = 0;
        progressed.workflow_transition_call_count = 0;
        progressed.request_user_input_call_count = 0;
        progressed.plan_current_call_count = 1;
        progressed.plan_submit_call_count = 2;
        progressed.plan_submit_call_ids = vec!["plan-1".to_string(), "plan-2".to_string()];
        progressed.complete_call_count = 0;
        vec![initial, progressed]
    }

    fn initial_task_capture() -> WireManifestEntry {
        let mut capture = valid_capture("mode.task");
        capture.workflow_call_count = 0;
        capture.workflow_result_count = 0;
        capture.workflow_transition_call_count = 0;
        capture.request_user_input_call_count = 0;
        capture.plan_current_call_count = 0;
        capture.plan_submit_call_count = 0;
        capture.plan_submit_call_ids.clear();
        capture.complete_call_count = 0;
        capture.prompt_sections.push("initialPlanningState");
        capture
    }

    fn progressed_task_capture() -> WireManifestEntry {
        valid_capture("mode.task")
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
            tool_names.extend(WORKFLOW_TOOLS.iter().map(ToString::to_string));
            tool_names.extend(PLAN_TOOLS.iter().map(ToString::to_string));
        }
        let tool_descriptions = BTreeMap::from([
            (
                "request_user_input".to_string(),
                "Never use this tool to ask whether to implement; use plan_submit instead."
                    .to_string(),
            ),
            (
                "plan_submit".to_string(),
                "The only tool for asking the user to approve implementation; do not use request_user_input or final text."
                    .to_string(),
            ),
        ]);
        WireManifestEntry {
            file: format!("{mode_id}.json"),
            protocol: "responsesHttp".to_string(),
            request_mode: "full".to_string(),
            model: Some("model".to_string()),
            mode_id: Some(mode_id),
            workflow_call_count: if task { 1 } else { 0 },
            workflow_result_count: if task { 1 } else { 0 },
            workflow_transition_call_count: if task { 1 } else { 0 },
            request_user_input_call_count: 0,
            plan_current_call_count: if task { 1 } else { 0 },
            plan_submit_call_count: if task { 1 } else { 0 },
            plan_submit_call_ids: if task {
                vec!["plan-1".to_string()]
            } else {
                Vec::new()
            },
            complete_call_count: 1,
            prompt_sections: if task {
                vec![
                    "threadModePrompt",
                    "canonicalUserPrompt",
                    "workflowProjection",
                ]
            } else {
                vec!["threadModePrompt", "canonicalUserPrompt"]
            },
            tool_names,
            tool_descriptions,
            tool_schema_sha256: "hash".to_string(),
        }
    }

    #[test]
    fn tool_schema_text_does_not_impersonate_workflow_projection() {
        let body = serde_json::json!({
            "instructions": "mode.task",
            "input": [],
            "tools": [{"name": "tool", "description": "pl.workflow currentStateId"}]
        });
        let texts = wire_message_texts(&body);
        let sections = detected_prompt_sections(&body, &prompt_text(&body, &texts), &texts);
        assert!(!sections.contains(&"workflowProjection"));
    }

    #[test]
    fn query_call_accepts_only_an_empty_object() {
        let body = serde_json::json!({
            "input": [{
                "type": "function_call",
                "name": "workflow_current",
                "arguments": "{}"
            }]
        });

        validate_workflow_call_arguments(&body).expect("query tools accept an empty object");
    }

    #[test]
    fn workflow_call_validation_rejects_wrong_types_and_unknown_fields() {
        for arguments in [
            serde_json::json!({"definition": {}}),
            serde_json::json!({"action": "compile"}),
            serde_json::json!({"supersede": true}),
        ] {
            let body = serde_json::json!({
                "input": [{
                    "type": "function_call",
                    "name": "workflow_current",
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
    fn workflow_call_validation_accepts_a_transition_with_cas() {
        let body = serde_json::json!({
            "input": [{
                "type": "function_call",
                "name": "workflow_transition",
                "arguments": serde_json::json!({
                    "expectedRunId": "run-1",
                    "expectedRevision": 1,
                    "expectedStateId": "planning",
                    "targetStateId": "editing_documents",
                    "completion": {
                        "reason": "Plan is ready",
                        "summary": "Planning criteria satisfied",
                        "evidence": ["plan is complete"]
                    }
                }).to_string()
            }]
        });

        validate_workflow_call_arguments(&body)
            .expect("a transition with full CAS should be shape-validated");
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
            WorkflowAcceptanceScope::Full,
        )
        .expect("wire manifest replay");
    }
}
