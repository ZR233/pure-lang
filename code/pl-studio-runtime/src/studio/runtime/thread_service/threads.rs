//! Thread 生命周期命令：创建、改名、自动标题、模式切换、归档与失败补偿。

use anyhow::{Context, Result, bail};

use crate::config::StudioRole;
use crate::studio::records::ThreadRecord;
use crate::studio::store::directory::DirectoryDelta;

use super::super::StudioRuntime;
use super::super::thread_title::{
    ThreadTitleCancellation, ThreadTitleCancellationCause, manual_title, provisional_title,
};
use super::super::{
    StudioArchiveThreadResult, StudioStartNewThreadRequest, StudioStartNewThreadResponse,
    StudioSubmitPromptRequest,
};

impl StudioRuntime {
    pub async fn create_thread(&self, project_id: &str, title: &str) -> Result<ThreadRecord> {
        let (delta, thread) = DirectoryDelta::register_root_thread(
            project_id,
            title,
            pl_protocol::ThreadModeId::simple(),
        );
        self.agent_facility
            .product_events
            .commit_directory(delta)
            .await?;
        let thread = ThreadRecord::from_directory_thread(thread);
        self.register_new_thread(thread.clone()).await?;
        Ok(thread)
    }

    pub async fn start_new_thread(
        &self,
        request: StudioStartNewThreadRequest,
    ) -> Result<StudioStartNewThreadResponse> {
        super::super::prompt_runner::validate_prompt_content(&request.input)?;
        let auto_title = request.title.is_none() && !request.input.text.trim().is_empty();
        let provisional = request
            .title
            .clone()
            .unwrap_or_else(|| provisional_title(&request.input.text));
        let title_prompt = request.input.text.clone();
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.ensure_prompt_runtime_ready().await?;
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
        self.register_new_thread(thread.clone()).await?;
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

    pub(in crate::studio::runtime) async fn apply_automatic_thread_title(
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

    pub(super) async fn retire_archived_thread_tree(&self, thread_ids: &[String]) {
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

    pub async fn set_thread_mode(
        &self,
        thread_id: &str,
        mode: pl_protocol::ThreadModeId,
    ) -> Result<()> {
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

    /// 未驻留即不 busy：钉住集合恢复保证有 pending 工作的 Thread 会被恢复，
    /// LRU 只淘汰空闲且已耐久化的 actor（design/19 §19.6）。
    pub(in crate::studio::runtime) async fn thread_is_busy(&self, thread_id: &str) -> Result<bool> {
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
    async fn unsaved_new_thread_stays_resident_and_reconnects_from_memory() {
        use sea_orm::ConnectionTrait;
        let (_home, _workspace, runtime, existing_id) = runtime_with_thread().await;
        let project_id = runtime
            .read_owned_thread(&existing_id)
            .await
            .unwrap()
            .project_id;
        let repository = runtime.persistence_repository().await.unwrap();
        repository.writer().flush().await.unwrap();
        runtime.store.database().execute_unprepared("CREATE TRIGGER fail_new_thread BEFORE INSERT ON threads BEGIN SELECT RAISE(ABORT, 'disk i/o error'); END").await.unwrap();
        let thread = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            runtime.create_thread(&project_id, "Unsaved"),
        )
        .await
        .unwrap()
        .unwrap();
        let renamed = runtime
            .rename_thread(thread.id.clone(), "Latest memory title".into())
            .await
            .unwrap();
        assert!(repository.writer().shutdown().await.is_err());
        assert!(
            runtime
                .store
                .read_thread(&thread.id)
                .await
                .unwrap()
                .is_none()
        );
        let (handle, id) = runtime.ensure_thread_agent(&thread.id).await.unwrap();
        assert!(handle.evict_agent(id.clone()).await.is_err());
        assert!(handle.snapshot(id).await.is_ok());
        for selected in [&existing_id, &thread.id] {
            let mut subscription = runtime
                .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
                    thread_id: selected.clone(),
                })
                .await
                .unwrap();
            let frame = subscription.recv().await.unwrap();
            if selected == &thread.id {
                let pl_protocol::ThreadSubscriptionUpdate::Snapshot { snapshot } = frame else {
                    panic!("initial memory snapshot");
                };
                assert_eq!(snapshot.thread.title, renamed.title);
            }
        }
        runtime
            .store
            .database()
            .execute_unprepared("DROP TRIGGER fail_new_thread")
            .await
            .unwrap();
        repository.writer().retry_now();
        repository.writer().flush().await.unwrap();
        assert_eq!(
            runtime
                .store
                .read_thread(&thread.id)
                .await
                .unwrap()
                .unwrap()
                .title,
            renamed.title
        );
        runtime.shutdown_runtime().await.unwrap();
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
