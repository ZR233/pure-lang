//! Physical child resources assembled from one resolved product configuration snapshot.
use super::StudioThreadFactory;
use super::errors::resource_error;
use crate::resource_store::FileResourceStore;
use crate::studio::ThreadRecord;
use crate::thread_assembler::{
    ChildThreadRequest, StudioChildResources, StudioThreadSpec, ThreadAssemblyError,
};
use pl_core::context::{
    ContextContent, ContextRecord, ContextSource, OpaquePayload, ResourceAccess,
};
use pl_tool::workspace::{AgentWorkspace, ToolWorkspace};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

impl StudioChildResources for StudioThreadFactory {
    async fn prepare(
        &self,
        request: &ChildThreadRequest,
        profile: &crate::config::ResolvedAgentProfile,
    ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
        check_cancelled(request)?;
        let parent = self
            .services
            .product_events
            .thread_snapshot(&request.caller)
            .map(ThreadRecord::from_directory_thread)
            .ok_or_else(|| ThreadAssemblyError::Identity(request.caller.clone()))?;
        let project = self
            .services
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == parent.project_id)
            .ok_or_else(|| ThreadAssemblyError::Identity(parent.project_id.clone()))?;
        let (assignment, worktree) =
            crate::studio::agent_host::workspace_preparation::prepare_workspace(
                &self.services.worktrees,
                &self.services.ssh_manager,
                crate::studio::agent_host::workspace_preparation::WorkspacePreparation {
                    project: &project,
                    root_thread_id: &parent.root_thread_id,
                    child_id: &request.id,
                    mode: profile.profile.workspace_mode,
                    writable_paths: request.writable_paths.clone(),
                },
            )
            .await
            .map_err(|error| resource_error("prepare child workspace", error))?;
        check_cancelled(request)?;
        let workspace = ToolWorkspace::new(match assignment.mode {
            pl_protocol::AgentWorkspaceMode::Unrestricted => {
                AgentWorkspace::local(&assignment.root)
            }
            pl_protocol::AgentWorkspaceMode::Directory => AgentWorkspace::directory(
                &assignment.root,
                assignment
                    .writable_paths
                    .as_ref()
                    .map(|paths| paths.iter().map(PathBuf::from).collect()),
            ),
            pl_protocol::AgentWorkspaceMode::Worktree => {
                AgentWorkspace::worktree(&assignment.project_root, &assignment.root)
            }
        })
        .with_lsp_runtime(Some(self.services.lsp_runtime.clone()));
        let store = FileResourceStore::new(
            self.services
                .store
                .attachments_dir()
                .join("thread-resources"),
        );
        let prepared_tools = self
            .prepare_thread_tools(super::thread_tools::ThreadToolAssembly {
                thread_id: &request.id,
                cancellation: &request.cancellation,
                config: &profile.config,
                route: &profile.route,
                project: &project,
                root_thread_id: &parent.root_thread_id,
                workspace,
                store: store.clone(),
            })
            .await?;
        check_cancelled(request)?;
        let receipt = serde_json::to_string(&assignment)
            .map_err(|error| resource_error("encode child workspace receipt", error))?;
        let (mut initial_context, instruction_sources) =
            super::thread_seed::capture(super::thread_seed::ThreadInstructionSeed {
                thread_id: &request.id,
                config: &profile.config,
                model: &profile.route.model,
                root: std::path::Path::new(&assignment.root),
                label: &profile.profile.display_name,
                instructions: &profile.profile.system_instructions,
                resources: &prepared_tools,
            })
            .await?;
        initial_context.push(ContextRecord {
            id: format!("workspace:{}", request.id),
            turn_id: None,
            source: ContextSource::Runtime {
                source_id: "studio.workspace".into(),
            },
            content: vec![ContextContent::Text {
                text: Arc::from(format!("Assigned workspace:\n{receipt}")),
            }],
            tool_calls: Vec::new(),
        });
        let mut initial_extensions = BTreeMap::from([
            ("studio.instructions".into(), instruction_sources),
            (
                "studio.project".into(),
                OpaquePayload::new(
                    "pl.studio.project",
                    1,
                    serde_json::to_string(&project)
                        .map_err(|error| resource_error("encode child project binding", error))?,
                )
                .map_err(|error| resource_error("freeze child project binding", error))?,
            ),
            (
                "studio.workspace".into(),
                OpaquePayload::new("pl.studio.workspace", 1, receipt.clone())
                    .map_err(|error| resource_error("freeze child workspace receipt", error))?,
            ),
            ("studio.creation-metadata".into(), request.metadata.clone()),
        ]);
        if profile.config.runtime.tool_capabilities.ask_user
            && prepared_tools.visibility != crate::search::ToolVisibilityConstraint::Exclusive
        {
            initial_extensions.insert(
                crate::plan_tool::PLAN_EXTENSION.into(),
                crate::plan_tool::encode_plan_state(&Default::default())
                    .map_err(|error| resource_error("freeze initial plan state", error))?,
            );
        }
        self.services
            .product_events
            .register_child_thread(crate::studio::store::directory::RegisteredChildThread {
                id: request.id.clone(),
                parent_thread_id: request.caller.clone(),
                root_thread_id: parent.root_thread_id,
                agent_path: request.id.clone(),
                project_id: parent.project_id,
                mode: parent.mode,
                role: profile.profile.profile_id.clone(),
                title: profile.profile.display_name.clone(),
            })
            .await
            .map_err(|error| resource_error("register child product association", error))?;
        if let Some(worktree) = worktree {
            let mut lease = worktree.lease;
            lease.transition(crate::studio::agent_host::worktree_lease::WorktreeLeaseState::Active);
            self.services
                .worktrees
                .record(lease)
                .map_err(|error| resource_error("activate workspace lease", error))?;
        }
        let spec = StudioThreadSpec {
            context_preparation: crate::compaction::preparer(
                &profile.route,
                profile.config.runtime.openai_compaction_mode,
            )?,
            agent_controls: if prepared_tools.visibility
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
            id: request.id.clone(),
            parent_id: Some(request.caller.clone()),
            route: profile.route.clone(),
            hosted_tools: prepared_tools.hosted,
            history: Vec::new(),
            tools: Vec::new(),
            initial_context,
            initial_extensions,
            resources: ResourceAccess::new(store),
            capacity: Default::default(),
            cold_store: Some(pl_core::thread::cold::ColdStoreHandle::new(
                self.services.store.sessions().clone(),
            )),
        };
        Ok(prepared_tools.tools.install(spec))
    }

    async fn close_published(
        &self,
        id: &str,
        disposition: pl_tool::collaboration::thread::AgentWorkspaceDisposition,
    ) -> Result<(), ThreadAssemblyError> {
        let Some(lease) = self.services.worktrees.get(id) else {
            return Ok(());
        };
        let manager = crate::studio::agent_host::workspace_preparation::manager_from_lease(
            &self.services.ssh_manager,
            &lease,
        );
        close_workspace(&self.services.worktrees, &manager, lease, disposition)
            .await
            .map_err(|error| resource_error("close published workspace", error))
    }

    async fn discard_unpublished(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        if let Some(mut lease) = self.services.worktrees.get(id) {
            use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
            if lease.state != WorktreeLeaseState::Cleaned {
                let manager = crate::studio::agent_host::workspace_preparation::manager_from_lease(
                    &self.services.ssh_manager,
                    &lease,
                );
                let handle = crate::agent::worktree::WorktreeHandle {
                    path: PathBuf::from(&lease.path),
                    branch: lease.branch.clone(),
                    base_commit: lease.base_commit.clone(),
                };
                match manager.discard(&handle).await {
                    Ok(()) => lease.transition(WorktreeLeaseState::Cleaned),
                    Err(error) => {
                        lease.transition(WorktreeLeaseState::Preserved);
                        self.services.worktrees.record(lease).map_err(|failure| {
                            resource_error("retain failed workspace cleanup", failure)
                        })?;
                        return Err(resource_error("discard unpublished workspace", error));
                    }
                }
                self.services
                    .worktrees
                    .record(lease)
                    .map_err(|error| resource_error("record workspace cleanup", error))?;
            }
        }
        if self.services.product_events.thread_snapshot(id).is_some() {
            self.services
                .product_events
                .fault_unregistered_child(id, "child preparation was not published")
                .await
                .map_err(|error| resource_error("record abandoned child", error))?;
        }
        Ok(())
    }
}

