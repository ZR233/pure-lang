mod complete;
mod inspect;
mod resolve;
mod scope;
mod verify;

use std::sync::Arc;

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::TurnEngine;
use crate::studio::task_coordinator::TaskCoordinator;
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{AgentRuntimeHandle, ToolEffect};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MergeIdInput {
    merge_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadConflictInput {
    merge_id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveConflictInput {
    merge_id: String,
    path: String,
    patch: Option<String>,
    ours: Option<bool>,
    theirs: Option<bool>,
    delete: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContinueConflictInput {
    merge_id: String,
    resolution_summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AbortConflictInput {
    merge_id: String,
    reason: String,
}

impl TaskCoordinator {
    pub(crate) fn register_conflict_tools(
        self: &Arc<Self>,
        core: &mut TurnEngine,
        thread_id: &str,
        runtime: AgentRuntimeHandle,
    ) {
        core.register_tool(self.merge_list_conflicts_tool(thread_id));
        core.register_tool(self.merge_read_conflict_tool(thread_id));
        core.register_tool(self.merge_resolve_file_tool(thread_id));
        core.register_tool(self.merge_verify_tool(thread_id));
        core.register_tool(self.merge_continue_tool(thread_id, runtime));
        core.register_tool(self.merge_abort_tool(thread_id));
    }

    fn merge_list_conflicts_tool(self: &Arc<Self>, thread_id: impl Into<String>) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_list_conflicts",
            "List the exact active task merge conflicts and their resolution state.",
            merge_id_schema(),
            move |input: MergeIdInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let output = coordinator
                        .list_active_conflicts(&thread_id, input.merge_id.trim())
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }

    fn merge_read_conflict_tool(self: &Arc<Self>, thread_id: impl Into<String>) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_read_conflict",
            "Read the durable base, ours, theirs and combined diff for one conflict.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("mergeId", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::required("path", serde_json::json!({"type":"string"})),
            ]),
            move |input: ReadConflictInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let output = coordinator
                        .read_active_conflict(&thread_id, input.merge_id.trim(), input.path.trim())
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }

    fn merge_resolve_file_tool(self: &Arc<Self>, thread_id: impl Into<String>) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_resolve_file",
            "Resolve exactly one durable conflict with patch, ours, theirs, or delete.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("mergeId", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::required("path", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::optional("patch", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::optional("ours", serde_json::json!({"type":"boolean"})),
                ToolInputSchemaField::optional("theirs", serde_json::json!({"type":"boolean"})),
                ToolInputSchemaField::optional("delete", serde_json::json!({"type":"boolean"})),
            ]),
            move |input: ResolveConflictInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let merge_id = input.merge_id.trim().to_string();
                    let path = input.path.trim().to_string();
                    let choice = ConflictResolutionChoice::from_input(input)?;
                    let output = coordinator
                        .resolve_active_conflict(&thread_id, &merge_id, &path, choice)
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }

    fn merge_verify_tool(self: &Arc<Self>, thread_id: impl Into<String>) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_verify",
            "Verify the fully resolved active conflict and persist the resolution attempt.",
            merge_id_schema(),
            move |input: MergeIdInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let output = coordinator
                        .verify_active_conflict(&thread_id, input.merge_id.trim())
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }

    fn merge_continue_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_continue",
            "Commit a verified conflict resolution and atomically accept the executor delivery.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("mergeId", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::required(
                    "resolutionSummary",
                    serde_json::json!({"type":"string"}),
                ),
            ]),
            move |input: ContinueConflictInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                let runtime = runtime.clone();
                async move {
                    let output = coordinator
                        .continue_active_conflict(
                            &thread_id,
                            input.merge_id.trim(),
                            input.resolution_summary.trim(),
                            Some(&runtime),
                        )
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }

    fn merge_abort_tool(self: &Arc<Self>, thread_id: impl Into<String>) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "merge_abort",
            "Abort the exact active conflict, prove restoration, and block the task.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("mergeId", serde_json::json!({"type":"string"})),
                ToolInputSchemaField::required("reason", serde_json::json!({"type":"string"})),
            ]),
            move |input: AbortConflictInput, _context| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let output = coordinator
                        .abort_active_conflict(
                            &thread_id,
                            input.merge_id.trim(),
                            input.reason.trim(),
                        )
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::ConflictWrite)
    }
}

pub(crate) enum ConflictResolutionChoice {
    Patch(String),
    Ours,
    Theirs,
    Delete,
}

impl ConflictResolutionChoice {
    fn from_input(input: ResolveConflictInput) -> Result<Self> {
        let mut choices = Vec::new();
        if let Some(patch) = input.patch.filter(|patch| !patch.trim().is_empty()) {
            choices.push(Self::Patch(patch));
        }
        if input.ours == Some(true) {
            choices.push(Self::Ours);
        }
        if input.theirs == Some(true) {
            choices.push(Self::Theirs);
        }
        if input.delete == Some(true) {
            choices.push(Self::Delete);
        }
        match choices.pop() {
            Some(choice) if choices.is_empty() => Ok(choice),
            _ => bail!("merge_resolve_file requires exactly one resolution strategy"),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Patch(_) => "patch",
            Self::Ours => "ours",
            Self::Theirs => "theirs",
            Self::Delete => "delete",
        }
    }
}

fn merge_id_schema() -> serde_json::Value {
    strict_tool_input_schema([ToolInputSchemaField::required(
        "mergeId",
        serde_json::json!({"type":"string"}),
    )])
}
