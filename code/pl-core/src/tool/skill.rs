use std::fs;
use std::path::{Path, PathBuf};

use pl_protocol::PureError;
use serde::Deserialize;
use serde_json::json;

use crate::config::SkillsConfig;
use crate::skill::{
    SkillCatalog, SkillMetadata, SkillSourceKind, bump_project_patch, bump_project_view,
    list_support_files, mark_project_skill_created, project_skill_dir_for_create, read_skill_file,
    support_file_path, validate_skill_document,
};

use super::truncation::{OutputTruncation, TruncatedOutput};
use super::{Tool, ToolContext, ToolInput, ToolOutput};

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsListInput {
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillViewInput {
    name: String,
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillManageInput {
    action: SkillManageAction,
    name: String,
    content: Option<String>,
    category: Option<String>,
    file_path: Option<String>,
    file_content: Option<String>,
    old_string: Option<String>,
    new_string: Option<String>,
    replace_mode: Option<ReplaceMode>,
    absorbed_into: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SkillManageAction {
    Create,
    Patch,
    Edit,
    Delete,
    WriteFile,
    RemoveFile,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReplaceMode {
    One,
    All,
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
        json!({
            "type": "object",
            "properties": {
                "category": { "type": "string" }
            },
            "additionalProperties": false
        })
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
            let input: SkillsListInput = parse_input(input.arguments, self.name())?;
            let catalog = SkillCatalog::discover(&context.workspace_root, &self.config)
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
            json_output(json!({
                "success": true,
                "count": skills.len(),
                "projectDir": catalog.project_dir,
                "skills": skills,
                "warnings": catalog.warnings,
            }))
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
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "filePath": {
                    "type": "string",
                    "description": "Optional support file path under references/, templates/, scripts/, or assets/."
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
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
            let input: SkillViewInput = parse_input(input.arguments, self.name())?;
            let catalog = SkillCatalog::discover(&context.workspace_root, &self.config)
                .map_err(|error| tool_error(self.name(), error))?;
            let skill = catalog.find(&input.name).ok_or_else(|| {
                tool_error(self.name(), format!("skill not found: {}", input.name))
            })?;
            let content = read_skill_file(skill, input.file_path.as_deref())
                .map_err(|error| tool_error(self.name(), error))?;
            bump_project_view(skill).map_err(|error| tool_error(self.name(), error))?;
            let support_files = if input.file_path.is_none() {
                list_support_files(&skill.path).map_err(|error| tool_error(self.name(), error))?
            } else {
                Vec::new()
            };
            json_output(json!({
                "success": true,
                "skill": skill,
                "filePath": input.file_path.unwrap_or_else(|| "SKILL.md".to_string()),
                "supportFiles": support_files,
                "content": content,
            }))
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
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "patch", "edit", "delete", "writeFile", "removeFile"]
                },
                "name": { "type": "string" },
                "content": {
                    "type": "string",
                    "description": "Full SKILL.md content for create/edit."
                },
                "category": {
                    "type": "string",
                    "description": "Optional category path for create."
                },
                "filePath": {
                    "type": "string",
                    "description": "Support file path under references/, templates/, scripts/, or assets/."
                },
                "fileContent": { "type": "string" },
                "oldString": { "type": "string" },
                "newString": { "type": "string" },
                "replaceMode": { "type": "string", "enum": ["one", "all"] },
                "absorbedInto": {
                    "type": "string",
                    "description": "Optional note when deleting a project skill after absorbing it elsewhere."
                }
            },
            "required": ["action", "name"],
            "additionalProperties": false
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> super::BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let input: SkillManageInput = parse_input(input.arguments, self.name())?;
            let catalog = SkillCatalog::discover(&context.workspace_root, &self.config)
                .map_err(|error| tool_error(self.name(), error))?;
            match input.action {
                SkillManageAction::Create => create_skill(self.name(), &catalog, input),
                SkillManageAction::Patch => patch_skill(self.name(), &catalog, input),
                SkillManageAction::Edit => edit_skill(self.name(), &catalog, input),
                SkillManageAction::Delete => delete_skill(self.name(), &catalog, input),
                SkillManageAction::WriteFile => write_support_file(self.name(), &catalog, input),
                SkillManageAction::RemoveFile => remove_support_file(self.name(), &catalog, input),
            }
        })
    }
}

