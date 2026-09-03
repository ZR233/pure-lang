use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioRole};
use crate::resolve_workspace_root;
use crate::studio::records::{ProjectRecord, ThreadRecord, ThreadVisibility};
use crate::studio::store::directory::{DirectoryDelta, ProjectDirectoryRecord, ProjectRemoval};

use super::thread_title::{
    ThreadTitleCancellation, ThreadTitleCancellationCause, manual_title, provisional_title,
};
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
        let mode = pl_protocol::ThreadModeId::from_label(request.mode.trim()).map_err(|_| {
            anyhow::Error::new(pl_protocol::studio::StudioError::invalid_argument(
                "mode must be an available mode.* id",
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
        let (delta, thread) = DirectoryDelta::register_root_thread(
            project_id,
            title,
            pl_protocol::ThreadModeId::simple(),
        );
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
        let auto_title = request.title.is_none() && !request.input.text.trim().is_empty();
        let provisional = request
            .title
            .clone()
            .unwrap_or_else(|| provisional_title(&request.input.text));
        let title_prompt = request.input.text.clone();
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
        self.ensure_mode_available(&request.mode)?;

        let (delta, thread) =
            DirectoryDelta::register_root_thread(&request.project_id, &provisional, request.mode);
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
        if auto_title {
            self.title_tasks
                .spawn(self.clone(), thread.id.clone(), provisional, title_prompt)
                .await;
        }
        Ok(StudioStartNewThreadResponse { thread, submission })
    }

    /// Renames a root Thread and publishes the canonical directory update.
    pub async fn rename_thread(&self, thread_id: String, title: String) -> Result<ThreadRecord> {
        let title = manual_title(&title)?;
        self.ensure_persistence_accepts_new_work()?;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.ensure_prompt_runtime_ready().await?;
        let mut thread = self.read_protocol_thread(&thread_id).await?;
        anyhow::ensure!(
            thread.parent_thread_id.is_none(),
            "only root Threads can be renamed"
        );
        if thread.archived {
            bail!("archived Threads cannot be renamed");
        }
        // A user edit is authoritative: stop the best-effort Explorer task
        // before publishing the canonical manual title.
        self.title_tasks
            .cancel(&thread_id, ThreadTitleCancellationCause::ManualRename)
            .await;
        thread.title = title;
        thread.updated_at = crate::studio::unix_seconds();
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                thread_upserts: vec![thread.clone()],
                ..Default::default()
            })
            .await?;
        Ok(ThreadRecord::from_directory_thread(thread))
    }

    pub(super) async fn apply_automatic_thread_title(
        &self,
        thread_id: &str,
        expected_title: &str,
        title: &str,
        cancellation: &mut ThreadTitleCancellation,
    ) -> Result<()> {
        let _lifecycle_guard = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            guard = self.lifecycle_lock.lock() => guard,
        };
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let Some(mut thread) = self
            .agent_facility
            .product_events
            .thread_snapshot(thread_id)
        else {
            return Ok(());
        };
        if thread.archived
            || thread.parent_thread_id.is_some()
            || thread.title != expected_title
            || thread.title == title
        {
            return Ok(());
        }
        thread.title = title.to_string();
        thread.updated_at = crate::studio::unix_seconds();
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                thread_upserts: vec![thread],
                ..Default::default()
            })
            .await?;
        Ok(())
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
        for removed_thread_id in &removed_thread_ids {
            self.title_tasks
                .cancel(
                    removed_thread_id,
                    ThreadTitleCancellationCause::ThreadArchive,
                )
                .await;
        }
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
        self.title_tasks
            .cancel(
                thread_id,
                ThreadTitleCancellationCause::NewThreadCompensation,
            )
            .await;
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
            self.title_tasks
                .cancel(thread_id, ThreadTitleCancellationCause::ProjectArchive)
                .await;
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
            .warm_thread_index(entries);
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
            .warm_thread_index(entries);
        Ok(threads)
    }

    pub async fn set_thread_mode(
        &self,
        thread_id: &str,
        mode: pl_protocol::ThreadModeId,
    ) -> Result<()> {
        self.ensure_persistence_accepts_new_work()?;
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let thread = self.read_owned_thread(thread_id).await?;
        if thread.parent_thread_id.is_some() {
            bail!("only a root Thread can change mode");
        }
        self.ensure_mode_available(&mode)?;
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
        handle
            .change_idle_thread_mode(agent_id, mode.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let mut updated = pl_protocol::Thread::from(thread);
        updated.mode = mode;
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

    fn ensure_mode_available(&self, mode_id: &pl_protocol::ThreadModeId) -> Result<()> {
        anyhow::ensure!(
            self.thread_modes.snapshot().mode(mode_id).is_some(),
            "selected Thread Mode `{mode_id}` is unavailable"
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StudioProductEventKind;
    use crate::studio::runtime::thread_title::title_cancellation_channel;
    use crate::{StudioHostKind, StudioRuntimeOptions};

    async fn runtime_with_thread() -> (tempfile::TempDir, tempfile::TempDir, StudioRuntime, String)
    {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime.start_runtime().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        let thread = runtime
            .create_thread(&project.id, "Old title")
            .await
            .unwrap();
        (home, workspace, runtime, thread.id)
    }

    #[tokio::test]
    async fn manual_rename_publishes_and_persists_directory_title() {
        let (home, _workspace, runtime, thread_id) = runtime_with_thread().await;
        let mut events = runtime.subscribe_product();

        let renamed = runtime
            .rename_thread(thread_id.clone(), "  Manual title  ".to_string())
            .await
            .unwrap();
        assert_eq!(renamed.title, "Manual title");
        assert_eq!(
            runtime
                .read_protocol_thread(&thread_id)
                .await
                .unwrap()
                .title,
            "Manual title"
        );

        let delta = loop {
            let event = events.recv().await.unwrap();
            if let StudioProductEventKind::ThreadDirectoryChanged(delta) = event.kind {
                break delta;
            }
        };
        assert_eq!(delta.upserted[0].title, "Manual title");
        runtime.shutdown_runtime().await.unwrap();
        drop(runtime);

        let reopened = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        })
        .await
        .unwrap();
        reopened.start_runtime().await.unwrap();
        assert_eq!(
            reopened
                .read_protocol_thread(&thread_id)
                .await
                .unwrap()
                .title,
            "Manual title"
        );
        reopened.shutdown_runtime().await.unwrap();
    }

    #[tokio::test]
    async fn automatic_title_uses_cas_and_drops_stale_result() {
        let (_home, _workspace, runtime, thread_id) = runtime_with_thread().await;
        let (_cancellation_owner, mut cancellation) = title_cancellation_channel();
        runtime
            .apply_automatic_thread_title(
                &thread_id,
                "Old title",
                "Explorer title",
                &mut cancellation,
            )
            .await
            .unwrap();
        assert_eq!(
            runtime
                .read_protocol_thread(&thread_id)
                .await
                .unwrap()
                .title,
            "Explorer title"
        );

        runtime
            .rename_thread(thread_id.clone(), "Manual title".to_string())
            .await
            .unwrap();
        runtime
            .apply_automatic_thread_title(
                &thread_id,
                "Explorer title",
                "Stale explorer title",
                &mut cancellation,
            )
            .await
            .unwrap();
        assert_eq!(
            runtime
                .read_protocol_thread(&thread_id)
                .await
                .unwrap()
                .title,
            "Manual title"
        );
        runtime.shutdown_runtime().await.unwrap();
    }

    #[tokio::test]
    async fn manual_rename_rejects_empty_title() {
        let (_home, _workspace, runtime, thread_id) = runtime_with_thread().await;
        let error = runtime
            .rename_thread(thread_id, " \n ".to_string())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("cannot be empty"));
        runtime.shutdown_runtime().await.unwrap();
    }

    #[tokio::test]
    async fn automatic_title_is_dropped_after_archive() {
        let (_home, _workspace, runtime, thread_id) = runtime_with_thread().await;
        runtime
            .agent_facility
            .product_events
            .commit_directory(DirectoryDelta::archive_threads(vec![thread_id.clone()]))
            .await
            .unwrap();
        runtime
            .title_tasks
            .cancel(&thread_id, ThreadTitleCancellationCause::ThreadArchive)
            .await;

        let (_cancellation_owner, mut cancellation) = title_cancellation_channel();
        runtime
            .apply_automatic_thread_title(
                &thread_id,
                "Old title",
                "Explorer title",
                &mut cancellation,
            )
            .await
            .unwrap();
        assert!(
            runtime
                .agent_facility
                .product_events
                .thread_snapshot(&thread_id)
                .is_none()
        );
        runtime.shutdown_runtime().await.unwrap();
    }
}
