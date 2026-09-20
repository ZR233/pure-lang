//! Cold child activation restores saved facts and opens fresh services; a `worktree` child whose
//! saved receipt has no physical worktree (or whose lease was cleaned by an archive) is recreated
//! at the same deterministic path instead of reusing the missing location.
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
        let config = self.services.config_runtime.clone();
        let profile_id = thread.role.clone();
        let profile =
            tokio::task::spawn_blocking(move || config.resolve_agent_profile(&profile_id))
                .await??;

        let project = self.project_record(&thread.project_id).await?;
        let history = super::recovery::recover_journal(&self.services.store, id)
            .await
            .map_err(|error| resource_error("recover Thread journal", error))?;
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
            || previous.ssh_alias != project.ssh_alias
        {
            return Err(ThreadAssemblyError::Identity(
                "child project target changed".into(),
            ));
        }
        // 非 worktree child 的工作区根跟随其根会话；`worktree` child 仍以 Project 仓库
        // `HEAD` 为 base。
        let workspace_mode = thread.workspace_mode;
        let root_thread_id = thread.root_thread_id.clone();
        let project_copy = project.clone();
        let worktrees = self.services.worktrees.clone();
        let (root, session_root) = tokio::task::spawn_blocking(move || {
            let project_root =
                crate::studio::agent_host::workspace_preparation::resolved_project_root(
                    &project_copy,
                )?;
            let session_root =
                crate::studio::agent_host::workspace_preparation::root_session_workspace_root(
                    &worktrees,
                    workspace_mode,
                    &root_thread_id,
                    &project_copy,
                )?;
            Ok::<_, crate::PureError>((project_root, session_root))
        })
        .await??;
        if std::path::Path::new(&assignment.project_root) != root {
            return Err(ThreadAssemblyError::Identity(
                "saved child workspace has a different project root".into(),
            ));
        }
        #[cfg(test)]
        self.record_session_workspace_root_for_test(id, session_root.clone());
        let workspace = match assignment.mode {
            pl_protocol::AgentWorkspaceMode::Unrestricted
                if std::path::Path::new(&assignment.root) == session_root
                    && assignment.worktree.is_none() =>
            {
                AgentWorkspace::host_permitted(root, session_root, None)
            }
            pl_protocol::AgentWorkspaceMode::Directory
                if std::path::Path::new(&assignment.root) == session_root
                    && assignment.worktree.is_none() =>
            {
                AgentWorkspace::host_permitted(
                    root,
                    session_root,
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
                self.ensure_restored_child_worktree(receipt, &thread, &project)
                    .await?;
                AgentWorkspace::worktree(root, PathBuf::from(&receipt.path))
            }
            pl_protocol::AgentWorkspaceMode::Unrestricted
            | pl_protocol::AgentWorkspaceMode::Directory => {
                return Err(ThreadAssemblyError::Identity(
                    "invalid saved child workspace assignment".into(),
                ));
            }
        };
        if profile.profile.workspace_mode != assignment.mode {
            return Err(ThreadAssemblyError::Identity(
                "child Profile workspace policy changed".into(),
            ));
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
            agent_controls: crate::thread_assembler::AgentControlExposure::Disabled,
            execution: pl_core::thread::input::InputDriverOptions {
                max_model_steps: Self::CHILD_MODEL_STEP_LIMIT,
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

    /// 保存的 worktree receipt 对应物理工作树缺失、或 lease 已被归档清理时，在同一确定性
    /// 路径重建工作树并记录 renewed 的 `prepared` lease；已有可用现场时保持不变。
    async fn ensure_restored_child_worktree(
        &self,
        receipt: &pl_protocol::AgentWorktreeSnapshot,
        thread: &crate::studio::ThreadRecord,
        project: &crate::studio::ProjectRecord,
    ) -> Result<(), ThreadAssemblyError> {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        let child_id = &thread.id;
        let recreate = match self.services.worktrees.get(child_id) {
            None => true,
            Some(lease) if lease.state == WorktreeLeaseState::Cleaned => true,
            Some(lease) => {
                let manager = crate::studio::agent_host::workspace_preparation::manager_from_lease(
                    &self.services.ssh_manager,
                    &lease,
                )
                .map_err(|error| resource_error("open restored child workspace manager", error))?;
                let handle = crate::agent::worktree::WorktreeHandle {
                    path: PathBuf::from(&lease.path),
                    branch: lease.branch.clone(),
                    base_commit: lease.base_commit.clone(),
                };
                matches!(manager.preview_existing(&handle).await, Ok(None))
            }
        };
        if !recreate {
            return Ok(());
        }
        let _creating = self.services.worktrees.creation_guard(child_id);
        crate::studio::agent_host::workspace_preparation::recreate_child_worktree(
            &self.services.worktrees,
            &self.services.ssh_manager,
            project,
            &thread.root_thread_id,
            child_id,
            receipt,
        )
        .await
        .map_err(|error| resource_error("recreate restored child worktree", error))?;
        Ok(())
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
