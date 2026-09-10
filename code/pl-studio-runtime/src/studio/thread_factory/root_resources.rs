//! Root activation uses the same physical tools as children and loads history without rerendering it.
use super::{
    StudioThreadFactory,
    errors::resource_error,
    thread_seed::{self, ThreadInstructionSeed},
    thread_tools::ThreadToolAssembly,
};
use crate::{
    resource_store::FileResourceStore,
    thread_assembler::{StudioThreadSpec, ThreadAssemblyError},
};
use pl_core::context::{OpaquePayload, ResourceAccess};
use pl_tool::workspace::{AgentWorkspace, ToolWorkspace};
use std::collections::BTreeMap;

impl StudioThreadFactory {
    pub(in crate::studio) async fn prepare_root_thread(
        &self,
        id: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
        let thread = self.thread_record(id).await?;
        if thread.parent_thread_id.is_some() {
            return Err(ThreadAssemblyError::Identity(id.into()));
        }
        let project = self.project_record(&thread.project_id).await?;
        let config = self.services.config_runtime.read()?.config;
        let route = config
            .models
            .resolve(&crate::config::StudioRole::Planner.id())?;
        let history = self
            .services
            .store
            .sessions()
            .read_thread_journal(id)
            .await
            .map_err(|error| resource_error("read Thread journal", error))?;
        let saved = pl_core::thread::journal::replay(&history)?;
        let selected_mode = crate::studio::thread_projection::saved_mode(&saved)
            .map_err(|error| resource_error("read saved Mode", error))?
            .unwrap_or_else(|| thread.mode.clone());
        let mode = self
            .services
            .thread_modes
            .snapshot()
            .mode(&selected_mode)
            .ok_or_else(|| ThreadAssemblyError::Identity(thread.mode.to_string()))?;
        crate::mode::validate_thread_mode_model(Some(&mode), &route.model)?;
        let project_path = project.clone();
        let root = tokio::task::spawn_blocking(move || {
            crate::studio::agent_host::workspace_preparation::resolved_project_root(&project_path)
        })
        .await??;
        let workspace = ToolWorkspace::new(AgentWorkspace::local(&root))
            .with_lsp_runtime(Some(self.services.lsp_runtime.clone()));
        let store = FileResourceStore::new(
            self.services
                .store
                .attachments_dir()
                .join("thread-resources"),
        );
        let mut prepared = self
            .prepare_thread_tools(ThreadToolAssembly {
                thread_id: id,
                cancellation: &cancellation,
                config: &config,
                route: &route,
                project: &project,
                root_thread_id: &thread.root_thread_id,
                workspace,
                store: store.clone(),
            })
            .await?;
        if prepared.visibility != crate::search::ToolVisibilityConstraint::Exclusive {
            prepared.tools = prepared
                .tools
                .with_tools(crate::workflow_tool::workflow_registrations(mode.clone())?);
        }
        let mut initial_context = Vec::new();
        let mut initial_extensions = BTreeMap::new();
        if history.is_empty() {
            let (context, sources) = thread_seed::capture(ThreadInstructionSeed {
                thread_id: id,
                config: &config,
                model: &route.model,
                root: &root,
                label: &mode.descriptor().display_name,
                instructions: mode.prompt(),
                resources: &prepared,
            })
            .await?;
            initial_context = context;
            initial_extensions.insert("studio.instructions".into(), sources);
            if config.runtime.tool_capabilities.ask_user
                && prepared.visibility != crate::search::ToolVisibilityConstraint::Exclusive
            {
                initial_extensions.insert(
                    crate::plan_tool::PLAN_EXTENSION.into(),
                    crate::plan_tool::encode_plan_state(&Default::default())
                        .map_err(|error| resource_error("freeze initial plan state", error))?,
                );
            }

            if let Some(workflow) = crate::mode::reconcile_workflow_for_turn(
                None,
                &mode,
                id,
                crate::studio::unix_seconds(),
            )? {
                if let Some(projection) =
                    crate::mode::workflow_model_context_section(&workflow, &mode)
                {
                    initial_context.push(pl_core::context::ContextRecord {
                        id: format!("workflow:{id}:initial"),
                        turn_id: None,
                        source: pl_core::context::ContextSource::Runtime {
                            source_id: "studio.workflow".into(),
                        },
                        content: vec![pl_core::context::ContextContent::Text {
                            text: projection.content.into(),
                        }],
                        tool_calls: Vec::new(),
                    });
                }
                initial_extensions.insert(
                    crate::workflow_tool::WORKFLOW_EXTENSION.into(),
                    crate::workflow_tool::encode_workflow_state(&workflow)
                        .map_err(|error| resource_error("freeze initial workflow", error))?,
                );
            }

            initial_extensions.insert(
                "studio.project".into(),
                OpaquePayload::new(
                    "pl.studio.project",
                    1,
                    serde_json::to_string(&project)
                        .map_err(|error| resource_error("encode root project", error))?,
                )
                .map_err(|error| resource_error("freeze root project", error))?,
            );
        }
        if !history.is_empty() && mode.workflow().is_some() {
            let restored = pl_core::thread::journal::replay(&history)?;
            let saved = restored
                .extensions
                .get(crate::workflow_tool::WORKFLOW_EXTENSION)
                .ok_or_else(|| {
                    ThreadAssemblyError::Identity("restored root has no workflow state".into())
                })?;
            let workflow = crate::workflow_tool::decode_workflow_state(&saved.payload)
                .map_err(|error| resource_error("decode restored workflow", error))?;
            if crate::mode::workflow_model_context_section(&workflow, &mode).is_none() {
                return Err(ThreadAssemblyError::Identity(
                    "restored workflow requires an explicit mode upgrade".into(),
                ));
            }
        }
        if cancellation.is_cancelled() {
            return Err(pl_core::thread::ThreadError::Cancelled.into());
        }
        let spec = StudioThreadSpec {
            context_preparation: crate::compaction::preparer(
                &route,
                config.runtime.openai_compaction_mode,
            )?,
            agent_controls: if prepared.visibility
                == crate::search::ToolVisibilityConstraint::Exclusive
            {
                crate::thread_assembler::AgentControlExposure::Disabled
            } else {
                crate::thread_assembler::AgentControlExposure::Enabled
            },
            execution: pl_core::thread::input::InputDriverOptions {
                max_model_steps: std::num::NonZeroU32::new(64)
                    .expect("fixed positive model step limit"),
            },
            id: id.into(),
            parent_id: None,
            route,
            hosted_tools: prepared.hosted,
            history,
            initial_context,
            initial_extensions,
            tools: Vec::new(),
            resources: ResourceAccess::new(store),
            capacity: Default::default(),
            cold_store: Some(pl_core::thread::cold::ColdStoreHandle::new(
                self.services.store.sessions().clone(),
            )),
        };
        Ok(prepared.tools.install(spec))
    }
}
