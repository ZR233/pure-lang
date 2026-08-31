use pl_model::ModelInfo;
use pl_protocol::Result;

use crate::config::InstructionsConfig;
use crate::execution_environment::{ExecutionEnvironment, ExecutionTransport, ShellDialect};
use crate::workspace::WorkspaceInstructions;
use crate::workspace::load_workspace_instruction_documents;

use super::{
    InstructionAssemblyRequest, InstructionBlock, InstructionProfile, InstructionSnapshot,
    InstructionSource, InstructionSourceKind,
};

const DEFAULT_BASE_INSTRUCTIONS: &str = include_str!("../prompts/system.md");
const PLATFORM_COMMON_INSTRUCTIONS: &str = include_str!("../prompts/platform/common.md");
const PLATFORM_UNIX_INSTRUCTIONS: &str = include_str!("../prompts/platform/unix.md");
const PLATFORM_WINDOWS_INSTRUCTIONS: &str = include_str!("../prompts/platform/windows.md");

pub struct InstructionAssembler;

impl InstructionAssembler {
    pub fn assemble(request: InstructionAssemblyRequest<'_>) -> Result<InstructionSnapshot> {
        Self::assemble_inner(request, None)
    }

    pub fn assemble_with_profile(
        request: InstructionAssemblyRequest<'_>,
        profile: &InstructionProfile,
    ) -> Result<InstructionSnapshot> {
        Self::assemble_inner(request, Some(profile))
    }

    fn assemble_inner(
        request: InstructionAssemblyRequest<'_>,
        profile: Option<&InstructionProfile>,
    ) -> Result<InstructionSnapshot> {
        let config_instructions = request.instructions;
        let mut snapshot = InstructionSnapshot {
            base: base_block(profile, config_instructions, request.model),
            developer: Vec::new(),
            user: Vec::new(),
        };

        if let Some(execution) = request.execution_profile {
            snapshot.push_developer(
                InstructionSource::new(InstructionSourceKind::ExecutionProfile, execution.label),
                execution.instructions,
            );
        }
        let environment = request
            .execution_environment
            .cloned()
            .unwrap_or_else(ExecutionEnvironment::detect_local);
        snapshot.push_developer(
            InstructionSource::new(
                InstructionSourceKind::Platform,
                platform_label(&environment),
            ),
            &platform_instructions(&environment),
        );
        if let Some(config) = config_instructions {
            snapshot.push_developer(
                InstructionSource::new(InstructionSourceKind::ConfigDeveloper, "config developer"),
                &config.developer,
            );
        }
        if let Some(profile) = profile {
            for block in &profile.developer_blocks {
                snapshot.push_developer(block.source.clone(), &block.content);
            }
        }
        if let Some(skills) = request.skills.filter(|skills| skills.enabled) {
            let prompt = request
                .skill_catalog
                .map(crate::skill::build_skills_prompt_from_catalog)
                .map(Some)
                .map(Ok)
                .unwrap_or_else(|| {
                    crate::skill::build_skills_prompt(request.workspace_root, skills, None)
                });
            match prompt {
                Ok(Some(skills)) => {
                    snapshot.push_developer(
                        InstructionSource::new(InstructionSourceKind::Skills, "skills"),
                        &skills,
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "failed to load skills prompt");
                }
            }
        }
        if let Some(constraint) = request.subagent_constraint {
            snapshot.push_developer(
                InstructionSource::new(
                    InstructionSourceKind::SubagentConstraint,
                    "subagent dispatch constraint",
                ),
                constraint,
            );
        }

        if let Some(config) = config_instructions {
            snapshot.push_user(
                InstructionSource::new(InstructionSourceKind::ConfigUser, "config user"),
                &config.user,
            );
            add_project_documents(
                &mut snapshot,
                request.workspace_root,
                request.current_dir,
                config,
                request.workspace_documents,
            )?;
        } else if let Some(instructions) = request.workspace_instructions {
            snapshot.push_user(
                InstructionSource::new(InstructionSourceKind::WorkspaceFallback, "workspace"),
                instructions,
            );
        }
        if let Some(profile) = profile {
            for block in &profile.user_context_blocks {
                snapshot.push_user(block.source.clone(), &block.content);
            }
        }
        if let (Some(catalog), Some(suggestions)) =
            (request.skill_catalog, request.skill_suggestions)
            && let Some(prompt) = crate::skill::build_skill_suggestions_from_catalog(
                catalog,
                suggestions.query,
                suggestions.excluded_names,
            )
        {
            snapshot.push_user(
                InstructionSource::new(
                    InstructionSourceKind::SkillSuggestions,
                    "skill suggestions",
                ),
                &prompt,
            );
        }

        Ok(snapshot)
    }
}

