use std::fs;
use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use crate::path_safety::*;
use crate::skill::*;

use super::{
    CreateSkillInput, DeleteSkillInput, EditSkillInput, PatchSkillInput, RemoveSkillFileInput,
    ReplaceMode, SkillActionOutput, SkillDeleteOutput, SkillFileOutput, SkillPatchOutput,
    SkillPathOutput, WriteSkillFileInput, json_output, tool_error,
};
use crate::tool::ToolResult;
use crate::tool::text_escape::decode_json_escaped_fragment_once;

pub(super) fn create_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: CreateSkillInput,
) -> Result<ToolResult, PureError> {
    if catalog.project_skill(&input.target.name).is_some() {
        let name = &input.target.name;
        return Err(tool_error(
            tool,
            format!("project skill already exists: {name}"),
        ));
    }
    let content = input.content;
    let metadata = validate_skill_document(&content, Some(&input.target.name))
        .map_err(|error| tool_error(tool, error))?;
    let category = input.category.as_deref().or(metadata.category.as_deref());
    let skill_dir = project_skill_dir_for_create(&catalog.project_dir, &metadata.name, category)
        .map_err(|error| tool_error(tool, error))?;
    ensure_project_path(
        &catalog.project_dir,
        &skill_dir,
        ProjectPathRequirement::AllowMissing,
        tool,
    )?;
    let skill_file = skill_dir.join("SKILL.md");
    ensure_project_path(
        &catalog.project_dir,
        &skill_file,
        ProjectPathRequirement::AllowMissing,
        tool,
    )?;
    if skill_file.exists() {
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
    fs::write(skill_file, content)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    mark_project_skill_created(&catalog.project_dir, &skill_dir)
        .map_err(|error| tool_error(tool, error))?;
    json_output(SkillPathOutput {
        action: SkillActionOutput {
            success: true,
            action: "create",
            name: &metadata.name,
        },
        path: &skill_dir,
    })
}

pub(super) fn edit_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: EditSkillInput,
) -> Result<ToolResult, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.target.name)?;
    let content = input.content;
    let metadata = validate_skill_document(&content, Some(&input.target.name))
        .map_err(|error| tool_error(tool, error))?;
    let skill_file = skill.path.join("SKILL.md");
    ensure_project_path(
        &catalog.project_dir,
        &skill_file,
        ProjectPathRequirement::MustExist,
        tool,
    )?;
    fs::write(skill_file, content)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    bump_project_patch(&catalog.project_dir, &skill.path)
        .map_err(|error| tool_error(tool, error))?;
    json_output(SkillPathOutput {
        action: SkillActionOutput {
            success: true,
            action: "edit",
            name: &metadata.name,
        },
        path: &skill.path,
    })
}

pub(super) fn patch_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: PatchSkillInput,
) -> Result<ToolResult, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.target.name)?;
    ensure_project_path(
        &catalog.project_dir,
        &skill.path,
        ProjectPathRequirement::MustExist,
        tool,
    )?;
    let old_string = input.old_string;
    if old_string.is_empty() {
        return Err(tool_error(tool, "oldString must not be empty"));
    }
    let new_string = input.new_string;
    let path = skill.path.join("SKILL.md");
    ensure_project_path(
        &catalog.project_dir,
        &path,
        ProjectPathRequirement::MustExist,
        tool,
    )?;
    let content = fs::read_to_string(&path)
        .map_err(|error| tool_error(tool, format!("failed to read SKILL.md: {error}")))?;
    let (needle, matches) = patch_needle(&content, &old_string).ok_or_else(|| {
        tool_error(
            tool,
            "oldString was not found in SKILL.md; read the current skill text and pass an exact oldString",
        )
    })?;
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
        ReplaceMode::One => content.replacen(&needle, &new_string, 1),
        ReplaceMode::All => content.replace(&needle, &new_string),
    };
    validate_skill_document(&updated, Some(&input.target.name))
        .map_err(|error| tool_error(tool, error))?;
    fs::write(&path, updated)
        .map_err(|error| tool_error(tool, format!("failed to write SKILL.md: {error}")))?;
    bump_project_patch(&catalog.project_dir, &skill.path)
        .map_err(|error| tool_error(tool, error))?;
    json_output(SkillPatchOutput {
        action: SkillActionOutput {
            success: true,
            action: "patch",
            name: &skill.name,
        },
        replacements: match replace_mode {
            ReplaceMode::One => 1,
            ReplaceMode::All => matches,
        },
        path: &skill.path,
    })
}

