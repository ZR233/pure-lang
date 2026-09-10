//! Product Mode changes are a single idle core transaction followed by directory projection.
use super::StudioRuntime;
use anyhow::{Context, Result};
use pl_core::{
    context::OpaquePayload,
    thread::{
        ContextReplacementReason, IdleReconfiguration, ReplaceContext, RuntimeFact,
        extensions::{ApplicationUpdate, ExtensionMutation},
    },
};
use pl_protocol::ThreadModeId;

impl StudioRuntime {
    pub async fn set_thread_mode(&self, thread_id: &str, mode_id: ThreadModeId) -> Result<()> {
        let _guard = self.lifecycle_lock.lock().await;
        let record = self.read_owned_thread(thread_id).await?;
        anyhow::ensure!(
            record.parent_thread_id.is_none(),
            "only a root Thread can change mode"
        );
        let mode = self
            .thread_modes
            .snapshot()
            .mode(&mode_id)
            .context("selected Thread Mode is unavailable")?;
        let route = self
            .config_runtime
            .read()?
            .config
            .models
            .resolve(&crate::config::StudioRole::Planner.id())?;
        crate::mode::validate_thread_mode_model(Some(&mode), &route.model)?;
        let thread = self.ensure_thread_owner(thread_id).await?;
        let state = thread.snapshot();
        let previous = state
            .extensions
            .get("studio.instructions")
            .context("Thread has no saved instruction sources")?;
        anyhow::ensure!(
            previous.payload.format() == "pl.studio.instructions"
                && previous.payload.version() == 1,
            "unsupported saved instructions"
        );
        let mut instructions: crate::instruction::InstructionSnapshot =
            serde_json::from_str(previous.payload.content())?;
        let profile = instructions
            .developer
            .iter_mut()
            .find(|block| {
                block.source.kind == crate::instruction::InstructionSourceKind::ExecutionProfile
            })
            .context("saved instructions contain no execution profile")?;
        profile
            .source
            .label
            .clone_from(&mode.descriptor().display_name);
        profile.content = mode.prompt().to_owned();
        let mut records = instructions.context_records(thread_id);
        let instruction_ids = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        records.extend(
            state
                .context
                .records
                .iter()
                .filter(|record| !instruction_ids.contains(&record.id))
                .cloned(),
        );
        let workflow_record = state
            .extensions
            .get(crate::workflow_tool::WORKFLOW_EXTENSION);
        let previous_workflow = workflow_record
            .map(|record| crate::workflow_tool::decode_workflow_state(&record.payload))
            .transpose()?;
        let now = crate::studio::unix_seconds();
        let workflow =
            crate::mode::reconcile_workflow_for_turn(previous_workflow, &mode, thread_id, now)?;
        let mode_record = state.extensions.get("studio.mode");
        let mut mutations = vec![
            ExtensionMutation::Put {
                id: "studio.mode".into(),
                expected_revision: mode_record.map(|record| record.revision),
                payload: OpaquePayload::new("pl.studio.mode", 1, serde_json::to_string(&mode_id)?)?,
            },
            ExtensionMutation::Put {
                id: "studio.instructions".into(),
                expected_revision: Some(previous.revision),
                payload: OpaquePayload::new(
                    "pl.studio.instructions",
                    1,
                    serde_json::to_string(&instructions)?,
                )?,
            },
        ];
        let mut facts = state
            .runtime_facts
            .iter()
            .filter(|fact| fact.source_id != "studio.workflow")
            .cloned()
            .collect::<Vec<_>>();
        if let Some(workflow) = &workflow {
            mutations.push(ExtensionMutation::Put {
                id: crate::workflow_tool::WORKFLOW_EXTENSION.into(),
                expected_revision: workflow_record.map(|record| record.revision),
                payload: crate::workflow_tool::encode_workflow_state(workflow)?,
            });
            if let Some(section) = crate::mode::workflow_model_context_section(workflow, &mode) {
                facts.push(RuntimeFact {
                    source_id: "studio.workflow".into(),
                    content: vec![pl_core::context::ContextContent::Text {
                        text: section.content.into(),
                    }],
                });
            }
        }
        thread
            .reconfigure(IdleReconfiguration {
                expected_sequence: state.commit_sequence,
                application: ApplicationUpdate { mutations, facts },
                context: Some(ReplaceContext {
                    expected_revision: state.context.revision,
                    reason: ContextReplacementReason::Rebuild,
                    records,
                }),
                remove_tools: [
                    crate::workflow_tool::TOOL_WORKFLOW_CURRENT,
                    crate::workflow_tool::TOOL_WORKFLOW_GRAPH,
                    crate::workflow_tool::TOOL_WORKFLOW_HISTORY,
                    crate::workflow_tool::TOOL_WORKFLOW_NEXT,
                    crate::workflow_tool::TOOL_WORKFLOW_RESTART,
                    crate::workflow_tool::TOOL_WORKFLOW_TRANSITION,
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
                tools: crate::workflow_tool::workflow_registrations(mode)?,
            })
            .await?;
        let mut directory = pl_protocol::Thread::from(record);
        directory.mode = mode_id;
        directory.updated_at = now;
        self.agent_facility
            .product_events
            .commit_directory(crate::studio::store::directory::DirectoryDelta {
                thread_upserts: vec![directory],
                ..Default::default()
            })
            .await?;
        Ok(())
    }
}
