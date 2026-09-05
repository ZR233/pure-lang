//! 指令快照的组装上下文与实现。

use std::path::Path;

use pl_core::ExecutionEnvironment;
use pl_core::WorkspaceInstructions;
use pl_core::instruction::{
    ExecutionInstructionProfile, InstructionAssembler, InstructionAssemblyRequest,
    InstructionSnapshot,
};

use crate::Result;

pub(super) struct StudioInstructionContext<'a> {
    pub(super) config: &'a crate::config::StudioConfig,
    pub(super) model: &'a pl_model::model::ModelInfo,
    pub(super) execution_label: &'a str,
    pub(super) execution_instructions: &'a str,
    pub(super) workspace_root: &'a Path,
    pub(super) workspace_documents: Option<&'a WorkspaceInstructions>,
    pub(super) workspace_instructions: &'a str,
    pub(super) skill_catalog: &'a pl_core::skill::SkillCatalog,
    pub(super) skills_config: &'a pl_core::config::SkillsConfig,
    pub(super) skill_query: &'a str,
    pub(super) excluded_skill_names: &'a [String],
    pub(super) subagent_constraint: Option<&'a str>,
    pub(super) execution_environment: &'a ExecutionEnvironment,
}

pub(super) fn instruction_snapshot(
    context: StudioInstructionContext<'_>,
) -> Result<InstructionSnapshot> {
    InstructionAssembler::assemble(InstructionAssemblyRequest {
        instructions: Some(&context.config.instructions),
        skills: Some(context.skills_config),
        skill_catalog: Some(context.skill_catalog),
        execution_profile: Some(ExecutionInstructionProfile {
            label: context.execution_label,
            instructions: context.execution_instructions,
        }),
        model: context.model,
        workspace_root: context.workspace_root,
        current_dir: context.workspace_root,
        workspace_documents: context.workspace_documents,
        workspace_instructions: Some(context.workspace_instructions),
        subagent_constraint: context.subagent_constraint,
        skill_suggestions: Some(pl_core::instruction::SkillSuggestionRequest {
            query: context.skill_query,
            excluded_names: context.excluded_skill_names,
        }),
        execution_environment: Some(context.execution_environment),
    })
}
