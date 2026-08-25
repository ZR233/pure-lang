use pl_model::ModelInfo;
use pl_protocol::Result;

use crate::config::InstructionsConfig;
use crate::workspace::load_workspace_instruction_documents;

use super::{
    InstructionAssemblyRequest, InstructionBlock, InstructionProfile, InstructionSnapshot,
    InstructionSource, InstructionSourceKind,
};

const DEFAULT_BASE_INSTRUCTIONS: &str = include_str!("../prompts/system.md");
const PLATFORM_COMMON_INSTRUCTIONS: &str = include_str!("../prompts/platform/common.md");
#[cfg(windows)]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = include_str!("../prompts/platform/windows.md");
#[cfg(unix)]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = include_str!("../prompts/platform/unix.md");
#[cfg(not(any(windows, unix)))]
const PLATFORM_SPECIFIC_INSTRUCTIONS: &str = "";

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
        snapshot.push_developer(
            InstructionSource::new(InstructionSourceKind::Platform, platform_label()),
            &platform_instructions(),
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
        if let Some(skills) = request.skills {
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

fn platform_label() -> &'static str {
    #[cfg(windows)]
    {
        "platform: windows"
    }
    #[cfg(unix)]
    {
        "platform: unix"
    }
    #[cfg(not(any(windows, unix)))]
    {
        "platform"
    }
}

fn platform_instructions() -> String {
    format!(
        "{}\n\n{}",
        PLATFORM_COMMON_INSTRUCTIONS.trim(),
        PLATFORM_SPECIFIC_INSTRUCTIONS.trim()
    )
    .trim()
    .to_string()
}

fn add_project_documents(
    snapshot: &mut InstructionSnapshot,
    workspace_root: &std::path::Path,
    current_dir: &std::path::Path,
    config: &InstructionsConfig,
) -> Result<()> {
    let documents = load_workspace_instruction_documents(
        workspace_root,
        current_dir,
        config.project_doc_max_bytes,
        &config.project_doc_fallback_filenames,
    )?;
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
