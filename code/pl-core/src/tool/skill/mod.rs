use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pl_protocol::{PureError, SkillActivation, SkillActivationCause, SkillActivationResourceBase};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::SkillsConfig;
use crate::skill::*;
use crate::time::unix_seconds;
use crate::tool::cache::ToolCachePolicy;
use crate::turn::ToolEffect;

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{
    DynTool, StaticTool, ToolCallContext, ToolDirective, ToolPolicy, ToolResult, ToolWorkspace,
    tool_error,
};

mod actions;

use actions::*;

#[derive(Debug, Clone)]
pub struct SkillsListTool {
    source: SkillCatalogSource,
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct SkillViewTool {
    source: SkillCatalogSource,
    workspace: ToolWorkspace,
}

#[derive(Debug, Clone)]
pub struct SkillManageTool {
    source: SkillCatalogSource,
    workspace: ToolWorkspace,
}

/// Agent Skill 工具组是否允许修改项目 Skill。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillToolMode {
    /// 只安装目录与读取工具，适用于产品提供的只读或快照目录。
    ReadOnly,
    /// 同时安装项目 Skill 管理工具。
    ProjectWritable,
}

/// 从单个冻结目录构造原生 Skill 工具组。
///
/// 工具共享同一个 `FrozenSkillCatalog`，因此 list、view 与 manage 不会在一个模型 step
/// 内观察到不同 generation。
pub fn skill_tools_from_catalog(
    catalog: Arc<FrozenSkillCatalog>,
    workspace: ToolWorkspace,
    mode: SkillToolMode,
) -> Vec<DynTool> {
    let mut tools = vec![
        SkillsListTool::from_catalog(catalog.clone(), workspace.clone()).into(),
        SkillViewTool::from_catalog(catalog.clone(), workspace.clone()).into(),
    ];
    match mode {
        SkillToolMode::ReadOnly => {}
        SkillToolMode::ProjectWritable => {
            tools.push(SkillManageTool::from_catalog(catalog, workspace).into());
        }
    }
    tools
}

#[derive(Debug, Clone)]
enum SkillCatalogSource {
    Config(SkillsConfig),
    Frozen(Arc<FrozenSkillCatalog>),
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkillViewOutput {
    success: bool,
    skill: SkillSummary,
    file_path: String,
    resource_base: SkillResourceBase,
    resource_hint: String,
    content: String,
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

impl SkillsListTool {
    pub fn new(config: SkillsConfig, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Config(config),
            workspace,
        }
    }

    pub fn from_catalog(catalog: Arc<FrozenSkillCatalog>, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Frozen(catalog),
            workspace,
        }
    }
}

impl SkillViewTool {
    pub fn new(config: SkillsConfig, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Config(config),
            workspace,
        }
    }

    pub fn from_catalog(catalog: Arc<FrozenSkillCatalog>, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Frozen(catalog),
            workspace,
        }
    }
}

impl SkillManageTool {
    pub fn new(config: SkillsConfig, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Config(config),
            workspace,
        }
    }

    pub fn from_catalog(catalog: Arc<FrozenSkillCatalog>, workspace: ToolWorkspace) -> Self {
        Self {
            source: SkillCatalogSource::Frozen(catalog),
            workspace,
        }
    }
}

impl StaticTool for SkillsListTool {
    type Input = SkillsListInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("skills_list"),
            "List or search available skills by name and description before loading one by exact name.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_programmatic_calls()
            .with_cache_policy(ToolCachePolicy::UntilWorkspaceMutation)
    }

    fn execute(
        &self,
        input: SkillsListInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let catalog =
                catalog_for(&self.source, self.workspace.root(), &context, "skills_list").await?;
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
            json_output(SkillsListOutput {
                success: true,
                count: skills.len(),
                truncated,
                skills,
            })
        }
    }
}