fn create_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    if catalog.project_skill(&input.name).is_some() {
        return Err(tool_error(
            tool,
            format!("project skill already exists: {}", input.name),
        ));
    }
    let content = required(input.content, tool, "content")?;
    let metadata = validate_skill_document(&content, Some(&input.name))
        .map_err(|error| tool_error(tool, error))?;
    let category = input.category.as_deref().or(metadata.category.as_deref());
    let skill_dir = project_skill_dir_for_create(&catalog.project_dir, &metadata.name, category)
        .map_err(|error| tool_error(tool, error))?;
    ensure_project_child(&catalog.project_dir, &skill_dir, tool)?;
    if skill_dir.join("SKILL.md").exists() {
        return Err(tool_error(
            tool,
            format!(
                "project skill directory already contains SKILL.md: {}",
                skill_dir.display()
            ),
        ));
    }
    fs::create_dir_all(&skill_dir)
        .map_err(|error| tool_error(tool, format!("failed to create skill directory: {error}")))?;
    fs::write(skill_dir.join("SKILL.md"), content)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    mark_project_skill_created(&skill_dir).map_err(|error| tool_error(tool, error))?;
    json_output(json!({
        "success": true,
        "action": "create",
        "name": metadata.name,
        "path": skill_dir,
    }))
}

fn edit_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    let content = required(input.content, tool, "content")?;
    let metadata = validate_skill_document(&content, Some(&input.name))
        .map_err(|error| tool_error(tool, error))?;
    ensure_project_child(&catalog.project_dir, &skill.path, tool)?;
    fs::write(skill.path.join("SKILL.md"), content)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    bump_project_patch(&skill.path).map_err(|error| tool_error(tool, error))?;
    json_output(json!({
        "success": true,
        "action": "edit",
        "name": metadata.name,
        "path": skill.path,
    }))
}

fn patch_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    let old_string = required(input.old_string, tool, "oldString")?;
    if old_string.is_empty() {
        return Err(tool_error(tool, "oldString must not be empty"));
    }
    let new_string = required(input.new_string, tool, "newString")?;
    let path = skill.path.join("SKILL.md");
    let content = fs::read_to_string(&path)
        .map_err(|error| tool_error(tool, format!("failed to read SKILL.md: {error}")))?;
    let matches = content.matches(&old_string).count();
    if matches == 0 {
        return Err(tool_error(tool, "oldString was not found in SKILL.md"));
    }
    let replace_mode = input.replace_mode.unwrap_or(ReplaceMode::One);
    if matches > 1 && matches!(replace_mode, ReplaceMode::One) {
        return Err(tool_error(
            tool,
            format!(
                "oldString matched {matches} times; use replaceMode=all or provide a unique oldString"
            ),
        ));
    }
    let updated = match replace_mode {
        ReplaceMode::One => content.replacen(&old_string, &new_string, 1),
        ReplaceMode::All => content.replace(&old_string, &new_string),
    };
    validate_skill_document(&updated, Some(&input.name))
        .map_err(|error| tool_error(tool, error))?;
    fs::write(&path, updated)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    bump_project_patch(&skill.path).map_err(|error| tool_error(tool, error))?;
    json_output(json!({
        "success": true,
        "action": "patch",
        "name": skill.name,
        "replacements": match replace_mode {
            ReplaceMode::One => 1,
            ReplaceMode::All => matches,
        },
        "path": skill.path,
    }))
}

fn delete_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    ensure_project_child(&catalog.project_dir, &skill.path, tool)?;
    fs::remove_dir_all(&skill.path)
        .map_err(|error| tool_error(tool, format!("failed to delete skill: {error}")))?;
    json_output(json!({
        "success": true,
        "action": "delete",
        "name": skill.name,
        "absorbedInto": input.absorbed_into,
    }))
}

