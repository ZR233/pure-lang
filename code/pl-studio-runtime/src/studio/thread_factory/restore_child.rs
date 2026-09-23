//! Cold child activation restores saved facts and opens fresh services; a `worktree` child whose
//! saved receipt has no physical worktree (or whose lease was cleaned by an archive) is recreated
//! at the same deterministic path instead of reusing the missing location.
use super::{StudioThreadFactory, errors::resource_error, thread_tools::ThreadToolAssembly};
use crate::{
    resource_store::FileResourceStore,
    thread_assembler::{StudioThreadSpec, ThreadAssemblyError, ThreadPreparation},
};
use pl_core::context::{OpaquePayload, ResourceAccess};
use pl_model::config::{AgentRoleId, ModelRouteConfig};
use pl_tool::workspace::{AgentWorkspace, ToolWorkspace};
use std::{collections::BTreeMap, path::PathBuf};

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
        if thread.role == crate::config::StudioRole::Planner.key() {
            return Err(ThreadAssemblyError::Identity(
                "child role is retired: planner".into(),
            ));
        }
        let project = self.project_record(&thread.project_id).await?;
        let protocol_thread: pl_protocol::Thread = thread.clone().into();
        let checkpoint = super::recovery::load_checkpoint(&self.services.store, &protocol_thread)
            .await
            .map_err(|error| resource_error("load Thread checkpoint", error))?
            .ok_or_else(|| {
                ThreadAssemblyError::Identity(format!("child {id} has no saved checkpoint"))
            })?;
        let restored = checkpoint.state.clone();
        let saved = restored.extensions.get("studio.workspace").ok_or_else(|| {
            ThreadAssemblyError::Identity(format!("child {id} has no workspace receipt"))
        })?;
        let assignment: pl_protocol::AgentWorkspaceAssignmentSnapshot =
            decode(&saved.payload, "pl.studio.workspace")?;
        let saved_profile = crate::studio::model_route::saved_profile(&restored)
            .map_err(|error| resource_error("decode saved child Agent Profile", error))?;
        let bootstrap_profile = saved_profile.is_none();
        let current_profile = if bootstrap_profile {
            let config = self.services.config_runtime.clone();
            let profile_id = thread.role.clone();
            tokio::task::spawn_blocking(move || config.resolve_agent_profile(&profile_id))
                .await?
                .ok()
        } else {
            None
        };
        let config = current_profile.as_ref().map_or_else(
            || {
                self.services
                    .config_runtime
                    .read()
                    .map(|snapshot| snapshot.config)
            },
            |profile| Ok(profile.config.clone()),
        )?;
        let role = AgentRoleId::new(thread.role.clone())?;
        let receipt_route = crate::studio::model_route::latest_request_route(&restored)
            .filter(|route| config.models.resolve_route(role.clone(), route).is_ok());
        let profile = match saved_profile {
            Some((profile, _)) => profile,
            None => migrate_profile(
                &thread.role,
                assignment.mode,
                current_profile.map(|profile| profile.profile),
                receipt_route,
            )?,
        };
        if profile.profile_id != thread.role {
            return Err(ThreadAssemblyError::Identity(
                "saved child Agent Profile identity does not match its directory role".into(),
            ));
        }
        let route_selector = crate::studio::model_route::profile_route(&profile)
            .map_err(|error| resource_error("read saved child model route", error))?;
        let resolved = config.models.resolve_route(role.clone(), &route_selector);
        let (route, model_available) = match resolved {
            Ok(route) => (route, true),
            Err(error) => {
                tracing::warn!(thread_id = id, %error, "saved child model route is unavailable");
                let fallback = config
                    .models
                    .resolve_route(role, config.mode_model_route(&thread.mode)?)?;
                (fallback, false)
            }
        };
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
        if profile.workspace_mode != assignment.mode {
            return Err(ThreadAssemblyError::Identity(
                "saved child Profile conflicts with its workspace assignment".into(),
            ));
        }

        let resources = FileResourceStore::new(self.services.store.session_resources_dir(id));
        let prepared = self
            .prepare_thread_tools(ThreadToolAssembly {
                thread_id: id,
                cancellation: &request.cancellation,
                config: &config,
                route: &route,
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
        let persistence = crate::studio::storage::thread_writer::ThreadStorageSink::new(
            self.services.store.clone(),
            protocol_thread,
        )
        .await
        .map_err(|error| resource_error("open restored child chat session", error))?;
        let mut initial_extensions = BTreeMap::new();
        if bootstrap_profile {
            initial_extensions.insert(
                crate::studio::model_route::AGENT_PROFILE_EXTENSION.into(),
                crate::studio::model_route::encode_profile(&profile).map_err(|error| {
                    resource_error("freeze migrated child Agent Profile", error)
                })?,
            );
        }
        let spec = StudioThreadSpec {
            context_preparation: crate::compaction::preparer(
                &route,
                config.runtime.openai_compaction_mode,
            )?,
            agent_controls: crate::thread_assembler::AgentControlExposure::Disabled,
            execution: pl_core::thread::input::InputDriverOptions {
                max_model_steps: Self::CHILD_MODEL_STEP_LIMIT,
            },
            id: id.clone(),
            parent_id: thread.parent_thread_id,
            route,
            model_available,
            hosted_tools: prepared.hosted,
            checkpoint: Some(checkpoint),
            initial_context: Vec::new(),
            initial_extensions,
            tools: Vec::new(),
            resources: ResourceAccess::new(resources),
            capacity: Default::default(),
            cold_store: Some(pl_core::thread::cold::ColdStoreHandle::new(persistence)),
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

fn migrate_profile(
    profile_id: &str,
    workspace_mode: pl_protocol::AgentWorkspaceMode,
    current: Option<pl_protocol::AgentProfileSnapshot>,
    receipt_route: Option<ModelRouteConfig>,
) -> Result<pl_protocol::AgentProfileSnapshot, ThreadAssemblyError> {
    let mut profile = match current {
        Some(profile) => profile,
        None => {
            let route = receipt_route.as_ref().ok_or_else(|| {
                ThreadAssemblyError::Identity(format!(
                    "child {profile_id} has no saved Agent Profile or valid model receipt"
                ))
            })?;
            pl_protocol::AgentProfileSnapshot {
                profile_id: profile_id.into(),
                display_name: profile_id.into(),
                description: String::new(),
                when_to_use: String::new(),
                system_instructions: String::new(),
                provider_id: route.provider.as_str().into(),
                model: route.model.clone(),
                effort: route
                    .effort
                    .as_ref()
                    .map(|effort| effort.as_str().to_owned()),
                source: "legacy-journal".into(),
                revision: "1".into(),
                content_hash: String::new(),
                system: crate::config::StudioRole::from_key(profile_id).is_some(),
                enabled: true,
                workspace_mode,
            }
        }
    };
    if let Some(route) = receipt_route {
        profile.provider_id = route.provider.into_string();
        profile.model = route.model;
        profile.effort = route.effort.map(|effort| effort.as_str().to_owned());
    }
    let mut hashable = profile.clone();
    hashable.content_hash.clear();
    profile.content_hash = crate::canonical_content_hash(
        &serde_json::to_vec(&hashable)
            .map_err(|error| resource_error("encode migrated child Agent Profile", error))?,
    );
    Ok(profile)
}
