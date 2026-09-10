//! Cold child activation restores saved facts and opens fresh services without recreating worktrees.
use super::{StudioThreadFactory, errors::resource_error, thread_tools::ThreadToolAssembly};
use crate::{
    resource_store::FileResourceStore,
    thread_assembler::{StudioThreadSpec, ThreadAssemblyError, ThreadPreparation},
};
use pl_core::context::{OpaquePayload, ResourceAccess};
use pl_tool::workspace::{AgentWorkspace, ToolWorkspace};
use std::path::PathBuf;

impl StudioThreadFactory {
    pub(super) async fn prepare_restored_child(
        &self,
        request: ThreadPreparation,
    ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
        let id = &request.identity.id;
        let thread = self.thread_record(id).await?;
        if thread.parent_thread_id != request.identity.parent_id
            || thread.parent_thread_id.is_none()
        {
            return Err(ThreadAssemblyError::Identity(id.clone()));
        }
        let project = self.project_record(&thread.project_id).await?;
        let history = self
            .services
            .store
            .sessions()
            .read_thread_journal(id)
            .await
            .map_err(|error| resource_error("read child journal", error))?;
        if history.is_empty() {
            return Err(ThreadAssemblyError::Identity(format!(
                "child {id} has no saved journal"
            )));
        }
        let restored = pl_core::thread::journal::replay(&history)?;
        let saved = restored.extensions.get("studio.workspace").ok_or_else(|| {
            ThreadAssemblyError::Identity(format!("child {id} has no workspace receipt"))
        })?;
        let assignment: pl_protocol::AgentWorkspaceAssignmentSnapshot =
            decode(&saved.payload, "pl.studio.workspace")?;
        let saved_project = restored.extensions.get("studio.project").ok_or_else(|| {
            ThreadAssemblyError::Identity(format!("child {id} has no project binding"))
        })?;
        let previous: crate::studio::ProjectRecord =
            decode(&saved_project.payload, "pl.studio.project")?;
        if previous.id != project.id
            || previous.path != project.path
            || previous.ssh_server_id != project.ssh_server_id
        {
            return Err(ThreadAssemblyError::Identity(
                "child project target changed".into(),
            ));
        }
        let project_copy = project.clone();
        let root = tokio::task::spawn_blocking(move || {
            crate::studio::agent_host::workspace_preparation::resolved_project_root(&project_copy)
        })
        .await??;
        if std::path::Path::new(&assignment.project_root) != root {
            return Err(ThreadAssemblyError::Identity(
                "saved child workspace has a different project root".into(),
            ));
        }
        let workspace = match assignment.mode {
            pl_protocol::AgentWorkspaceMode::Unrestricted
                if std::path::Path::new(&assignment.root) == root
                    && assignment.worktree.is_none() =>
            {
                AgentWorkspace::local(root)
            }
            pl_protocol::AgentWorkspaceMode::Directory
                if std::path::Path::new(&assignment.root) == root
                    && assignment.worktree.is_none() =>
            {
                AgentWorkspace::directory(
                    root,
                    assignment
                        .writable_paths
                        .as_ref()
                        .map(|paths| paths.iter().map(PathBuf::from).collect()),
                )
            }
            pl_protocol::AgentWorkspaceMode::Worktree => {
                let receipt = assignment
                    .worktree
                    .as_ref()
                    .filter(|receipt| receipt.path == assignment.root)
                    .ok_or_else(|| {
                        ThreadAssemblyError::Identity("invalid saved worktree receipt".into())
                    })?;
                AgentWorkspace::worktree(root, PathBuf::from(&receipt.path))
            }
            pl_protocol::AgentWorkspaceMode::Unrestricted
            | pl_protocol::AgentWorkspaceMode::Directory => {
                return Err(ThreadAssemblyError::Identity(
                    "invalid saved child workspace assignment".into(),
                ));
            }
        };
        let config = self.services.config_runtime.clone();
        let profile_id = thread.role.clone();
        let profile =
            tokio::task::spawn_blocking(move || config.resolve_agent_profile(&profile_id))
                .await??;
        if profile.profile.workspace_mode != assignment.mode {
            return Err(ThreadAssemblyError::Identity(
                "child Profile workspace policy changed".into(),
            ));
        }
        if let Some(plan) = restored.extensions.get(crate::plan_tool::PLAN_EXTENSION) {
            crate::plan_tool::decode_plan_state(&plan.payload)
                .map_err(|error| resource_error("decode restored child plan", error))?;
        }
        let resources = FileResourceStore::new(
            self.services
                .store
                .attachments_dir()
                .join("thread-resources"),
        );
        let prepared = self
            .prepare_thread_tools(ThreadToolAssembly {
                thread_id: id,
                cancellation: &request.cancellation,
                config: &profile.config,
                route: &profile.route,
                project: &project,
                root_thread_id: &thread.root_thread_id,
                workspace: ToolWorkspace::new(workspace)
                    .with_lsp_runtime(Some(self.services.lsp_runtime.clone())),
                store: resources.clone(),
            })
            .await?;
        if request.cancellation.is_cancelled() {
            return Err(ThreadAssemblyError::Closed);
        }
        let spec = StudioThreadSpec {
            context_preparation: crate::compaction::preparer(
                &profile.route,
                profile.config.runtime.openai_compaction_mode,
            )?,
            agent_controls: if prepared.visibility
                == crate::search::ToolVisibilityConstraint::Exclusive
            {
                crate::thread_assembler::AgentControlExposure::Disabled
            } else {
                crate::thread_assembler::AgentControlExposure::ProgressOnly
            },
            execution: pl_core::thread::input::InputDriverOptions {
                max_model_steps: std::num::NonZeroU32::new(64)
                    .expect("fixed positive model step limit"),
            },
            id: id.clone(),
            parent_id: thread.parent_thread_id,
            route: profile.route,
            hosted_tools: prepared.hosted,
            history,
            initial_context: Vec::new(),
            initial_extensions: Default::default(),
            tools: Vec::new(),
            resources: ResourceAccess::new(resources),
            capacity: Default::default(),
            cold_store: Some(pl_core::thread::cold::ColdStoreHandle::new(
                self.services.store.sessions().clone(),
            )),
        };
        Ok(prepared.tools.install(spec))
    }
}

fn decode<T: serde::de::DeserializeOwned>(
    payload: &OpaquePayload,
    format: &str,
) -> Result<T, ThreadAssemblyError> {
    if payload.format() != format || payload.version() != 1 {
        return Err(ThreadAssemblyError::Identity(format!(
            "unsupported saved binding {} version {}",
            payload.format(),
            payload.version()
        )));
    }
    serde_json::from_str(payload.content())
        .map_err(|error| resource_error("decode saved workspace binding", error))
}