fn write_support_file(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    let file_path = required(input.file_path, tool, "filePath")?;
    let file_content = required(input.file_content, tool, "fileContent")?;
    let relative = support_file_path(&file_path).map_err(|error| tool_error(tool, error))?;
    let path = skill.path.join(relative);
    ensure_project_child(&catalog.project_dir, &path, tool)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            tool_error(
                tool,
                format!("failed to create support file directory: {error}"),
            )
        })?;
    }
    fs::write(&path, file_content)
        .map_err(|error| tool_error(tool, format!("failed to write support file: {error}")))?;
    bump_project_patch(&skill.path).map_err(|error| tool_error(tool, error))?;
    json_output(json!({
        "success": true,
        "action": "writeFile",
        "name": skill.name,
        "filePath": file_path,
    }))
}

fn remove_support_file(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    let file_path = required(input.file_path, tool, "filePath")?;
    let relative = support_file_path(&file_path).map_err(|error| tool_error(tool, error))?;
    let path = skill.path.join(relative);
    ensure_project_child(&catalog.project_dir, &path, tool)?;
    if !path.is_file() {
        return Err(tool_error(
            tool,
            format!("support file does not exist: {file_path}"),
        ));
    }
    fs::remove_file(&path)
        .map_err(|error| tool_error(tool, format!("failed to remove support file: {error}")))?;
    bump_project_patch(&skill.path).map_err(|error| tool_error(tool, error))?;
    json_output(json!({
        "success": true,
        "action": "removeFile",
        "name": skill.name,
        "filePath": file_path,
    }))
}

fn writable_project_skill<'a>(
    tool: &str,
    catalog: &'a SkillCatalog,
    name: &str,
) -> Result<&'a SkillMetadata, PureError> {
    if let Some(skill) = catalog.project_skill(name) {
        return Ok(skill);
    }
    if let Some(skill) = catalog.find(name) {
        let source = match skill.source {
            SkillSourceKind::Project => "project",
            SkillSourceKind::User => "user",
            SkillSourceKind::System => "system",
            SkillSourceKind::External => "external",
        };
        return Err(tool_error(
            tool,
            format!(
                "skill '{name}' comes from read-only {source} skills; create a project skill override before modifying it"
            ),
        ));
    }
    Err(tool_error(tool, format!("project skill not found: {name}")))
}

fn ensure_project_child(project_dir: &Path, path: &Path, tool: &str) -> Result<(), PureError> {
    let absolute_project = absolute_path(project_dir);
    let absolute_path = absolute_path(path);
    if absolute_path.starts_with(&absolute_project) {
        Ok(())
    } else {
        Err(tool_error(
            tool,
            format!("path escapes project skills directory: {}", path.display()),
        ))
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

fn parse_input<T: serde::de::DeserializeOwned>(
    arguments: serde_json::Value,
    tool: &str,
) -> Result<T, PureError> {
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn required(value: Option<String>, tool: &str, field: &str) -> Result<String, PureError> {
    value.ok_or_else(|| tool_error(tool, format!("{field} is required")))
}

fn json_output(value: serde_json::Value) -> Result<ToolOutput, PureError> {
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
            stderr: TruncatedOutput {
                content: String::new(),
                was_truncated: false,
                original_length: 0,
            },
        },
        output_file: PathBuf::new(),
        exit_code: Some(0),
        timed_out: false,
    })
}

fn tool_error(tool: &str, error: impl std::fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

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

    #[test]
    fn create_writes_project_skill() {
        let workspace = temp_dir("create");
        let catalog = SkillCatalog {
            project_dir: workspace.join("skills"),
            skills: Vec::new(),
            warnings: Vec::new(),
        };
        let input = SkillManageInput {
            action: SkillManageAction::Create,
            name: "local-flow".to_string(),
            content: Some(skill_content("local-flow", "Local flow")),
            category: None,
            file_path: None,
            file_content: None,
            old_string: None,
            new_string: None,
            replace_mode: None,
            absorbed_into: None,
        };

        create_skill("skill_manage", &catalog, input).unwrap();

        assert!(workspace.join("skills/local-flow/SKILL.md").exists());
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
            }],
            warnings: Vec::new(),
        };

        let error = writable_project_skill("skill_manage", &catalog, "shared")
            .unwrap_err()
            .to_string();

        assert!(error.contains("read-only system"));
    }
}
