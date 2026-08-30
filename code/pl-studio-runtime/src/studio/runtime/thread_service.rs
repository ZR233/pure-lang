use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioRole};
use crate::studio::records::{ProjectRecord, ThreadRecord, ThreadVisibility};
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord, ProjectRemoval};
use crate::{StudioMode, resolve_workspace_root};

use super::{
    StudioArchiveThreadResult, StudioRuntime, StudioStartNewThreadRequest,
    StudioStartNewThreadResponse, StudioSubmitPromptRequest,
};

impl StudioRuntime {
    /// Creates a root Thread and accepts its first Turn from the shared API request.
    pub async fn create_thread_command(
        &self,
        project_id: String,
        request: pl_protocol::studio::CreateThreadRequest,
    ) -> Result<StudioStartNewThreadResponse> {
        let mode = StudioMode::from_label(request.mode.trim()).map_err(|_| {
            anyhow::Error::new(pl_protocol::studio::StudioError::invalid_argument(
                "mode must be an available mode.* Skill id",
            ))
        })?;
        self.start_new_thread(StudioStartNewThreadRequest {
            project_id,
            title: request.title,
            input: request.input,
            mode,
            options: super::StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..super::StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        self.ensure_persistence_accepts_new_work()?;
        let path = path.as_ref();
        let _ = resolve_workspace_root(path)?;
        let path_text = path.to_string_lossy().to_string();
        let name = crate::studio::paths::project_name(path);
        let now = crate::studio::unix_seconds();
        // 聚合冷加载：按 path 找到既有行或分配新 id，然后内存先行提交目录 delta。
        let existing = self.store.find_project_by_path(&path_text, None).await?;
        let (record, delta_record) = match existing {
            Some(existing) => {
                let delta_record = ProjectDirectoryRecord {
                    id: existing.id.clone(),
                    name: name.clone(),
                    path: path_text.clone(),
                    ssh_server_id: None,
                    created_at: existing.created_at,
                    updated_at: now,
                    last_opened_at: Some(now),
                    closed: false,
                };
                let public = ProjectRecord {
                    id: existing.id.clone(),
                    name,
                    path: path_text,
                    ssh_server_id: None,
                    updated_at: now,
                };
                (public, delta_record)
            }
            None => {
                let id = crate::studio::ids::new_id("project");
                let delta_record = ProjectDirectoryRecord {
                    id: id.clone(),
                    name: name.clone(),
                    path: path_text.clone(),
                    ssh_server_id: None,
                    created_at: now,
                    updated_at: now,
                    last_opened_at: Some(now),
                    closed: false,
                };
                let public = ProjectRecord {
                    id,
                    name,
                    path: path_text,
                    ssh_server_id: None,
                    updated_at: now,
                };
                (public, delta_record)
            }
        };
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::upsert_project(delta_record))
            .await?;
        Ok(record)
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        Ok(self.agent_facility.product_events.project_snapshot().await)
    }

    pub async fn create_thread(&self, project_id: &str, title: &str) -> Result<ThreadRecord> {
        self.ensure_persistence_accepts_new_work()?;
        let (delta, thread) =
            DirectoryDelta::register_root_thread(project_id, title, StudioMode::simple());
        self.agent_facility
            .product_events
            .commit_directory(delta)
            .await?;
        Ok(ThreadRecord::from_directory_thread(thread))
    }

    pub async fn start_new_thread(
        &self,
        request: StudioStartNewThreadRequest,
    ) -> Result<StudioStartNewThreadResponse> {
        super::prompt_runner::validate_prompt_content(&request.input)?;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.ensure_prompt_runtime_ready().await?;
        self.ensure_persistence_accepts_new_work()?;
        let drafts = self
            .attachment_drafts
            .resolve(&request.input.attachment_draft_ids)
            .await?;
        let config = self.config_runtime.read()?;
        let route = config.config.models.resolve(&StudioRole::Planner.id())?;
        self.attachment_drafts
            .validate_for_model(&route.model, &drafts)?;
        // 校验走内存目录 owner：open_project 的落库是异步跟随的。
        let projects = self.agent_facility.product_events.project_snapshot().await;
        anyhow::ensure!(
            projects
                .iter()
                .any(|project| project.id == request.project_id),
            "selected Project not found"
        );
        self.ensure_mode_available(&request.project_id, request.mode.label())
            .await?;

        let (delta, thread) =
            DirectoryDelta::register_root_thread(&request.project_id, &request.title, request.mode);
        // 目录事实内存先行；SQLite 失败进入持久化降级而不是命令失败
        // （design/20 §20.4）。
        self.agent_facility
            .product_events
            .commit_directory(delta)
            .await?;
        let thread = ThreadRecord::from_directory_thread(thread);
        let submission = self
            .submit_prompt_for_owned_thread_with_lifecycle_lock(
                StudioSubmitPromptRequest {
                    thread_id: thread.id.clone(),
                    input: request.input,
                    options: request.options,
                },
                thread.clone(),
            )
            .await;
        let submission = match submission {
            Ok(submission) => submission,
            Err(error) => {
                if let Err(cleanup_error) = self.compensate_unstarted_thread(&thread.id).await {
                    return Err(error.context(format!(
                        "failed to compensate new Thread {}: {cleanup_error:#}",
                        thread.id
                    )));
                }
                return Err(error);
            }
        };
        Ok(StudioStartNewThreadResponse { thread, submission })
    }

    pub async fn archive_thread(
        &self,
        thread_id: String,
    ) -> Result<Option<StudioArchiveThreadResult>> {
        self.ensure_persistence_accepts_new_work()?;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let Some((thread, roots, thread_tree)) =
            self.activate_thread_archive_scope(&thread_id).await?
        else {
            return Ok(None);
        };
        let _pins = self
            .residency
            .pin_many(thread_tree.iter().map(|thread| thread.id.clone()));
        for candidate in &thread_tree {
            let _ = self.ensure_thread_agent(&candidate.id).await?;
        }
        let root_index = roots
            .iter()
            .position(|candidate| candidate.id == thread_id)
            .context("selected root Thread not found")?;
        let next_root = roots
            .get(root_index + 1)
            .or_else(|| root_index.checked_sub(1).and_then(|index| roots.get(index)))
            .cloned();
        for candidate in &thread_tree {
            if self.thread_is_busy(&candidate.id).await? {
                bail!("thread tree has an active turn or pending input");
            }
        }
        for candidate in &thread_tree {
            let emitter = self.interaction_emitter(candidate.id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(
                    self.pending_thread_interactions(&candidate.id).await?,
                    "thread archived",
                    emitter,
                )
                .await?;
        }
        let removed_thread_ids = thread_tree
            .iter()
            .map(|candidate| candidate.id.clone())
            .chain(std::iter::once(thread.id.clone()))
            .collect::<Vec<_>>();
        self.retire_archived_thread_tree(&removed_thread_ids).await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::archive_threads(removed_thread_ids.clone()))
            .await?;
        self.model_performance.remove_session(&thread.id).await?;
        Ok(Some(StudioArchiveThreadResult {
            archived_root_id: thread.id,
            removed_thread_ids,
            next_root,
        }))
    }

    async fn compensate_unstarted_thread(&self, thread_id: &str) -> Result<()> {
        let actor_cleanup_error = self
            .close_project_agent_trees(&[thread_id.to_string()])
            .await
            .err();
        self.residency.remove(thread_id).await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta::archive_threads(vec![thread_id.to_string()]))
            .await?;
        self.model_performance.remove_session(thread_id).await?;
        if let Some(error) = actor_cleanup_error {
            return Err(error).context(format!(
                "new Thread {thread_id} was archived but its actor cleanup failed"
            ));
        }
        Ok(())
    }