fn patch_needle(content: &str, old_string: &str) -> Option<(String, usize)> {
    let matches = content.matches(old_string).count();
    if matches > 0 {
        return Some((old_string.to_string(), matches));
    }

    let normalized = decode_json_escaped_fragment_once(old_string)?;
    let matches = content.matches(&normalized).count();
    (matches > 0).then_some((normalized, matches))
}

pub(super) fn delete_skill(
    tool: &str,
    catalog: &SkillCatalog,
    input: DeleteSkillInput,
) -> Result<ToolResult, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.target.name)?;
    ensure_project_path(
        &catalog.project_dir,
        &skill.path,
        ProjectPathRequirement::MustExist,
        tool,
    )?;
    remove_dir_all_no_follow(&catalog.project_dir, &skill.path)
        .map_err(|error| tool_error(tool, format!("failed to delete skill: {error}")))?;
    json_output(SkillDeleteOutput {
        action: SkillActionOutput {
            success: true,
            action: "delete",
            name: &skill.name,
        },
        absorbed_into: input.absorbed_into,
    })
}

pub(super) fn write_support_file(
    tool: &str,
    catalog: &SkillCatalog,
    input: WriteSkillFileInput,
) -> Result<ToolResult, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.target.name)?;
    let file_path = input.file_path;
    let file_content = input.file_content;
    let relative = support_file_path(&file_path).map_err(|error| tool_error(tool, error))?;
    let path = skill.path.join(relative);
    ensure_project_path(
        &catalog.project_dir,
        &path,
        ProjectPathRequirement::AllowMissing,
        tool,
    )?;
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
    bump_project_patch(&catalog.project_dir, &skill.path)
        .map_err(|error| tool_error(tool, error))?;
    json_output(SkillFileOutput {
        action: SkillActionOutput {
            success: true,
            action: "writeFile",
            name: &skill.name,
        },
        file_path: &file_path,
    })
}

pub(super) fn remove_support_file(
    tool: &str,
    catalog: &SkillCatalog,
    input: RemoveSkillFileInput,
) -> Result<ToolResult, PureError> {
    let skill = writable_project_skill(tool, catalog, &input.target.name)?;
    let file_path = input.file_path;
    let relative = support_file_path(&file_path).map_err(|error| tool_error(tool, error))?;
    let path = skill.path.join(relative);
    ensure_project_path(
        &catalog.project_dir,
        &path,
        ProjectPathRequirement::MustExist,
        tool,
    )?;
    if !path.is_file() {
        return Err(tool_error(
            tool,
            format!("support file does not exist: {file_path}"),
        ));
    }
    fs::remove_file(&path)
        .map_err(|error| tool_error(tool, format!("failed to remove support file: {error}")))?;
    bump_project_patch(&catalog.project_dir, &skill.path)
        .map_err(|error| tool_error(tool, error))?;
    json_output(SkillFileOutput {
        action: SkillActionOutput {
            success: true,
            action: "removeFile",
            name: &skill.name,
        },
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

#[derive(Debug, Clone, Copy)]
enum ProjectPathRequirement {
    MustExist,
    AllowMissing,
}

fn ensure_project_path(
    project_dir: &Path,
    path: &Path,
    requirement: ProjectPathRequirement,
    tool: &str,
) -> Result<(), PureError> {
    let absolute_project = absolute_path(project_dir);
    let absolute_path = absolute_path(path);
    if !is_lexically_within(&absolute_project, &absolute_path) {
        return Err(tool_error(
            tool,
            format!("path escapes project skills directory: {}", path.display()),
        ));
    }
    match std::fs::symlink_metadata(&absolute_project) {
        Ok(metadata) if crate::path_safety::is_link_or_reparse(&metadata) => {
            return Err(tool_error(
                tool,
                format!(
                    "project skills directory is a symbolic link or Windows reparse point: {}",
                    project_dir.display()
                ),
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(tool_error(
                tool,
                format!(
                    "project skills path is not a directory: {}",
                    project_dir.display()
                ),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(tool_error(tool, error)),
    }
    let result = match requirement {
        ProjectPathRequirement::MustExist => {
            validate_existing_path(&absolute_project, &absolute_path)
        }
        ProjectPathRequirement::AllowMissing => {
            validate_path_for_write(&absolute_project, &absolute_path)
        }
    };
    result.map_err(|error| tool_error(tool, error))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}
