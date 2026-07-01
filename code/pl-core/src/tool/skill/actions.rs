use std::fs;
use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use crate::skill::{
    SkillCatalog, SkillMetadata, SkillSourceKind, bump_project_patch, mark_project_skill_created,
    project_skill_dir_for_create, support_file_path, validate_skill_document,
};

use super::{
    ReplaceMode, SkillDeleteOutput, SkillFileOutput, SkillManageInput, SkillPatchOutput,
    SkillPathOutput, json_output, required, tool_error,
};
use crate::tool::ToolOutput;

pub(super) fn create_skill(
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
    json_output(SkillPathOutput {
        success: true,
        action: "create",
        name: &metadata.name,
        path: &skill_dir,
    })
}

pub(super) fn edit_skill(
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
    json_output(SkillPathOutput {
        success: true,
        action: "edit",
        name: &metadata.name,
        path: &skill.path,
    })
}

pub(super) fn patch_skill(
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
    json_output(SkillPatchOutput {
        success: true,
        action: "patch",
        name: &skill.name,
        replacements: match replace_mode {
            ReplaceMode::One => 1,
            ReplaceMode::All => matches,
        },
        path: &skill.path,
    })
}

pub(super) fn delete_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: SkillManageInput,
) -> Result<ToolOutput, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.name)?;
    ensure_project_child(&catalog.project_dir, &skill.path, tool)?;
    fs::remove_dir_all(&skill.path)
        .map_err(|error| tool_error(tool, format!("failed to delete skill: {error}")))?;
    json_output(SkillDeleteOutput {
        success: true,
        action: "delete",
        name: &skill.name,
        absorbed_into: input.absorbed_into,
    })
}

pub(super) fn write_support_file(
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
    json_output(SkillFileOutput {
        success: true,
        action: "writeFile",
        name: &skill.name,
        file_path: &file_path,
    })
}

pub(super) fn remove_support_file(
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
    json_output(SkillFileOutput {
        success: true,
        action: "removeFile",
        name: &skill.name,
        file_path: &file_path,
    })
}

pub(super) fn writable_project_skill<'a>(
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
