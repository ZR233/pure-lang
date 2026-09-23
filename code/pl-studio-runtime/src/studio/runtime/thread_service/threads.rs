//! Thread 生命周期命令：创建、改名、自动标题、模式切换、归档与失败补偿。

use anyhow::{Context, Result, bail};

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

/// 归档收束轮询间隔：既避免忙等，又让正常收束延迟保持在可忽略范围。
const ARCHIVE_TREE_SETTLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

impl StudioRuntime {
    pub async fn create_thread(&self, project_id: &str, title: &str) -> Result<ThreadRecord> {
        let project = self
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == project_id)
            .context("selected Project not found")?;
        let (delta, thread) = DirectoryDelta::register_root_thread(
            crate::studio::ids::new_id("thread"),
            project_id,
            title,
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
            project.path,
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
        let route = config.config.resolve_mode_model_route(&request.mode)?;
        self.attachment_drafts
            .validate_for_model(&route.model, &drafts)?;
        // 校验走内存目录 owner：open_project 的落库是异步跟随的。
        let projects = self.agent_facility.product_events.project_snapshot().await;
        let project = projects
            .iter()
            .find(|project| project.id == request.project_id)
            .cloned()
            .context("selected Project not found")?;
        self.ensure_mode_available(&request.mode)?;

        // D4：会话工作区在发布 Thread 之前就已确定。`worktree` 先做前置校验与物理创建，
        // 再记录 `active` lease；任一阶段失败都让命令失败且不留下已发布 Thread。
        let thread_id = crate::studio::ids::new_id("thread");
        let mut session_lease = None;
        if request.workspace_mode == pl_protocol::ThreadWorkspaceMode::Worktree {
            // 创建窗口内的 lease 由进程内「创建中」标记保护：在记录 `prepared` 之前设置，
            // 直到 owner Thread 目录事实提交（或失败 settle 并发布）之后才清除。
            self.agent_facility.worktrees.mark_creating(&thread_id);
            let created =
                crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                    &self.agent_facility.worktrees,
                    &self.ssh_manager,
                    &project,
                    &thread_id,
                )
                .await;
            let mut lease = match created {
                Ok(lease) => lease,
                Err(error) => {
                    self.settle_session_worktree_failure(
                        &thread_id,
                        Some("the session worktree could not be created"),
                    )
                    .await;
                    return Err(error.into());
                }
            };
            lease.transition(crate::studio::agent_host::worktree_lease::WorktreeLeaseState::Active);
            if let Err(error) = self.agent_facility.worktrees.record(lease.clone()) {
                self.agent_facility.worktrees.clear_creating(&thread_id);
                self.preserve_session_worktree(
                    Some(lease),
                    Some("the session worktree lease could not be activated"),
                )
                .await;
                return Err(error);
            }
            session_lease = Some(lease);
        }