    async fn retire_archived_thread_tree(&self, thread_ids: &[String]) {
        if let Err(error) = self.close_project_agent_trees(thread_ids).await {
            tracing::warn!(
                root_thread_id = thread_ids.first().map(String::as_str).unwrap_or_default(),
                error = %error,
                "archived Thread actor cleanup deferred"
            );
        }
        for thread_id in thread_ids {
            self.residency.remove(thread_id).await;
        }
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        self.ensure_persistence_accepts_new_work()?;
        let Some(project) = self
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == project_id)
        else {
            return Ok(None);
        };
        let threads = self.activate_project_archive_scope(project_id).await?;
        let thread_ids = threads
            .iter()
            .map(|thread| thread.id.clone())
            .collect::<Vec<_>>();
        let active_threads = threads
            .iter()
            .filter(|thread| thread.visibility == ThreadVisibility::Active)
            .collect::<Vec<_>>();
        let _pins = self
            .residency
            .pin_many(active_threads.iter().map(|thread| thread.id.clone()));
        for thread in &active_threads {
            let _ = self.ensure_thread_agent(&thread.id).await?;
        }
        for thread in &active_threads {
            if self.thread_is_busy(&thread.id).await? {
                bail!("project has an active turn");
            }
        }
        for thread in &active_threads {
            let emitter = self.interaction_emitter(thread.id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(
                    self.pending_thread_interactions(&thread.id).await?,
                    "project archived",
                    emitter,
                )
                .await?;
        }
        self.retire_archived_thread_tree(&thread_ids).await;
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                project_removals: vec![ProjectRemoval {
                    project_id: project.id.clone(),
                    thread_ids: thread_ids.clone(),
                    closed_at: crate::studio::unix_seconds(),
                }],
                ..Default::default()
            })
            .await?;
        for thread_id in &thread_ids {
            self.model_performance.remove_session(thread_id).await?;
        }
        Ok(Some(project))
    }

    /// 归档是跨 owner 命令；先原子物化它需要的冷目录范围，再执行全部业务校验。
    async fn activate_thread_archive_scope(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<(ThreadRecord, Vec<ThreadRecord>, Vec<ThreadRecord>)>> {
        let (roots, mut tree) = tokio::try_join!(
            self.store.list_root_threads_for_activation(root_thread_id),
            self.store.list_threads_for_root(root_thread_id),
        )?;
        for hot in self
            .agent_facility
            .product_events
            .threads_for_root(root_thread_id)
        {
            if !tree.iter().any(|candidate| candidate.id == hot.id) {
                tree.push(ThreadRecord::from_directory_thread(hot));
            }
        }
        let Some(root) = tree
            .iter()
            .find(|thread| thread.id == root_thread_id && thread.parent_thread_id.is_none())
            .cloned()
        else {
            return Ok(None);
        };
        let mut entries = roots
            .iter()
            .chain(tree.iter())
            .cloned()
            .map(pl_protocol::Thread::from)
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        entries.dedup_by(|left, right| left.id == right.id);
        self.agent_facility
            .product_events
            .apply_thread_delta(entries, Vec::new())
            .await?;
        Ok(Some((root, roots, tree)))
    }

    async fn activate_project_archive_scope(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        let threads = self.store.list_threads_for_project(project_id).await?;
        let entries = threads
            .iter()
            .filter(|thread| thread.visibility == ThreadVisibility::Active)
            .cloned()
            .map(pl_protocol::Thread::from)
            .collect();
        self.agent_facility
            .product_events
            .apply_thread_delta(entries, Vec::new())
            .await?;
        Ok(threads)
    }

    pub async fn set_thread_mode(&self, thread_id: &str, mode: StudioMode) -> Result<()> {
        self.ensure_persistence_accepts_new_work()?;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let thread = self.read_owned_thread(thread_id).await?;
        if thread.parent_thread_id.is_some() {
            bail!("only a root Thread can change mode");
        }
        self.ensure_mode_available(&thread.project_id, mode.label())
            .await?;
        let (handle, agent_id) = self.ensure_thread_agent(thread_id).await?;
        let snapshot = handle
            .snapshot(agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        if snapshot.active_turn_id().is_some()
            || snapshot.pending_inputs > 0
            || !snapshot.state.is_idle()
        {
            bail!("thread mode cannot change while the Thread is running or has pending input");
        }
        if !self
            .pending_thread_interactions(thread_id)
            .await?
            .is_empty()
        {
            bail!("thread mode cannot change while an interaction is pending");
        }
        let context = handle
            .read_thread_context(agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        if context
            .session
            .workflow()
            .and_then(|workflow| workflow.current_run.as_ref())
            .is_some_and(|run| run.lifecycle == pl_protocol::WorkflowRunLifecycle::Active)
        {
            bail!("thread mode cannot change while a workflow run is active");
        }
        let mut updated = pl_protocol::Thread::from(thread);
        updated.mode = pl_protocol::ThreadMode::from(mode);
        updated.updated_at = crate::studio::unix_seconds();
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                thread_upserts: vec![updated],
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    async fn ensure_mode_available(&self, project_id: &str, mode_id: &str) -> Result<()> {
        let catalog = self.skills.read(project_id).await.catalog_for_turn();
        if let Some(catalog) = catalog {
            anyhow::ensure!(
                catalog.find_mode(mode_id).is_some(),
                "selected Mode Skill `{mode_id}` is unavailable"
            );
        } else {
            anyhow::ensure!(
                pl_core::skill::BUILTIN_MODE_IDS.contains(&mode_id),
                "custom Mode Skill `{mode_id}` requires a discovered project Skill catalog"
            );
        }
        Ok(())
    }

    pub fn set_model_role(
        &self,
        expected_settings_revision: u64,
        role: StudioRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<crate::ConfigRuntimeSnapshot> {
        let provider_id = provider_id.trim();
        let model_slug = model_slug.trim();
        let current = self.config_runtime.read()?;
        anyhow::ensure!(
            current.revision == expected_settings_revision,
            "settings revision conflict: expected {expected_settings_revision}, actual {}",
            current.revision
        );
        let mut config = current.config;
        let provider_key = ProviderId::new(provider_id)?;
        let resolved_effort = {
            let provider = config
                .models
                .providers
                .get(&provider_key)
                .with_context(|| {
                    format!(
                        "role {} references missing provider: {provider_id}",
                        role.key()
                    )
                })?;
            let models = provider.effective_models()?;
            let model = models
                .iter()
                .find(|model| model.slug == model_slug)
                .with_context(|| {
                    format!(
                        "role {} references missing model: {provider_id}.{model_slug}",
                        role.key()
                    )
                })?;
            match effort.map(str::trim).filter(|value| !value.is_empty()) {
                Some(value) => {
                    if !model
                        .supported_efforts()
                        .iter()
                        .any(|candidate| candidate == value)
                    {
                        bail!(
                            "role {} uses unsupported effort '{}' for model {provider_id}.{model_slug}",
                            role.key(),
                            value
                        );
                    }
                    Some(value.to_string())
                }
                None => model.default_effort(),
            }
        };
        let next_route = ModelRouteConfig {
            provider: provider_key,
            model: model_slug.to_string(),
            effort: resolved_effort.map(ReasoningEffort::new),
        };
        config.models.routes.insert(role.id(), next_route);
        config.validate()?;
        Ok(self.config_runtime.replace(current.revision, config)?)
    }

    /// 未驻留即不 busy：钉住集合恢复保证有 pending 工作的 Thread 会被恢复，
    /// LRU 只淘汰空闲且已耐久化的 actor（design/19 §19.6）。
    pub(super) async fn thread_is_busy(&self, thread_id: &str) -> Result<bool> {
        let Some((handle, agent_id)) = self.try_get_thread_handle(thread_id).await? else {
            return Ok(false);
        };
        match handle.snapshot(agent_id).await {
            Ok(snapshot) => Ok(snapshot.active_turn_id().is_some() || snapshot.pending_inputs > 0),
            Err(pl_core::AgentRuntimeError::NotFound(_)) => Ok(false),
            Err(error) => Err(anyhow::anyhow!(error)),
        }
    }
}
