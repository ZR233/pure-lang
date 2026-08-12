use std::path::{Path, PathBuf};

use pl_protocol::{PureError, SkillActivation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::SkillsConfig;
use crate::skill::*;

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{
    FunctionToolDefinition, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeEvent,
    deserialize_tool_input,
};

mod actions;

use actions::*;

#[derive(Debug, Clone)]
pub struct SkillsListTool {
    config: SkillsConfig,
}

#[derive(Debug, Clone)]
pub struct SkillViewTool {
    config: SkillsConfig,
}

#[derive(Debug, Clone)]
pub struct SkillManageTool {
    config: SkillsConfig,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillsListInput {
    /// Optional category filter.
    category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SkillViewInput {
    /// Skill name.
    name: String,
    /// Optional support file under references/, templates/, scripts/, or assets/.
    file_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "action")]
enum SkillManageInput {
    Create(CreateSkillInput),
    Patch(PatchSkillInput),
    Edit(EditSkillInput),
    Delete(DeleteSkillInput),
    WriteFile(WriteSkillFileInput),
    RemoveFile(RemoveSkillFileInput),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateSkillInput {
    /// Project skill name.
    name: String,
    /// Full SKILL.md content.
    content: String,
    /// Optional category path.
    category: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchSkillInput {
    /// Project skill name.
    name: String,
    /// Exact existing text to replace.
    old_string: String,
    /// Replacement text.
    new_string: String,
    /// Replace one occurrence or all occurrences.
    replace_mode: Option<ReplaceMode>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditSkillInput {
    /// Project skill name.
    name: String,
    /// Complete replacement SKILL.md content.
    content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeleteSkillInput {
    /// Project skill name.
    name: String,
    /// Optional note identifying where its knowledge was absorbed.
    absorbed_into: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteSkillFileInput {
    /// Project skill name.
    name: String,
    /// Support file under references/, templates/, scripts/, or assets/.
    file_path: String,
    /// Complete support file content.
    file_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoveSkillFileInput {
    /// Project skill name.
    name: String,
    /// Existing support file path.
    file_path: String,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ReplaceMode {
    One,
    All,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillsListOutput<'a> {
    success: bool,
    count: usize,
    project_dir: &'a Path,
    skills: Vec<&'a SkillMetadata>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillViewOutput<'a> {
    success: bool,
    skill: &'a SkillMetadata,
    file_path: String,
    support_files: Vec<crate::skill::SkillFile>,
    content: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillPathOutput<'a> {
    success: bool,
    action: &'static str,
    name: &'a str,
    path: &'a Path,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillPatchOutput<'a> {
    success: bool,
    action: &'static str,
    name: &'a str,
    replacements: usize,
    path: &'a Path,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillDeleteOutput<'a> {
    success: bool,
    action: &'static str,
    name: &'a str,
    absorbed_into: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillFileOutput<'a> {
    success: bool,
    action: &'static str,
    name: &'a str,
    file_path: &'a str,
}

impl SkillsListTool {
    pub fn new(config: SkillsConfig) -> Self {
        Self { config }
    }
}

impl SkillViewTool {
    pub fn new(config: SkillsConfig) -> Self {
        Self { config }
    }
}

impl SkillManageTool {
    pub fn new(config: SkillsConfig) -> Self {
        Self { config }
    }
}

impl Tool for SkillsListTool {
    fn name(&self) -> &str {
        "skills_list"
    }

    fn description(&self) -> &str {
        "List available project, user, and external skills with short metadata."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<SkillsListInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: SkillsListInput = deserialize_tool_input(self.name(), input.arguments)?;
            let catalog = SkillCatalog::discover(context.workspace.root(), &self.config)
                .map_err(|error| tool_error(self.name(), error))?;
            let skills = catalog
                .skills
                .iter()
                .filter(|skill| {
                    input.category.as_deref().is_none_or(|category| {
                        skill
                            .category
                            .as_deref()
                            .is_some_and(|value| value.eq_ignore_ascii_case(category))
                    })
                })
                .collect::<Vec<_>>();
            json_output(SkillsListOutput {
                success: true,
                count: skills.len(),
                project_dir: &catalog.project_dir,
                skills,
                warnings: catalog.warnings,
            })
        })
    }
}

impl Tool for SkillViewTool {
    fn name(&self) -> &str {
        "skill_view"
    }

    fn description(&self) -> &str {
        "Read a full skill or one of its support files. Call this before using a relevant skill."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<SkillViewInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let turn_id = input.session_id.clone();
            let tool_id = input.tool_id.clone();
            let input: SkillViewInput = deserialize_tool_input(self.name(), input.arguments)?;
            let catalog = SkillCatalog::discover(context.workspace.root(), &self.config)
                .map_err(|error| tool_error(self.name(), error))?;
            let skill = catalog.find(&input.name).ok_or_else(|| {
                let name = &input.name;
                tool_error(self.name(), format!("skill not found: {name}"))
            })?;
            let read = read_skill_file(skill, input.file_path.as_deref())
                .map_err(|error| tool_error(self.name(), error))?;
            bump_project_view(&catalog.project_dir, skill)
                .map_err(|error| tool_error(self.name(), error))?;
            let support_files = if read.is_main {
                list_support_files(&skill.path).map_err(|error| tool_error(self.name(), error))?
            } else {
                Vec::new()
            };
            let activation = skill_activation(skill, &turn_id, &tool_id);
            json_output_with_events(
                SkillViewOutput {
                    success: true,
                    skill,
                    file_path: read.file_path,
                    support_files,
                    content: read.content,
                },
                vec![ToolRuntimeEvent::SkillActivated { activation }],
            )
        })
    }
}

impl Tool for SkillManageTool {
    fn name(&self) -> &str {
        "skill_manage"
    }

    fn description(&self) -> &str {
        "Create, patch, edit, delete, or manage support files for project skills. Writes only to the current workspace skills directory."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<SkillManageInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            context.ensure_workspace_writable()?;
            let input: SkillManageInput = deserialize_tool_input(self.name(), input.arguments)?;
            let catalog = SkillCatalog::discover(context.workspace.root(), &self.config)
                .map_err(|error| tool_error(self.name(), error))?;
            match input {
                SkillManageInput::Create(input) => create_skill(self.name(), &catalog, input),
                SkillManageInput::Patch(input) => patch_skill(self.name(), &catalog, input),
                SkillManageInput::Edit(input) => edit_skill(self.name(), &catalog, input),
                SkillManageInput::Delete(input) => delete_skill(self.name(), &catalog, input),
                SkillManageInput::WriteFile(input) => {
                    write_support_file(self.name(), &catalog, input)
                }
                SkillManageInput::RemoveFile(input) => {
                    remove_support_file(self.name(), &catalog, input)
                }
            }
        })
    }
}

fn json_output(value: impl Serialize) -> Result<ToolOutput, PureError> {
    json_output_with_events(value, Vec::new())
}

fn json_output_with_events(
    value: impl Serialize,
    runtime_events: Vec<ToolRuntimeEvent>,
) -> Result<ToolOutput, PureError> {
    let description = serde_json::to_string_pretty(&value)?;
    let stdout = TruncatedOutput {
        original_length: description.len(),
        content: description,
        was_truncated: false,
    };
    Ok(ToolOutput {
        description: stdout.content.clone(),
        truncated: OutputTruncation {
            stdout,
            stderr: TruncatedOutput::empty(),
        },
        output_file: PathBuf::new(),
        exit_code: Some(0),
        timed_out: false,
        runtime_events,
    })
}

fn skill_activation(skill: &SkillMetadata, turn_id: &str, tool_call_id: &str) -> SkillActivation {
    SkillActivation {
        name: skill.name.clone(),
        source: skill_source_label(skill.source).to_string(),
        path: skill.path.to_string_lossy().to_string(),
        turn_id: turn_id.to_string(),
        tool_call_id: tool_call_id.to_string(),
        activated_at: unix_seconds(),
    }
}

fn skill_source_label(source: SkillSourceKind) -> &'static str {
    match source {
        SkillSourceKind::Project => "project",
        SkillSourceKind::User => "user",
        SkillSourceKind::System => "system",
        SkillSourceKind::External => "external",
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests;