        // 会话对外只有一个 canonical 工作区地址：`local` 取 canonical Project 目录，
        // `worktree` 取该会话工作树路径；写定后只读（design/12 §12.1）。
        let workspace_path = match &session_lease {
            Some(lease) => lease.path.clone(),
            None => project.path.clone(),
        };
        let (delta, thread) = DirectoryDelta::register_root_thread(
            thread_id,
            &request.project_id,
            &provisional,
            request.mode,
            request.workspace_mode,
            workspace_path,
        );
        // 目录事实内存先行；SQLite 失败进入持久化降级而不是命令失败
        // （design/18 §18.2）。
        if let Err(error) = self
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
        {
            self.clear_creating_session_worktree(&session_lease);
            self.preserve_session_worktree(
                session_lease.take(),
                Some("the new session Thread was not published"),
            )
            .await;
            return Err(error);
        }
        // 目录事实已提交：owner Thread 行已存在，创建窗口收束。
        self.clear_creating_session_worktree(&session_lease);
        let thread = ThreadRecord::from_directory_thread(thread);
        if let Err(error) = self.register_new_thread(thread.clone()).await {
            // 目录事实已发布：必须先按未启动会话补偿（关闭并归档），不能留下无法激活的
            // 已发布会话；会话自身 worktree 仍按 D6 只收束为 `preserved`。
            self.preserve_session_worktree(
                session_lease.take(),
                Some("the new session Thread could not be activated"),
            )
            .await;
            if let Err(cleanup_error) = self.compensate_unstarted_thread(&thread.id).await {
                return Err(error.context(format!(
                    "failed to compensate new Thread {}: {cleanup_error:#}",
                    thread.id
                )));
            }
            return Err(error);
        }
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
                // 已发布 Thread 的补偿归档后，会话自身 worktree 仍按 D6 保留现场等待显式清理。
                self.preserve_session_worktree(
                    session_lease.take(),
                    Some("the first prompt of the new session was rejected"),
                )
                .await;
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
        let root_index = roots
            .iter()
            .position(|candidate| candidate.id == thread_id)
            .context("selected root Thread not found")?;
        let next_root = roots
            .get(root_index + 1)
            .or_else(|| root_index.checked_sub(1).and_then(|index| roots.get(index)))
            .cloned();
        let removed_thread_ids = thread_tree
            .iter()
            .map(|candidate| candidate.id.clone())
            .chain(std::iter::once(thread.id.clone()))
            .collect::<Vec<_>>();
        // 归档先结束整棵树的活动工作（design/01 §1.4）：中断当前 Turn、丢弃未消费输入，
        // 再有界等待收束；无法结束时返回类型化失败并保留会话现场。
        self.end_thread_tree_work(&removed_thread_ids).await?;
        // 归档是破坏性动作：按 per-cause `Cleanup` 收束该会话树拥有的物理工作树
        // （会话自身 lease + 树内 child lease），复用唯一 `close_workspace` 状态机；
        // 任一步失败回落 `preserved` 并发布 Recovery，归档本身仍完成（design/12 §12.5）。
        self.archive_cleanup_workspaces(&removed_thread_ids).await;
        self.retire_archived_thread_tree(&removed_thread_ids)
            .await?;
        self.publish_archived_cleanup_recovery(&removed_thread_ids)
            .await;
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

