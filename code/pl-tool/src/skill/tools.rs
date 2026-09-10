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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::deserialize_tool_input;
    use crate::test_support::{ToolTestExt, input};
    use std::path::PathBuf;

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

    fn tool_context(_workspace_root: PathBuf) -> pl_core::tool::opaque::CallContext {
        crate::test_support::thread_context()
    }
    async fn configured_skill(
        config: SkillsConfig,
        workspace: ToolWorkspace,
        kind: ThreadSkillKind,
    ) -> ThreadSkillTool {
        let catalog =
            discover_local_skills(workspace.root(), &config, None, CancellationToken::new())
                .await
                .unwrap();
        ThreadSkillTool::new(Arc::new(catalog), kind)
    }
    async fn configured_manage(
        config: SkillsConfig,
        workspace: ToolWorkspace,
    ) -> ThreadSkillManageTool {
        let catalog =
            discover_local_skills(workspace.root(), &config, None, CancellationToken::new())
                .await
                .unwrap();
        ThreadSkillManageTool::new(Arc::new(catalog), workspace)
    }
    fn tool_workspace(workspace_root: &Path) -> ToolWorkspace {
        ToolWorkspace::new(crate::workspace::AgentWorkspace::local(
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
        let tool = ThreadSkillTool::new(Arc::new(catalog), ThreadSkillKind::View);

        let output = tool
            .execute_raw(
                input(json!({"name": "review-single-pr"})),
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

    fn activation_from_output(output: &pl_core::tool::ToolOutput) -> crate::skill::SkillSummary {
        let pl_core::thread::extensions::ExtensionMutation::Put { payload, .. } =
            &output.extension_mutations()[0]
        else {
            panic!("saved skill view")
        };
        serde_json::from_str::<SkillViewOutputSnapshot>(payload.content())
            .unwrap()
            .skill
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
        let tool =
            configured_skill(config, tool_workspace(&workspace), ThreadSkillKind::List).await;

        let full = tool
            .execute_raw(
                input(json!({"category": category})),
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let full =
            serde_json::from_str::<SkillsListOutputSnapshot>(full.payload().content()).unwrap();
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
                input(json!({
                    "category": category,
                    "query": "diagnose a Rust release linker failure",
                    "limit": 1,
                })),
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let ranked =
            serde_json::from_str::<SkillsListOutputSnapshot>(ranked.payload().content()).unwrap();
        assert_eq!(ranked.count, 1);
        assert!(ranked.truncated);
        assert_eq!(ranked.skills[0].name, "release-build-triage");
        assert!(ranked.skills[0].description.contains("Cargo profile"));

        let error = tool
            .execute_raw(
                input(json!({"query": "Rust", "limit": 51})),
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
        let schema = crate::typed_tool_input_schema::<SkillManageInput>();

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
    async fn skill_view_records_project_usage_independent_of_product_mode() {
        let workspace = temp_dir("view-task-readonly");
        write_project_skill(&workspace, "local-flow");
        let usage_path = workspace.join(".agents/skills/local-flow/.usage.json");
        let tool = configured_skill(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
            ThreadSkillKind::View,
        )
        .await;

        let output = tool
            .execute_raw(
                input(json!({"name": "local-flow"})),
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();

        assert_eq!(activation_from_output(&output).name, "local-flow");
        assert!(usage_path.exists());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[tokio::test]
    async fn skill_view_main_file_alias_returns_resource_base_without_enumerating_files() {
        let workspace = temp_dir("view-main-alias");
        write_project_skill(&workspace, "local-flow");
        let tool = configured_skill(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
            ThreadSkillKind::View,
        )
        .await;

        let output = tool
            .execute_raw(
                input(json!({"name": "local-flow", "filePath": "SKILL.md"})),
                tool_context(workspace.clone()),
            )
            .await
            .unwrap();
        let result =
            serde_json::from_str::<SkillViewOutputSnapshot>(output.payload().content()).unwrap();

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
        let tool = configured_skill(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
            ThreadSkillKind::View,
        )
        .await;

        let error = tool
            .execute_raw(
                input(json!({"name": "missing"})),
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
        let tool = configured_skill(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
            ThreadSkillKind::View,
        )
        .await;

        let error = tool
            .execute_raw(
                input(json!({"name": "linked-flow"})),
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
        let tool = configured_manage(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        )
        .await;

        let error = tool
            .execute_raw(
                input(json!({
                    "action": "writeFile",
                    "name": "local-flow",
                    "filePath": "references/new.md",
                    "fileContent": "blocked"
                })),
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
        let tool = configured_manage(
            SkillsConfig {
                project_dir: ".agents/skills".to_string(),
                ..SkillsConfig::default()
            },
            tool_workspace(&workspace),
        )
        .await;

        tool.execute_raw(
            input(json!({
                "action": "delete",
                "name": "local-flow"
            })),
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
