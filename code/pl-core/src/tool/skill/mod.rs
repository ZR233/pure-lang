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
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::tool::{StaticToolTestExt, ToolInput, deserialize_tool_input};

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-skill-tool-{name}-{stamp}"))
    }

    fn skill_content(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n")
    }

    fn tool_context(_workspace_root: PathBuf) -> ToolCallContext {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        ToolCallContext::test(event_tx)
    }

    fn tool_workspace(workspace_root: &Path) -> ToolWorkspace {
        ToolWorkspace::new(crate::tool::AgentWorkspace::local(
            workspace_root.to_path_buf(),
        ))
    }

    fn write_project_skill(workspace: &Path, name: &str) {
        let skill_dir = workspace.join(".agents/skills").join(name);
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            skill_content(name, "Project skill"),
        )
        .unwrap();
        fs::write(skill_dir.join("references/example.md"), "support").unwrap();
    }

    fn write_project_skill_with_metadata(
        workspace: &Path,
        name: &str,
        description: &str,
        category: &str,
    ) {
        let skill_dir = workspace.join(".agents/skills").join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: {description}\ncategory: {category}\n---\n# {name}\n"
        ),
    )
    .unwrap();
    }

    #[tokio::test]
    async fn read_only_catalog_tool_group_omits_project_mutation() {
        let workspace = tempfile::tempdir().unwrap();
        write_project_skill(workspace.path(), "readonly");
        let catalog = Arc::new(
            discover_local_skills(
                workspace.path(),
                &SkillsConfig::default(),
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap(),
        );

        let tools = skill_tools_from_catalog(
            catalog,
            tool_workspace(workspace.path()),
            SkillToolMode::ReadOnly,
        );

        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.definition().name().wire_name())
                .collect::<Vec<_>>(),
            ["skills_list", "skill_view"]
        );
    }

    #[tokio::test]
    async fn skill_view_reads_host_registered_project_snapshot_outside_default_project_dir() {
        let source = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        write_project_skill(source.path(), "review-single-pr");
        let registry = crate::skill::SkillRegistry::new();
        let provider = crate::skill::FileSystemSkillProvider::from_directories(
            "product-project-skills",
            vec![crate::skill::SkillDirectorySource::new(
                source.path().join(".agents/skills"),
                crate::skill::SkillSourceKind::Project,
            )],
        )
        .unwrap();
        let _registration = registry.register(Arc::new(provider)).unwrap();
        let catalog = registry
            .discover(crate::skill::SkillProviderRequest {
                workspace_root: workspace.path().to_path_buf(),
                config: SkillsConfig::default(),
                system_dir: None,
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let tool = SkillViewTool::from_catalog(Arc::new(catalog), tool_workspace(workspace.path()));

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "review-single-pr"}),
                },
                tool_context(workspace.path().to_path_buf()),
            )
            .await
            .unwrap();

        assert_eq!(activation_from_output(&output).name, "review-single-pr");
        assert!(
            !source
                .path()
                .join(".agents/skills/review-single-pr/.usage.json")
                .exists()
        );
    }

    #[cfg(unix)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_link(target: &Path, link: &Path) {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    }

    #[cfg(unix)]
    fn remove_directory_link(link: &Path) {
        fs::remove_file(link).unwrap();
    }

    #[cfg(windows)]
    fn remove_directory_link(link: &Path) {
        fs::remove_dir(link).unwrap();
    }

    fn activation_from_output(output: &ToolResult) -> &SkillActivation {
        let Some(ToolDirective::SkillActivated { activation }) = output.runtime_events.first()
        else {
            panic!("expected skill activation")
        };
        activation
    }

    #[derive(Debug, serde::Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    struct SkillViewOutputSnapshot {
        success: bool,
        skill: crate::skill::SkillSummary,
        file_path: String,
        resource_base: crate::skill::SkillResourceBase,
        resource_hint: String,
        content: String,
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SkillsListOutputSnapshot {
        count: usize,
        truncated: bool,
        skills: Vec<SkillsListRowSnapshot>,
    }

    #[derive(Debug, serde::Deserialize)]
    struct SkillsListRowSnapshot {
        name: String,
        description: String,
    }

    #[tokio::test]
    async fn skills_list_preserves_full_listing_and_supports_ranked_search() {
        let workspace = temp_dir("list-search");
        let category = "selector-test-category";
        write_project_skill_with_metadata(
            &workspace,
            "release-build-triage",
            "Diagnose Rust release linker and Cargo profile failures.",
            category,
        );
        write_project_skill_with_metadata(
            &workspace,
            "rust-formatting",
            "Format Rust source code.",
            category,
        );
        write_project_skill_with_metadata(
            &workspace,
            "slide-deck-authoring",
            "Create presentations and speaker notes.",
            category,
        );
        let mut config = SkillsConfig {
            project_dir: ".agents/skills".to_string(),
            user_dir: workspace
                .join("missing-user")
                .to_string_lossy()
                .into_owned(),
            ..SkillsConfig::default()
        };
        config.system.enabled = false;
        let tool = SkillsListTool::new(config, tool_workspace(&workspace));

        let full = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"category": category}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let full =
            serde_json::from_str::<SkillsListOutputSnapshot>(&full.canonical_output()).unwrap();
        assert_eq!(full.count, 3);
        assert!(!full.truncated);
        assert_eq!(
            full.skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            [
                "release-build-triage",
                "rust-formatting",
                "slide-deck-authoring"
            ]
        );

        let ranked = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({
                        "category": category,
                        "query": "diagnose a Rust release linker failure",
                        "limit": 1,
                    }),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let ranked =
            serde_json::from_str::<SkillsListOutputSnapshot>(&ranked.canonical_output()).unwrap();
        assert_eq!(ranked.count, 1);
        assert!(ranked.truncated);
        assert_eq!(ranked.skills[0].name, "release-build-triage");
        assert!(ranked.skills[0].description.contains("Cargo profile"));

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"query": "Rust", "limit": 51}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("between 1 and 50"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn create_writes_project_skill() {
        let workspace = temp_dir("create");
        let catalog = SkillCatalog {
            project_dir: workspace.join(".agents/skills"),
            skills: Vec::new(),
            warnings: Vec::new(),
            complete: true,
        };
        let input = CreateSkillInput {
            target: SkillTargetInput {
                name: "local-flow".to_string(),
            },
            content: skill_content("local-flow", "Local flow"),
            category: None,
        };

        create_skill("skill_manage", &catalog, &tool_workspace(&workspace), input).unwrap();

        assert!(
            workspace
                .join(".agents/skills/local-flow/SKILL.md")
                .exists()
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn skill_inputs_and_outputs_flatten_shared_fields() {
        let input = serde_json::from_value::<SkillManageInput>(serde_json::json!({
            "action": "create",
            "name": "local-flow",
            "content": "body",
            "category": null,
        }))
        .unwrap();
        let SkillManageInput::Create(input) = input else {
            panic!("expected create action");
        };
        assert_eq!(input.target.name, "local-flow");

        let output = serde_json::to_value(SkillPathOutput {
            action: SkillActionOutput {
                success: true,
                action: "create",
                name: "local-flow",
            },
            path: Path::new(".agents/skills/local-flow"),
        })
        .unwrap();
        assert_eq!(
            output,
            serde_json::json!({
                "success": true,
                "action": "create",
                "name": "local-flow",
                "path": ".agents/skills/local-flow",
            })
        );
    }

    #[test]
    fn skill_manage_schema_is_a_provider_object_union() {
        let tool = SkillManageTool::new(SkillsConfig::default(), tool_workspace(Path::new(".")));
        let schema = tool.input_schema();

        assert_eq!(schema["type"], "object");
        assert!(schema["oneOf"].is_array());
        assert!(schema.get("additionalProperties").is_none());
        let input = deserialize_tool_input::<SkillManageInput>(
            "skill_manage",
            json!({
                "action": "create",
                "name": "local-flow",
                "content": "body",
            }),
        )
        .expect("valid skill_manage action");
        assert!(matches!(input, SkillManageInput::Create(_)));

        let error = deserialize_tool_input::<SkillManageInput>(
            "skill_manage",
            json!({
                "action": "create",
                "name": "local-flow",
                "content": "body",
                "unexpected": true,
            }),
        )
        .expect_err("unknown skill_manage field");
        assert!(error.to_string().contains("unknown field `unexpected`"));
    }

    #[test]
    fn skill_view_is_never_cached() {
        let tool = SkillViewTool::new(SkillsConfig::default(), tool_workspace(Path::new(".")));
        assert_eq!(
            tool.policy().cache_policy(&json!({"name": "demo"})),
            ToolCachePolicy::Never
        );
    }

    #[test]
    fn patch_accepts_json_escaped_markdown_old_string() {
        let workspace = temp_dir("patch-escaped-old-string");
        let skill_dir = workspace.join(".agents/skills/local-flow");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: local-flow\ndescription: Local flow\n---\n# local-flow\n\nSnippet: `\"unknown\\nusage\"`\n",
    )
    .unwrap();
        let catalog = SkillCatalog {
            project_dir: workspace.join(".agents/skills"),
            skills: vec![SkillMetadata {
                name: "local-flow".to_string(),
                description: "Local flow".to_string(),
                category: None,
                platforms: Vec::new(),
                source: SkillSourceKind::Project,
                path: skill_dir.clone(),
                provider_id: crate::skill::SkillProviderId::new("local-filesystem").unwrap(),
                invocation: crate::skill::SkillInvocationPolicy::default(),
                resource_base: crate::skill::SkillResourceBase::Directory {
                    path: skill_dir.clone(),
                },
            }],
            warnings: Vec::new(),
            complete: true,
        };
        let input = PatchSkillInput {
            target: SkillTargetInput {
                name: "local-flow".to_string(),
            },
            old_string: r#"Snippet: `\"unknown\\nusage\"`"#.to_string(),
            new_string: "Snippet: `\"known\\nusage\"`".to_string(),
            replace_mode: None,
        };

        patch_skill("skill_manage", &catalog, &tool_workspace(&workspace), input).unwrap();

        let updated = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(updated.contains("Snippet: `\"known\\nusage\"`"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn rejects_readonly_skill_patch() {
        let catalog = SkillCatalog {
            project_dir: PathBuf::from("project/skills"),
            skills: vec![SkillMetadata {
                name: "shared".to_string(),
                description: "shared".to_string(),
                category: None,
                platforms: Vec::new(),
                source: SkillSourceKind::System,
                path: PathBuf::from("user/shared"),
                provider_id: crate::skill::SkillProviderId::new("local-filesystem").unwrap(),
                invocation: crate::skill::SkillInvocationPolicy::default(),
                resource_base: crate::skill::SkillResourceBase::Directory {
                    path: PathBuf::from("user/shared"),
                },
            }],
            warnings: Vec::new(),
            complete: true,
        };

        let error = writable_project_skill("skill_manage", &catalog, "shared")
            .unwrap_err()
            .to_string();

        assert!(error.contains("read-only system"));
    }

    #[tokio::test]
    async fn skill_view_success_emits_skill_activation() {
        let workspace = temp_dir("view-activation");
        write_project_skill(&workspace, "local-flow");
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "local-flow"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let activation = activation_from_output(&output);

        assert_eq!(activation.name, "local-flow");
        assert_eq!(activation.source, "project");
        assert_eq!(activation.turn_id, "turn-1");
        assert_eq!(activation.item_identity(), "call-1");
        assert!(
            activation
                .directory_path()
                .is_some_and(|path| path.ends_with("local-flow"))
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn skill_view_records_project_usage_independent_of_product_mode() {
        let workspace = temp_dir("view-task-readonly");
        write_project_skill(&workspace, "local-flow");
        let usage_path = workspace.join(".agents/skills/local-flow/.usage.json");
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "local-flow"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();

        assert_eq!(activation_from_output(&output).name, "local-flow");
        assert!(usage_path.exists());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn skill_view_support_file_success_activates_parent_skill() {
        let workspace = temp_dir("view-support-activation");
        write_project_skill(&workspace, "local-flow");
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "local-flow", "filePath": "references/example.md"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let activation = activation_from_output(&output);

        assert_eq!(activation.name, "local-flow");
        assert_eq!(activation.source, "project");
        fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn skill_view_main_file_alias_returns_resource_base_without_enumerating_files() {
        let workspace = temp_dir("view-main-alias");
        write_project_skill(&workspace, "local-flow");
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let output = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "local-flow", "filePath": "SKILL.md"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let result =
            serde_json::from_str::<SkillViewOutputSnapshot>(&output.canonical_output()).unwrap();

        assert_eq!(activation_from_output(&output).name, "local-flow");
        assert!(result.success);
        assert_eq!(result.skill.name, "local-flow");
        assert_eq!(result.file_path, "SKILL.md");
        assert!(matches!(
            result.resource_base,
            crate::skill::SkillResourceBase::Directory { .. }
        ));
        assert!(result.resource_hint.contains("filePath"));
        assert!(result.content.contains("# local-flow"));
        fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn skill_view_failure_does_not_emit_activation() {
        let workspace = temp_dir("view-failure");
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "missing"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("skill not found"));
        let _ = fs::remove_dir_all(workspace);
    }

    #[tokio::test]
    async fn skill_discovery_skips_linked_skill_directories() {
        let workspace = temp_dir("linked-discovery");
        let outside = temp_dir("linked-discovery-target");
        fs::create_dir_all(workspace.join(".agents/skills")).unwrap();
        write_project_skill(&outside, "linked-flow");
        create_directory_link(
            &outside.join(".agents/skills/linked-flow"),
            &workspace.join(".agents/skills/linked-flow"),
        );
        let tool = SkillViewTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({"name": "linked-flow"}),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("skill not found"), "{error}");
        remove_directory_link(&workspace.join(".agents/skills/linked-flow"));
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[tokio::test]
    async fn skill_manage_rejects_linked_support_directory() {
        let workspace = temp_dir("linked-support-write");
        let outside = temp_dir("linked-support-write-target");
        write_project_skill(&workspace, "local-flow");
        fs::remove_dir_all(workspace.join(".agents/skills/local-flow/references")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        create_directory_link(
            &outside,
            &workspace.join(".agents/skills/local-flow/references"),
        );
        let tool = SkillManageTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        let error = tool
            .execute_raw(
                ToolInput {
                    arguments: json!({
                        "action": "writeFile",
                        "name": "local-flow",
                        "filePath": "references/new.md",
                        "fileContent": "blocked"
                    }),
                },
                tool_context(workspace.clone()),
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("reparse point"), "{error}");
        assert!(!outside.join("new.md").exists());
        remove_directory_link(&workspace.join(".agents/skills/local-flow/references"));
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[tokio::test]
    async fn skill_delete_unlinks_support_directory_without_touching_target() {
        let workspace = temp_dir("linked-support-delete");
        let outside = temp_dir("linked-support-delete-target");
        write_project_skill(&workspace, "local-flow");
        fs::remove_dir_all(workspace.join(".agents/skills/local-flow/references")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("kept.md"), "kept").unwrap();
        create_directory_link(
            &outside,
            &workspace.join(".agents/skills/local-flow/references"),
        );
        let tool = SkillManageTool::new(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        );

        tool.execute_raw(
            ToolInput {
                arguments: json!({
                    "action": "delete",
                    "name": "local-flow"
                }),
            },
            tool_context(workspace.clone()),
        )
        .await
        .unwrap();

        assert!(!workspace.join(".agents/skills/local-flow").exists());
        assert_eq!(fs::read_to_string(outside.join("kept.md")).unwrap(), "kept");
        fs::remove_dir_all(workspace).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