    /// Restores an archived root and its descendants without executing historical work.
    pub async fn restore_thread(&self, thread_id: String) -> Result<ThreadRecord> {
        let _guard = self.lifecycle_lock.lock().await;
        let root = self.read_protocol_thread(&thread_id).await?;
        anyhow::ensure!(
            root.parent_thread_id.is_none(),
            "only root Threads can be restored"
        );
        let projects = self.agent_facility.product_events.project_snapshot().await;
        anyhow::ensure!(
            projects.iter().any(|project| project.id == root.project_id),
            "open the original Project before restoring its Threads"
        );
        if !root.archived {
            return Ok(ThreadRecord::from_directory_thread(root));
        }
        // 归档清理了该会话树的工作树：`worktree` 会话在恢复时必须在**同一确定性路径**重建，
        // 使会话对外地址在归档与恢复之间保持稳定（design/12 §12.5）。已有可用 lease 时
        // 行为不变。
        if root.workspace_mode == pl_protocol::ThreadWorkspaceMode::Worktree {
            use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
            let project = projects
                .iter()
                .find(|project| project.id == root.project_id)
                .cloned()
                .context("selected Project not found")?;
            let existing = self.agent_facility.worktrees.get(&thread_id);
            // `preserved` 是归档清理失败的现场：物理工作树仍在且身份匹配时重新绑定为
            // `active` 并退掉该 Thread 对应的 Recovery 条目；否则按同一确定性路径重建
            // （design/12 §12.5）。
            let rebindable = match &existing {
                Some(lease) if lease.state == WorktreeLeaseState::Preserved => {
                    let manager =
                        crate::studio::agent_host::workspace_preparation::manager_from_lease(
                            &self.ssh_manager,
                            lease,
                        )?;
                    let handle = crate::agent::worktree::WorktreeHandle {
                        path: std::path::PathBuf::from(&lease.path),
                        branch: lease.branch.clone(),
                        base_commit: lease.base_commit.clone(),
                    };
                    lease.validate_identity().is_ok()
                        && matches!(manager.preview_existing(&handle).await, Ok(Some(_)))
                }
                _ => false,
            };
            if rebindable {
                let mut lease = existing.context("preserved session worktree lease disappeared")?;
                lease.transition(WorktreeLeaseState::Active);
                self.agent_facility
                    .worktrees
                    .record(lease)
                    .map_err(|error| {
                        error.context("rebind the preserved session worktree as active")
                    })?;
            } else if !matches!(&existing, Some(lease) if lease.state == WorktreeLeaseState::Active)
            {
                self.agent_facility.worktrees.mark_creating(&thread_id);
                let created =
                    crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                        &self.agent_facility.worktrees,
                        &self.ssh_manager,
                        &project,
                        &thread_id,
                    )
                    .await;
                let mut lease = match created {
                    Ok(lease) => lease,
                    Err(error) => {
                        self.settle_session_worktree_failure(
                            &thread_id,
                            Some("the restored session worktree could not be recreated"),
                        )
                        .await;
                        return Err(error.into());
                    }
                };
                // 创建落定后立即激活，后续冷激活按 `Active` lease 解析同一路径。
                lease.transition(WorktreeLeaseState::Active);
                if let Err(error) = self.agent_facility.worktrees.record(lease.clone()) {
                    self.agent_facility.worktrees.clear_creating(&thread_id);
                    self.preserve_session_worktree(
                        Some(lease),
                        Some("the restored session worktree lease could not be activated"),
                    )
                    .await;
                    return Err(error);
                }
                self.agent_facility.worktrees.clear_creating(&thread_id);
            }
            // 恢复结束时 worktree 会话必须落到可激活的 `active` lease，否则恢复失败并保留现场。
            anyhow::ensure!(
                self.agent_facility
                    .worktrees
                    .get(&thread_id)
                    .is_some_and(|lease| lease.state == WorktreeLeaseState::Active),
                "restored session worktree lease is not active"
            );
            // 会话工作区已落到可用状态：退掉该 Thread 的 Recovery 条目，否则后续激活会被
            // 已归档遗留的阻断项拒绝（复用既有退订入口）。
            let issues = self.recovery.retire_thread(&thread_id);
            self.agent_facility
                .product_events
                .emit_recovery_state(issues);
        }
        let mut tree = self
            .store
            .read_directory_tree(&thread_id)
            .await?
            .into_iter()
            .map(|record| (record.id.clone(), pl_protocol::Thread::from(record)))
            .collect::<std::collections::BTreeMap<_, _>>();
        for thread in self
            .agent_facility
            .product_events
            .threads_for_root(&thread_id)
        {
            tree.insert(thread.id.clone(), thread);
        }
        tree.insert(root.id.clone(), root);
        let now = crate::studio::unix_seconds();
        for thread in tree.values_mut() {
            thread.archived = false;
            thread.updated_at = now;
        }
        let restored = tree
            .get(&thread_id)
            .context("root missing from restore tree")?
            .clone();
        self.agent_facility
            .product_events
            .commit_directory(DirectoryDelta {
                thread_upserts: tree.into_values().collect(),
                ..Default::default()
            })
            .await?;
        Ok(ThreadRecord::from_directory_thread(restored))
    }

    async fn compensate_unstarted_thread(&self, thread_id: &str) -> Result<()> {
        self.threads.close(thread_id).await?;
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
        Ok(())
    }

    /// 会话自身 worktree 从不随 Thread 生命周期删除：失败路径只把它收束为 `preserved`，
    /// 由 Recovery 显式清理（design/12 §12.5）。
    ///
    /// 收束成功后**立即**基于当前 durable lease 发布/刷新 Recovery 条目（含 preview），
    /// 不依赖下一次启动审计；发布失败只记日志，不改变已经失败的创建命令结果。
    async fn preserve_session_worktree(
        &self,
        lease: Option<crate::studio::agent_host::worktree_lease::WorktreeLease>,
        reason: Option<&str>,
    ) {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        let Some(mut lease) = lease else {
            return;
        };
        if lease.state != WorktreeLeaseState::Preserved {
            lease.transition(WorktreeLeaseState::Preserved);
            if let Err(error) = self.agent_facility.worktrees.record(lease.clone()) {
                tracing::error!(
                    error = %error,
                    "failed to preserve the session worktree lease after a failed creation"
                );
                return;
            }
        }
        self.publish_worktree_recovery(reason, &lease).await;
    }

    /// 创建阶段失败的统一收束出口：清除创建中标记，并在现场被保留为 `preserved` 时立即
    /// 发布带 preview 的 Recovery（同一 issue id upsert）。
    ///
    /// 顺序固定为「调用方已完成 settle → 清除标记 → 发布」：清除标记使并发的周期审计也能
    /// 把该资源算作需要人工处置，避免审计的 `replace` 与本次显式发布互相覆盖导致条目丢失。
    async fn settle_session_worktree_failure(&self, thread_id: &str, reason: Option<&str>) {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        self.agent_facility.worktrees.clear_creating(thread_id);
        let Some(lease) = self.agent_facility.worktrees.get(thread_id) else {
            return;
        };
        if lease.state != WorktreeLeaseState::Preserved {
            return;
        }
        self.publish_worktree_recovery(reason, &lease).await;
    }

    /// 会话 worktree 的创建窗口结束：owner Thread 目录事实已提交（或失败路径已自行清除）。
    fn clear_creating_session_worktree(
        &self,
        lease: &Option<crate::studio::agent_host::worktree_lease::WorktreeLease>,
    ) {
        if let Some(lease) = lease {
            self.agent_facility
                .worktrees
                .clear_creating(&lease.owner_thread_id);
        }
    }

    /// 归档清理该会话树拥有的物理工作树：会话自身 `Session` lease 与树内 `Child` lease
    /// 都按 [`crate::studio::agent_host::workspace_preparation::close_workspace`] 的
    /// `validate_identity → preview_existing → cleanupRequested → discard → cleaned`
    /// 顺序收束。任一步失败回落 `preserved` 且不抛错，归档本身仍完成；失败现场由
    /// [`Self::publish_archived_cleanup_recovery`] 在关闭之后统一发布。
    async fn archive_cleanup_workspaces(&self, thread_ids: &[String]) {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        for thread_id in thread_ids {
            let Some(lease) = self.agent_facility.worktrees.get(thread_id) else {
                continue;
            };
            if lease.state == WorktreeLeaseState::Cleaned {
                continue;
            }
            let manager = match crate::studio::agent_host::workspace_preparation::manager_from_lease(
                &self.ssh_manager,
                &lease,
            ) {
                Ok(manager) => manager,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        thread_id = %thread_id,
                        "archived session tree worktree manager could not be opened"
                    );
                    continue;
                }
            };
            if let Err(error) = crate::studio::agent_host::workspace_preparation::close_workspace(
                &self.agent_facility.worktrees,
                &manager,
                lease,
                pl_tool::collaboration::thread::AgentWorkspaceDisposition::Cleanup,
            )
            .await
            {
                // `close_workspace` 已把 lease 收束为 `preserved` 并 record；归档继续。
                tracing::warn!(
                    %error,
                    thread_id = %thread_id,
                    "archived session tree worktree could not be cleaned; the lease was preserved"
                );
            }
        }
    }

    /// 归档关闭完成之后发布仍为 `preserved` 的会话树工作树 Recovery 卡片，使失败现场
    /// 立即可见且可处置（同一 issue id upsert），不依赖下一次启动审计。
    async fn publish_archived_cleanup_recovery(&self, thread_ids: &[String]) {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        for thread_id in thread_ids {
            let Some(lease) = self.agent_facility.worktrees.get(thread_id) else {
                continue;
            };
            if lease.state != WorktreeLeaseState::Preserved {
                continue;
            }
            self.publish_worktree_recovery(
                Some("the archived session worktree could not be cleaned"),
                &lease,
            )
            .await;
        }
    }

    pub(super) async fn retire_archived_thread_tree(&self, thread_ids: &[String]) -> Result<()> {
        self.close_project_agent_trees(thread_ids).await?;
        for thread_id in thread_ids {
            self.residency.remove(thread_id).await;
            let issues = self.recovery.retire_thread(thread_id);
            self.agent_facility
                .product_events
                .emit_recovery_state(issues);
        }
        Ok(())
    }

    async fn activate_thread_archive_scope(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<(ThreadRecord, Vec<ThreadRecord>, Vec<ThreadRecord>)>> {
        let (mut roots, mut tree) = tokio::try_join!(
            self.store.list_root_threads_for_archive(root_thread_id),
            self.store.list_threads_for_archive(root_thread_id),
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
        // A newly created root may not yet be in SQLite. Archive selection must
        // use the same canonical hot facts as the tree itself.
        let hot = self
            .agent_facility
            .product_events
            .read_thread_directory()
            .await?;
        if let Some(directory) = hot.state.value() {
            for thread in &directory.threads {
                if thread.project_id == root.project_id
                    && thread.parent_thread_id.is_none()
                    && !thread.archived
                {
                    roots.retain(|entry| entry.id != thread.id);
                    roots.push(ThreadRecord::from_directory_thread(thread.clone()));
                }
            }
        }
        roots.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| b.id.cmp(&a.id))
        });
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

    fn ensure_mode_available(&self, mode_id: &pl_protocol::ThreadModeId) -> Result<()> {
        anyhow::ensure!(
            self.thread_modes.snapshot().mode(mode_id).is_some(),
            "selected Thread Mode `{mode_id}` is unavailable"
        );
        Ok(())
    }

    /// 未驻留即不 busy：钉住集合恢复保证有 pending 工作的 Thread 会被恢复，
    /// LRU 只淘汰空闲且已耐久化的 actor（design/17 §17.5）。
    pub(in crate::studio::runtime) async fn thread_is_busy(&self, thread_id: &str) -> Result<bool> {
        let snapshot = self.read_thread_state(thread_id).await?;
        Ok(snapshot
            .turns
            .iter()
            .any(|turn| turn.state == pl_core::thread::TurnState::Running)
            || snapshot
                .inputs
                .iter()
                .any(|input| input.state == pl_core::thread::input::InputState::Pending)
            || snapshot
                .tasks
                .values()
                .any(|task| task.status == pl_core::thread::task::TaskStatus::Running))
    }

    /// 结束会话树的活动工作并有界等待收束，供 `archive_thread` 在 `lifecycle_lock` 下调用。
    ///
    /// 只复用既有能力：`ThreadHandle::interrupt_turn(None)` 停止当前正在执行的那个 Turn
    /// （不指定预期身份），`ThreadHandle::discard_input(id)` 逐条丢弃尚未消费的输入。
    /// 等待期间以 `thread_is_busy` 为唯一判定，不引入第二份并发状态；不得无限自旋，也
    /// 不得在仍 busy 时继续归档。
    ///
    /// 并发契约：`archive_thread` 与所有输入提交入口（`prompt_runner/submit.rs` 的
    /// `submit_prompt*`、`threads.rs` 的 `start_new_thread`）都在同一把 `lifecycle_lock`
    /// 下进入 owner mailbox，因此归档期间不会有并发新输入落入这棵树。
    async fn end_thread_tree_work(&self, thread_ids: &[String]) -> Result<()> {
        for thread_id in thread_ids {
            let Some(thread) = self.threads.thread(thread_id) else {
                // Cold history has no active work and must not be activated for archiving.
                continue;
            };
            match thread.interrupt_turn(None).await {
                // owner 已关闭表示它不再运行新工作，按非 busy 处理而非失败。
                Ok(_) | Err(pl_core::thread::ThreadError::Closed) => {}
                Err(error) => return Err(anyhow::Error::new(error)),
            }
        }
        let deadline = tokio::time::Instant::now() + self.archive_settle_timeout();
        loop {
            let mut still_busy = None;
            for thread_id in thread_ids {
                if self.threads.thread(thread_id).is_none() {
                    continue;
                }
                self.discard_pending_inputs(thread_id).await?;
                if still_busy.is_none() && self.thread_is_busy(thread_id).await? {
                    still_busy = Some(thread_id.clone());
                }
            }
            let Some(thread_id) = still_busy else {
                return Ok(());
            };
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow::Error::new(
                    pl_protocol::studio::StudioError::new(
                        pl_protocol::studio::StudioErrorCode::Busy,
                        "The session could not be ended before archiving",
                        true,
                    )
                    .with_details(serde_json::json!({ "threadId": thread_id })),
                ));
            }
            tokio::time::sleep(ARCHIVE_TREE_SETTLE_POLL_INTERVAL).await;
        }
    }

    /// 丢弃该 Thread 当前 canonical 快照里仍未消费的输入，不改写已消费/已丢弃的记录。
    async fn discard_pending_inputs(&self, thread_id: &str) -> Result<()> {
        let Some(thread) = self.threads.thread(thread_id) else {
            return Ok(());
        };
        let snapshot = self.read_thread_state(thread_id).await?;
        let pending = snapshot
            .inputs
            .iter()
            .filter(|record| record.state == pl_core::thread::input::InputState::Pending)
            .map(|record| record.input.id.clone())
            .collect::<Vec<_>>();
        for input_id in pending {
            match thread.discard_input(input_id).await {
                // 已消费：保持原样；正在被驱动：交由中断收束后的下一轮重试。
                Ok(_)
                | Err(pl_core::thread::ThreadError::InputConsumed)
                | Err(pl_core::thread::ThreadError::InputInUse)
                | Err(pl_core::thread::ThreadError::Closed) => {}
                Err(error) => return Err(anyhow::Error::new(error)),
            }
        }
        Ok(())
    }
}