impl StaticTool for SkillViewTool {
    type Input = SkillViewInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("skill_view"),
            "Read a full skill or one of its support files. Call this before using a relevant skill.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
            .with_parallel_tool_calls()
            .with_programmatic_calls()
    }

    fn execute(
        &self,
        input: SkillViewInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            let turn_id = context.identity().turn_id.clone();
            let tool_id = context.identity().item_id.clone();
            let catalog =
                catalog_for(&self.source, self.workspace.root(), &context, "skill_view").await?;
            let skill = catalog
                .find(&input.target.name)
                .ok_or_else(|| {
                    let name = &input.target.name;
                    tool_error("skill_view", format!("skill not found: {name}"))
                })?
                .clone();
            if !skill.invocation.model_invocable {
                return Err(tool_error(
                    "skill_view",
                    format!("skill is not model-invocable: {}", skill.name),
                ));
            }
            let cancellation = context.cancellation_token().unwrap_or_default();
            let definition = catalog
                .load(
                    &input.target.name,
                    SkillLoadInvocation::Model,
                    cancellation.clone(),
                )
                .await
                .map_err(|error| tool_error("skill_view", error))?;
            let requested_resource = input
                .file_path
                .as_deref()
                .map(str::trim)
                .filter(|path| !is_main_skill_path(path));
            let (file_path, content) = match requested_resource {
                Some(file_path) => (
                    file_path.to_string(),
                    catalog
                        .read_resource(
                            &input.target.name,
                            file_path,
                            SkillLoadInvocation::Model,
                            cancellation.clone(),
                        )
                        .await
                        .map_err(|error| tool_error("skill_view", error))?,
                ),
                None => (SKILL_FILE_NAME.to_string(), definition.content),
            };
            catalog
                .record_model_view(&input.target.name, cancellation)
                .await
                .map_err(|error| tool_error("skill_view", error))?;
            let activation = skill_activation(skill, &turn_id, &tool_id);
            json_output_with_events(
                SkillViewOutput {
                    success: true,
                    skill: definition.summary.clone(),
                    file_path,
                    resource_base: definition.summary.resource_base,
                    resource_hint: "Pass filePath under references/, templates/, scripts/, or assets/ to read one resource on demand.".to_string(),
                    content,
                },
                vec![ToolDirective::SkillActivated { activation }],
            )
        }
    }
}

fn is_main_skill_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/");
    let normalized = normalized.trim_start_matches("./");
    normalized.is_empty() || normalized == "." || normalized.eq_ignore_ascii_case(SKILL_FILE_NAME)
}

impl StaticTool for SkillManageTool {
    type Input = SkillManageInput;

    fn definition(&self) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(
            crate::tool::ToolName::builtin("skill_manage"),
            "Create, patch, edit, delete, or manage support files for project skills. Writes only to the current workspace skills directory.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite)
    }

    fn execute(
        &self,
        input: SkillManageInput,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult, PureError>> + Send {
        async move {
            self.workspace.ensure_workspace_writable()?;
            let catalog = catalog_for(
                &self.source,
                self.workspace.root(),
                &context,
                "skill_manage",
            )
            .await?;
            let result = match input {
                SkillManageInput::Create(input) => {
                    create_skill("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
                SkillManageInput::Patch(input) => {
                    patch_skill("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
                SkillManageInput::Edit(input) => {
                    edit_skill("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
                SkillManageInput::Delete(input) => {
                    delete_skill("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
                SkillManageInput::WriteFile(input) => {
                    write_support_file("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
                SkillManageInput::RemoveFile(input) => {
                    remove_support_file("skill_manage", catalog.snapshot(), &self.workspace, input)
                }
            };
            if result.is_ok() {
                catalog.invalidate();
            }
            result
        }
    }
}

async fn catalog_for(
    source: &SkillCatalogSource,
    workspace_root: &Path,
    context: &ToolCallContext,
    tool_name: &str,
) -> Result<Arc<FrozenSkillCatalog>, PureError> {
    match source {
        SkillCatalogSource::Config(config) => discover_local_skills(
            workspace_root,
            config,
            None,
            context.cancellation_token().unwrap_or_default(),
        )
        .await
        .map(Arc::new)
        .map_err(|error| tool_error(tool_name, error)),
        SkillCatalogSource::Frozen(catalog) => Ok(catalog.clone()),
    }
}

fn json_output(value: impl Serialize) -> Result<ToolResult, PureError> {
    json_output_with_events(value, Vec::new())
}

fn json_output_with_events(
    value: impl Serialize,
    runtime_events: Vec<ToolDirective>,
) -> Result<ToolResult, PureError> {
    let description = serde_json::to_string_pretty(&value)?;
    let stdout = TruncatedOutput {
        original_length: description.len(),
        content: description,
        was_truncated: false,
    };
    Ok(ToolResult::from_runtime_text(
        stdout.content.clone(),
        OutputTruncation {
            stdout,
            stderr: TruncatedOutput::empty(),
        },
        PathBuf::new(),
        Some(0),
        false,
        runtime_events,
    ))
}

fn skill_activation(skill: SkillMetadata, turn_id: &str, tool_call_id: &str) -> SkillActivation {
    SkillActivation {
        name: skill.name.clone(),
        source: skill_source_label(skill.source).to_string(),
        provider_id: skill.provider_id.as_str().to_string(),
        resource_base: match skill.resource_base {
            SkillResourceBase::Directory { path } => SkillActivationResourceBase::Directory {
                path: path.to_string_lossy().to_string(),
            },
            SkillResourceBase::Url { url } => SkillActivationResourceBase::Url { url },
            SkillResourceBase::Opaque { description } => {
                SkillActivationResourceBase::Opaque { description }
            }
        },
        turn_id: turn_id.to_string(),
        cause: SkillActivationCause::Tool {
            tool_call_id: tool_call_id.to_string(),
        },
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

#[cfg(test)]
mod unit_tests;
