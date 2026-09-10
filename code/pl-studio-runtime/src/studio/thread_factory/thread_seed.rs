//! Creation-only instruction capture using the already selected physical workspace services.
use super::{errors::resource_error, thread_tools::PreparedThreadTools};
use crate::{
    instruction::{ExecutionInstructionProfile, InstructionAssembler, InstructionAssemblyRequest},
    thread_assembler::ThreadAssemblyError,
};
use pl_core::context::{ContextRecord, OpaquePayload};
use std::path::Path;

pub(super) struct ThreadInstructionSeed<'a> {
    pub thread_id: &'a str,
    pub config: &'a crate::config::StudioConfig,
    pub model: &'a pl_model::model::ModelInfo,
    pub root: &'a Path,
    pub label: &'a str,
    pub instructions: &'a str,
    pub resources: &'a PreparedThreadTools,
}

pub(super) async fn capture(
    seed: ThreadInstructionSeed<'_>,
) -> Result<(Vec<ContextRecord>, OpaquePayload), ThreadAssemblyError> {
    let config = seed.config.instructions.clone();
    let root = seed.root.to_owned();
    let documents = match &seed.resources.remote {
        Some(host) => {
            pl_tool::remote::load_remote_workspace_instructions(
                &host.files,
                config.project_doc_max_bytes,
                &config.project_doc_fallback_filenames,
            )
            .await?
        }
        None => {
            let config = config.clone();
            let root = root.clone();
            tokio::task::spawn_blocking(move || {
                pl_tool::workspace::load_workspace_instruction_documents(
                    &root,
                    &root,
                    config.project_doc_max_bytes,
                    &config.project_doc_fallback_filenames,
                )
            })
            .await?
            .map_err(|error| resource_error("read workspace instruction documents", error))?
        }
    };
    let model = seed.model.clone();
    let label = seed.label.to_owned();
    let instructions = seed.instructions.to_owned();
    let environment = seed.resources.environment.clone();
    let catalog = seed.resources.catalog.clone();
    let skills = seed.config.skills.clone();
    let snapshot = tokio::task::spawn_blocking(move || {
        InstructionAssembler::assemble(InstructionAssemblyRequest {
            instructions: Some(&config),
            skills: catalog.as_ref().map(|_| &skills),
            skill_catalog: catalog.as_ref().map(|catalog| catalog.snapshot()),
            execution_profile: Some(ExecutionInstructionProfile {
                label: &label,
                instructions: &instructions,
            }),
            model: &model,
            workspace_root: &root,
            current_dir: &root,
            workspace_documents: Some(&documents),
            workspace_instructions: None,
            subagent_constraint: None,
            skill_suggestions: None,
            execution_environment: Some(&environment),
        })
    })
    .await??;
    let payload = OpaquePayload::new(
        "pl.studio.instructions",
        1,
        serde_json::to_string(&snapshot)
            .map_err(|error| resource_error("encode instruction sources", error))?,
    )
    .map_err(|error| resource_error("freeze instruction sources", error))?;
    let records = snapshot.context_records(seed.thread_id);
    Ok((records, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_assembler::StudioThreadTools;
    use pl_core::context::{ContextContent, ContextSource};
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn initial_capture_is_deterministic_and_project_documents_remain_runtime_context() {
        let directory = tempfile::tempdir().unwrap();
        tokio::fs::write(directory.path().join("AGENTS.md"), "原始项目说明\n")
            .await
            .unwrap();
        let config = crate::config::StudioConfig::default();
        let model = pl_model::model::ModelInfo::compatible("seed-model");
        let resources = PreparedThreadTools {
            catalog: None,
            visibility: crate::search::ToolVisibilityConstraint::Unavailable,
            tools: StudioThreadTools::selected(
                crate::resource_store::FileResourceStore::new(directory.path().join("resources")),
                Vec::new(),
            ),
            hosted: Vec::new(),
            remote: None,
            environment: pl_tool::environment::ExecutionEnvironment::detect_local(),
        };
        let seed = || ThreadInstructionSeed {
            thread_id: "thread",
            config: &config,
            model: &model,
            root: directory.path(),
            label: "role",
            instructions: "fixed role instructions",
            resources: &resources,
        };
        let original = capture(seed()).await.unwrap();
        assert_eq!(capture(seed()).await.unwrap(), original);
        let mut runtime_seen = false;
        for record in &original.0 {
            match record.source {
                ContextSource::Instruction => assert!(
                    !runtime_seen,
                    "stable instructions must form the leading prefix"
                ),
                ContextSource::Runtime { .. } => runtime_seen = true,
                ContextSource::User
                | ContextSource::Assistant
                | ContextSource::ToolResult { .. } => {
                    panic!("unexpected source in instruction seed")
                }
            }
            for content in &record.content {
                if let ContextContent::Text { text } = content
                    && text.contains("原始项目说明")
                {
                    assert!(matches!(record.source, ContextSource::Runtime { .. }));
                }
            }
        }
        assert!(runtime_seen);
        let frozen = original.clone();
        tokio::fs::write(directory.path().join("AGENTS.md"), "新版项目说明\n")
            .await
            .unwrap();
        let changed = capture(seed()).await.unwrap();
        assert_ne!(changed, original);
        assert_eq!(original, frozen);
        assert!(original.1.content().contains("原始项目说明"));
    }
}