fn base_block(
    profile: Option<&InstructionProfile>,
    config: Option<&InstructionsConfig>,
    model: &ModelInfo,
) -> InstructionBlock {
    if let Some(base_override) = profile
        .and_then(|profile| profile.base_system_prompt.as_deref())
        .map(str::trim)
        .filter(|content| !content.is_empty())
    {
        return InstructionBlock {
            source: InstructionSource::new(
                InstructionSourceKind::ProfileBaseOverride,
                "profile base override",
            ),
            content: base_override.to_string(),
        };
    }
    if let Some(base_override) = config
        .map(|config| config.base_override.trim())
        .filter(|content| !content.is_empty())
    {
        return InstructionBlock {
            source: InstructionSource::new(
                InstructionSourceKind::ConfigBaseOverride,
                "config base override",
            ),
            content: base_override.to_string(),
        };
    }
    if !model.base_instructions.trim().is_empty() {
        return InstructionBlock {
            source: InstructionSource::new(InstructionSourceKind::ModelBase, "model base"),
            content: model.base_instructions.trim().to_string(),
        };
    }
    InstructionBlock {
        source: InstructionSource::new(InstructionSourceKind::BuiltInBase, "built-in base"),
        content: DEFAULT_BASE_INSTRUCTIONS.trim().to_string(),
    }
}

fn platform_label(environment: &ExecutionEnvironment) -> String {
    format!(
        "platform: {}/{}",
        environment.os.as_str(),
        environment.shell.as_str()
    )
}

fn platform_instructions(environment: &ExecutionEnvironment) -> String {
    let target = match environment.transport {
        ExecutionTransport::Local => "local process",
        ExecutionTransport::Ssh => "SSH remote helper",
    };
    let os_specific = if environment.os.is_windows() {
        PLATFORM_WINDOWS_INSTRUCTIONS
    } else {
        PLATFORM_UNIX_INSTRUCTIONS
    };
    let shell_rules = match environment.shell {
        ShellDialect::Bash => "Use Bash/POSIX command syntax; commands are started as `bash -c`.",
        ShellDialect::Sh => {
            "Use portable POSIX `sh` syntax only; Bash-only extensions are unavailable. Commands are started as `sh -c`."
        }
        ShellDialect::Pwsh => {
            "Use PowerShell 7+ syntax; commands are started as `pwsh -NoProfile -Command`."
        }
        ShellDialect::PowerShell => {
            "Use Windows PowerShell syntax; commands are started as `powershell -NoProfile -Command`."
        }
        ShellDialect::Cmd => "Use Windows cmd.exe syntax; commands are started as `cmd /C`.",
    };
    format!(
        "{}\n\n{}\n\n## Runtime execution environment\n- Execution target: {target}\n- Target OS: {}\n- Shell dialect: {}\n- Shell executable: `{}`\n- {shell_rules}\n- Follow this runtime shell exactly. Do not infer syntax from the controller OS, your model defaults, or `$SHELL`.",
        PLATFORM_COMMON_INSTRUCTIONS.trim(),
        os_specific.trim(),
        environment.os.as_str(),
        environment.shell.as_str(),
        environment.shell_path_display(),
    )
    .trim()
    .to_string()
}

fn add_project_documents(
    snapshot: &mut InstructionSnapshot,
    workspace_root: &std::path::Path,
    current_dir: &std::path::Path,
    config: &InstructionsConfig,
    loaded_documents: Option<&WorkspaceInstructions>,
) -> Result<()> {
    let documents = match loaded_documents {
        Some(documents) => documents.clone(),
        None => load_workspace_instruction_documents(
            workspace_root,
            current_dir,
            config.project_doc_max_bytes,
            &config.project_doc_fallback_filenames,
        )?,
    };
    for document in documents.documents {
        let label = document
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "project document".to_string());
        snapshot.push_user(
            InstructionSource::path(InstructionSourceKind::ProjectDoc, label, &document.path),
            &document.content,
        );
    }
    Ok(())
}
