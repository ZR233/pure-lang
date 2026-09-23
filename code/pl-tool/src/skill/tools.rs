use std::path::Path;
use std::result::Result;
use std::sync::Arc;

use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::skill::*;

use crate::workspace::ToolWorkspace;

mod actions;
mod thread;
pub use thread::{ThreadSkillKind, ThreadSkillManageTool, ThreadSkillTool};
mod text_escape;
use crate::tool_error;

use actions::*;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillsListInput {
    /// Optional category filter.
    category: Option<String>,
    /// Optional natural-language name and description query.
    query: Option<String>,
    /// Maximum query results. Defaults to 10 and must be between 1 and 50.
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillViewInput {
    /// Skill name.
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Optional support file under references/, templates/, scripts/, or assets/.
    file_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillTargetInput {
    /// Project skill name.
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum SkillManageInput {
    Create(CreateSkillInput),
    Patch(PatchSkillInput),
    Edit(EditSkillInput),
    Delete(DeleteSkillInput),
    WriteFile(WriteSkillFileInput),
    RemoveFile(RemoveSkillFileInput),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSkillInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Full SKILL.md content.
    content: String,
    /// Optional category path.
    category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchSkillInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Exact existing text to replace.
    old_string: String,
    /// Replacement text.
    new_string: String,
    /// Replace one occurrence or all occurrences.
    replace_mode: Option<ReplaceMode>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditSkillInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Complete replacement SKILL.md content.
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteSkillInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Optional note identifying where its knowledge was absorbed.
    absorbed_into: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteSkillFileInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Support file under references/, templates/, scripts/, or assets/.
    file_path: String,
    /// Complete support file content.
    file_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveSkillFileInput {
    #[serde(flatten)]
    target: SkillTargetInput,
    /// Existing support file path.
    file_path: String,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReplaceMode {
    One,
    All,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillsListOutput<'a> {
    success: bool,
    count: usize,
    truncated: bool,
    skills: Vec<SkillModelSummary<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillModelSummary<'a> {
    name: &'a str,
    description: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillViewOutput {
    success: bool,
    skill: SkillSummary,
    file_path: String,
    resource_base: SkillResourceBase,
    resource_hint: String,
    content: String,
}

/// Reads the identity of a previously loaded Skill from its frozen result.
///
/// # Errors
/// Rejects unsupported formats or malformed saved data without reading current Skill files.
pub fn saved_skill_name(payload: &pl_core::context::OpaquePayload) -> Result<String, PureError> {
    if payload.format() != "pl.tool.skill-view" || payload.version() != 1 {
        return Err(PureError::ConfigError(
            "unsupported saved Skill view".into(),
        ));
    }
    let view: SkillViewOutput = serde_json::from_str(payload.content())?;
    Ok(view.skill.name)
}

/// Decodes a successful main-document load without consulting the current catalog.
///
/// # Errors
/// Rejects malformed supported receipts; unrelated formats and support resources return None.
pub fn saved_skill_activation(
    payload: &pl_core::context::OpaquePayload,
    turn_id: String,
    cause: pl_protocol::SkillActivationCause,
) -> Result<Option<pl_protocol::SkillActivation>, PureError> {
    if payload.format() != "pl.tool.skill-view" || payload.version() != 1 {
        return Ok(None);
    }
    let view: SkillViewOutput = serde_json::from_str(payload.content())?;
    if !view.success || !is_main_skill_path(&view.file_path) {
        return Ok(None);
    }
    Ok(Some(pl_protocol::SkillActivation {
        name: view.skill.name,
        source: super::provider::source_label(view.skill.source).into(),
        provider_id: view.skill.provider_id.as_str().into(),
        resource_base: super::provider::activation_resource_base(&view.resource_base),
        turn_id,
        cause,
        activated_at: 0,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillPathOutput<'a> {
    #[serde(flatten)]
    action: SkillActionOutput<'a>,
    path: &'a Path,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillPatchOutput<'a> {
    #[serde(flatten)]
    action: SkillActionOutput<'a>,
    replacements: usize,
    path: &'a Path,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillDeleteOutput<'a> {
    #[serde(flatten)]
    action: SkillActionOutput<'a>,
    absorbed_into: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillActionOutput<'a> {
    success: bool,
    action: &'static str,
    name: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillFileOutput<'a> {
    #[serde(flatten)]
    action: SkillActionOutput<'a>,
    file_path: &'a str,
}

fn list_snapshot(
    catalog: &FrozenSkillCatalog,
    input: SkillsListInput,
) -> Result<SkillsListOutput<'_>, PureError> {
    let snapshot = catalog.snapshot();
    let query = input
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty());
    let (selected, truncated) = if let Some(query) = query {
        let limit = input.limit.unwrap_or(10);
        if !(1..=50).contains(&limit) {
            return Err(tool_error("skills_list", "limit must be between 1 and 50"));
        }
        let selection = SkillSelector.select(
            &snapshot.skills,
            SkillSelectionRequest {
                query,
                limit,
                category: input.category.as_deref(),
                excluded_names: &[],
                model_invocable_only: true,
            },
        );
        let truncated = selection.truncated();
        (selection.matches, truncated)
    } else {
        let mut skills = snapshot
            .skills
            .iter()
            .filter(|skill| skill.invocation.model_invocable)
            .filter(|skill| {
                input.category.as_deref().is_none_or(|category| {
                    skill
                        .category
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(category))
                })
            })
            .collect::<Vec<_>>();
        skills.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| left.name.cmp(&right.name))
        });
        (skills, false)
    };
    let skills = selected
        .into_iter()
        .map(|skill| SkillModelSummary {
            name: &skill.name,
            description: &skill.description,
        })
        .collect::<Vec<_>>();
    Ok(SkillsListOutput {
        success: true,
        count: skills.len(),
        truncated,
        skills,
    })
}

fn is_main_skill_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    normalized.is_empty() || normalized == "." || normalized.eq_ignore_ascii_case(SKILL_FILE_NAME)
}

struct ManageRequest {
    input: SkillManageInput,
    cancellation: tokio_util::sync::CancellationToken,
}

async fn manage_snapshot(
    catalog: Arc<FrozenSkillCatalog>,
    workspace: ToolWorkspace,
    request: ManageRequest,
) -> Result<serde_json::Value, PureError> {
    let ManageRequest {
        input,
        cancellation,
    } = request;
    workspace.ensure_workspace_writable()?;
    let _lease = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(tool_error("skill_manage", "operation cancelled before mutation")),
        lease = workspace.write_lock() => lease,
    };
    if cancellation.is_cancelled() {
        return Err(tool_error(
            "skill_manage",
            "operation cancelled before mutation",
        ));
    }
    let frozen = catalog.clone();
    let result = tokio::task::spawn_blocking(move || match input {
        SkillManageInput::Create(input) => {
            create_skill("skill_manage", frozen.snapshot(), &workspace, input)
        }
        SkillManageInput::Patch(input) => {
            patch_skill("skill_manage", frozen.snapshot(), &workspace, input)
        }
        SkillManageInput::Edit(input) => {
            edit_skill("skill_manage", frozen.snapshot(), &workspace, input)
        }
        SkillManageInput::Delete(input) => {
            delete_skill("skill_manage", frozen.snapshot(), &workspace, input)
        }
        SkillManageInput::WriteFile(input) => {
            write_support_file("skill_manage", frozen.snapshot(), &workspace, input)
        }
        SkillManageInput::RemoveFile(input) => {
            remove_support_file("skill_manage", frozen.snapshot(), &workspace, input)
        }
    })
    .await
    .map_err(|source| tool_error("skill_manage", source));
    catalog.invalidate();
    result?
}