fn check_cancelled(request: &ChildThreadRequest) -> Result<(), ThreadAssemblyError> {
    if request.cancellation.is_cancelled() {
        Err(pl_core::thread::ThreadError::Cancelled.into())
    } else {
        Ok(())
    }
}

async fn close_workspace(
    leases: &crate::studio::agent_host::worktree_lease::WorktreeLeaseOwner,
    manager: &crate::agent::worktree::WorktreeManager,
    mut lease: crate::studio::agent_host::worktree_lease::WorktreeLease,
    disposition: pl_tool::collaboration::thread::AgentWorkspaceDisposition,
) -> anyhow::Result<()> {
    use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
    use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
    if lease.state == WorktreeLeaseState::Cleaned {
        return Ok(());
    }
    lease.transition(WorktreeLeaseState::Preserved);
    leases.record(lease.clone())?;
    match disposition {
        AgentWorkspaceDisposition::Preserve => return Ok(()),
        AgentWorkspaceDisposition::Cleanup => {}
    }
    let expected = crate::agent::worktree::WorktreeManager::allocate_path(
        std::path::Path::new(&lease.repository_root),
        &lease.root_thread_id,
        &lease.child_id,
    );
    anyhow::ensure!(
        std::path::Path::new(&lease.path) == expected,
        "worktree cleanup refused a mismatched Pure-owned leaf"
    );
    anyhow::ensure!(
        lease.branch == crate::agent::worktree::WorktreeManager::branch_for(&lease.child_id),
        "worktree cleanup refused a mismatched Pure-owned branch"
    );
    let handle = crate::agent::worktree::WorktreeHandle {
        path: PathBuf::from(&lease.path),
        branch: lease.branch.clone(),
        base_commit: lease.base_commit.clone(),
    };
    manager.preview_existing(&handle).await?;
    lease.transition(WorktreeLeaseState::CleanupRequested);
    leases.record(lease.clone())?;
    if let Err(error) = manager.discard(&handle).await {
        lease.transition(WorktreeLeaseState::Preserved);
        leases.record(lease)?;
        return Err(error.into());
    }
    lease.transition(WorktreeLeaseState::Cleaned);
    leases.record(lease)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::worktree::{LocalWorktreeBackend, WorktreeCreateSpec, WorktreeManager};
    use crate::studio::agent_host::{
        ThreadWriteBehindWriter,
        worktree_lease::{WorktreeLease, WorktreeLeaseOwner, WorktreeLeaseState},
    };
    use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
    use pretty_assertions::assert_eq;

    fn git(root: &std::path::Path, args: &[&str]) {
        let result = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[tokio::test]
    async fn published_workspace_preserves_and_retries_locked_and_partially_deleted_cleanup() {
        let repository = tokio::task::spawn_blocking(|| {
            let directory = tempfile::tempdir().unwrap();
            git(directory.path(), &["init"]);
            git(
                directory.path(),
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "base",
                ],
            );
            directory
        })
        .await
        .unwrap();
        let root = std::fs::canonicalize(repository.path()).unwrap();
        let manager = WorktreeManager::new(root.clone(), Arc::new(LocalWorktreeBackend::default()));
        let handle = manager
            .create(WorktreeCreateSpec {
                repo_root: root.clone(),
                root_thread_id: "root".into(),
                child_id: "child".into(),
                base_commit: manager.resolve_head(&root).await.unwrap(),
            })
            .await
            .unwrap();
        let store = crate::studio::StudioStore::open_memory().await.unwrap();
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let leases = WorktreeLeaseOwner::new(writer.clone());
        let mut lease = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            child_id: "child".into(),
            root_thread_id: "root".into(),
            project_id: "project".into(),
            ssh_server_id: None,
            repository_root: root.to_string_lossy().into_owned(),
            path: handle.path.to_string_lossy().into_owned(),
            branch: handle.branch.clone(),
            base_commit: handle.base_commit.clone(),
        };
        leases.record(lease.clone()).unwrap();
        lease.transition(WorktreeLeaseState::Active);
        leases.record(lease.clone()).unwrap();
        close_workspace(
            &leases,
            &manager,
            lease,
            AgentWorkspaceDisposition::default(),
        )
        .await
        .unwrap();
        assert!(handle.path.exists());
        assert_eq!(
            leases.get("child").unwrap().state,
            WorktreeLeaseState::Preserved
        );
        let git_root = root.clone();
        let path = handle.path.to_string_lossy().into_owned();
        tokio::task::spawn_blocking(move || git(&git_root, &["worktree", "lock", &path]))
            .await
            .unwrap();
        assert!(
            close_workspace(
                &leases,
                &manager,
                leases.get("child").unwrap(),
                AgentWorkspaceDisposition::Cleanup
            )
            .await
            .is_err()
        );
        assert!(handle.path.exists());
        assert_eq!(
            leases.get("child").unwrap().state,
            WorktreeLeaseState::Preserved
        );
        let git_root = root.clone();
        let path = handle.path.to_string_lossy().into_owned();
        tokio::task::spawn_blocking(move || git(&git_root, &["worktree", "unlock", &path]))
            .await
            .unwrap();
        let branch_lock = root
            .join(".git/refs/heads")
            .join(format!("{}.lock", handle.branch));
        std::fs::create_dir_all(branch_lock.parent().unwrap()).unwrap();
        std::fs::write(&branch_lock, "held").unwrap();
        assert!(
            close_workspace(
                &leases,
                &manager,
                leases.get("child").unwrap(),
                AgentWorkspaceDisposition::Cleanup,
            )
            .await
            .is_err()
        );
        assert!(
            !handle.path.exists(),
            "directory removal must succeed before branch deletion is refused"
        );
        assert_eq!(
            leases.get("child").unwrap().state,
            WorktreeLeaseState::Preserved
        );
        std::fs::remove_file(branch_lock).unwrap();
        close_workspace(
            &leases,
            &manager,
            leases.get("child").unwrap(),
            AgentWorkspaceDisposition::Cleanup,
        )
        .await
        .unwrap();
        assert!(!handle.path.exists());
        assert_eq!(
            leases.get("child").unwrap().state,
            WorktreeLeaseState::Cleaned
        );
        writer.shutdown().await.unwrap();
        store.sessions().shutdown().await.unwrap();
    }
}
