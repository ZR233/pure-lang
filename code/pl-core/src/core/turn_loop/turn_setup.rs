use std::path::Path;

use pl_model::{ReasoningConfig, ReasoningSummary};
use pl_protocol::Result;

use super::super::TurnEngine;
use crate::ReasoningEffort;
use crate::instruction::{InstructionAssembler, InstructionAssemblyRequest, InstructionSnapshot};
use crate::turn::TurnRequest;

pub(super) fn instruction_snapshot(
    core: &TurnEngine,
    request: &TurnRequest,
    model: &pl_model::ModelInfo,
    workspace_root: &Path,
) -> Result<InstructionSnapshot> {
    let mut snapshot = match request.instruction_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => {
            let assembly_request = InstructionAssemblyRequest {
                instructions: None,
                skills: core.skills.as_ref(),
                skill_catalog: core
                    .skill_catalog
                    .as_deref()
                    .map(|catalog| catalog.snapshot()),
                execution_profile: None,
                model,
                workspace_root,
                current_dir: workspace_root,
                workspace_instructions: request.workspace_instructions.as_deref(),
                subagent_constraint: None,
            };
            match core.instruction_profile.as_ref() {
                Some(profile) => {
                    InstructionAssembler::assemble_with_profile(assembly_request, profile)?
                }
                None => InstructionAssembler::assemble(assembly_request)?,
            }
        }
    };
    if let Some(instruction) = request.skill_invocation_instruction.as_deref() {
        snapshot.push_skill_invocation(instruction);
    }
    Ok(snapshot)
}

pub(super) fn reasoning(effort: Option<&ReasoningEffort>) -> Option<ReasoningConfig> {
    effort.map(|effort| ReasoningConfig {
        effort: Some(effort.as_str().to_string()),
        summary: Some(if effort.is_none() {
            ReasoningSummary::Disabled
        } else {
            ReasoningSummary::Enabled
        }),
    })
}
