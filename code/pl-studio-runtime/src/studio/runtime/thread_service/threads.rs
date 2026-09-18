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

/// 归档收束轮询间隔：既避免忙等，又让正常收束延迟保持在可忽略范围。
const ARCHIVE_TREE_SETTLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

impl StudioRuntime {
    pub async fn create_thread(&self, project_id: &str, title: &str) -> Result<ThreadRecord> {
        let (delta, thread) = DirectoryDelta::register_root_thread(
            crate::studio::ids::new_id("thread"),
            project_id,
            title,
            pl_protocol::ThreadModeId::simple(),
            pl_protocol::ThreadWorkspaceMode::Local,
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

        let (delta, thread) = DirectoryDelta::register_root_thread(
            thread_id,
            &request.project_id,
            &provisional,
            request.mode,
            request.workspace_mode,
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
        for candidate in &thread_tree {
            let _ = self.ensure_thread_owner(&candidate.id).await?;
        }
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
        self.retire_archived_thread_tree(&removed_thread_ids)
            .await?;
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
        let (snapshot, _) = self.read_thread_facts(thread_id).await?;
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
                // 未被激活的成员不可能在运行工作，与 `thread_is_busy` 的冷读语义一致。
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
        let (snapshot, _) = self.read_thread_facts(thread_id).await?;
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

    #[cfg(test)]
    fn set_archive_settle_timeout(&mut self, timeout: std::time::Duration) {
        self.archive_settle_timeout = timeout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StudioProductEventKind;
    use crate::studio::runtime::thread_title::title_cancellation_channel;
    use crate::{StudioHostKind, StudioRuntimeOptions};
    use pl_core::context::OpaquePayload;
    use pl_core::model::{
        DynModelSession, Model, ModelError, ModelFactory, ModelFailureKind, ModelRequest,
        ModelSession, PreparedModelCall,
    };
    use pl_core::thread::input::{InputDriverOptions, InputState, ThreadInput};
    use pl_core::thread::{ThreadHandle, ThreadLifecycle, TurnState};
    use std::sync::Arc;
    use std::time::Duration;

    /// 协作式模型替身：只有被取消时才返回，用来制造一个可被中断收束的运行中 Turn。
    #[derive(Clone)]
    struct CooperativeCancelModel;

    impl Model for CooperativeCancelModel {
        async fn open_session(&self) -> Result<DynModelSession, ModelError> {
            Ok(DynModelSession::new(CooperativeCancelSession))
        }
    }

    struct CooperativeCancelSession;

    impl ModelSession for CooperativeCancelSession {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let cancellation = request.cancellation.clone();
            Ok(PreparedModelCall::new(async move {
                cancellation.cancelled().await;
                Err(cancelled_model_error())
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    /// 不返回的模型替身：忽略取消令牌，直到测试显式释放；模拟无法在有界时间内结束的现场。
    #[derive(Clone)]
    struct StuckModel {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl Model for StuckModel {
        async fn open_session(&self) -> Result<DynModelSession, ModelError> {
            Ok(DynModelSession::new(StuckSession(self.clone())))
        }
    }

    struct StuckSession(StuckModel);

    impl ModelSession for StuckSession {
        async fn prepare(
            &mut self,
            _request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let entered = self.0.entered.clone();
            let release = self.0.release.clone();
            Ok(PreparedModelCall::new(async move {
                entered.notify_one();
                // 故意不观察取消：即使 `interrupt_turn` 已经触发也不会结束。
                release.notified().await;
                Err(cancelled_model_error())
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    fn cancelled_model_error() -> ModelError {
        ModelError {
            details: None,
            kind: ModelFailureKind::Cancelled,
            usage: Default::default(),
            source: None,
        }
    }

    fn n3_input(id: &str) -> ThreadInput {
        ThreadInput {
            id: id.to_string(),
            payload: OpaquePayload::text("N3 archive prompt"),
            context: Vec::new(),
        }
    }

    fn drive_options() -> InputDriverOptions {
        InputDriverOptions {
            max_model_steps: pl_core::thread::ModelStepLimit::Unlimited,
        }
    }

    async fn wait_until_turn_running(thread: &ThreadHandle) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if thread
                    .snapshot()
                    .turns
                    .iter()
                    .any(|turn| turn.state == TurnState::Running)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the submitted Turn must start running");
    }

    async fn wait_until_idle(runtime: &StudioRuntime, thread_id: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if !runtime.thread_is_busy(thread_id).await.unwrap() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the session must settle to idle");
    }

    /// 运行中的会话可以直接结束并归档：中断当前 Turn 后收束为取消终态，而不是失败。
    #[tokio::test]
    async fn archiving_a_running_session_ends_the_turn_then_archives() {
        let (_home, _workspace, runtime, id) = runtime_with_thread_without_optional_tools().await;
        let thread = runtime.ensure_thread_owner(&id).await.unwrap();
        thread
            .replace_model(ModelFactory::new(CooperativeCancelModel))
            .await
            .unwrap();
        thread
            .submit_input_and_run(n3_input("n3-running-input"), drive_options())
            .await
            .unwrap();
        wait_until_turn_running(&thread).await;
        // 修复前该现场会被 `thread tree has an active turn or pending input` 拒绝。
        assert!(runtime.thread_is_busy(&id).await.unwrap());

        let archived = runtime.archive_thread(id.clone()).await.unwrap().unwrap();
        assert_eq!(archived.archived_root_id, id);
        assert!(archived.removed_thread_ids.contains(&id));

        let turns = thread.snapshot().turns;
        let last = turns.last().expect("the interrupted Turn is recorded");
        assert_eq!(last.state, TurnState::Cancelled);
        assert!(runtime.read_thread(&id).await.unwrap().archived);
        let visible = runtime
            .query_threads(&Default::default(), None, 20)
            .await
            .unwrap();
        assert!(
            !visible
                .state
                .value()
                .unwrap()
                .threads
                .iter()
                .any(|thread| thread.id == id)
        );
        runtime.shutdown().await;
    }

    /// 未消费输入在归档时被丢弃，既不被消费也不会注入新的 Turn。
    #[tokio::test]
    async fn archiving_discards_pending_input_without_starting_a_turn() {
        let (_home, _workspace, runtime, id) = runtime_with_thread_without_optional_tools().await;
        let thread = runtime.ensure_thread_owner(&id).await.unwrap();
        // 只受理不驱动：输入停留在 Pending，构成修复前会阻断归档的现场。
        thread
            .submit_input(n3_input("n3-pending-input"))
            .await
            .unwrap();
        assert!(runtime.thread_is_busy(&id).await.unwrap());

        runtime.archive_thread(id.clone()).await.unwrap().unwrap();

        let snapshot = thread.snapshot();
        let record = snapshot
            .inputs
            .iter()
            .find(|record| record.input.id == "n3-pending-input")
            .expect("the admitted input is recorded");
        assert_eq!(record.state, InputState::Discarded);
        assert!(
            snapshot.turns.is_empty(),
            "discarding a pending input must not start a Turn"
        );
        assert!(runtime.read_thread(&id).await.unwrap().archived);
        runtime.shutdown().await;
    }

    /// 归档语义未被破坏：运行中会话归档后仍可按既有语义恢复并重新激活。
    #[tokio::test]
    async fn archiving_a_running_session_preserves_identity_and_restores() {
        let (_home, _workspace, runtime, id) = runtime_with_thread_without_optional_tools().await;
        let original = runtime.read_thread(&id).await.unwrap();
        let thread = runtime.ensure_thread_owner(&id).await.unwrap();
        thread
            .replace_model(ModelFactory::new(CooperativeCancelModel))
            .await
            .unwrap();
        thread
            .submit_input_and_run(n3_input("n3-restore-input"), drive_options())
            .await
            .unwrap();
        wait_until_turn_running(&thread).await;

        runtime.archive_thread(id.clone()).await.unwrap().unwrap();
        assert!(runtime.read_thread(&id).await.unwrap().archived);

        let restored = runtime.restore_thread(id.clone()).await.unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.title, original.title);
        assert!(!runtime.read_thread(&id).await.unwrap().archived);
        let owner = runtime
            .ensure_thread_owner(&id)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "restore activation failed: {error:#}; issues: {:?}",
                    runtime.recovery_issues()
                )
            });
        assert_ne!(owner.snapshot().lifecycle, ThreadLifecycle::Closed);
        runtime.shutdown().await;
    }

    /// 无法在有界时间内结束时返回类型化失败且不归档：会话与现场都保留。
    #[tokio::test]
    async fn archiving_reports_typed_failure_and_preserves_the_session_when_work_cannot_end() {
        let (_home, _workspace, mut runtime, id) =
            runtime_with_thread_without_optional_tools().await;
        runtime.set_archive_settle_timeout(Duration::from_millis(200));
        let thread = runtime.ensure_thread_owner(&id).await.unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        thread
            .replace_model(ModelFactory::new(StuckModel {
                entered: entered.clone(),
                release: release.clone(),
            }))
            .await
            .unwrap();
        thread
            .submit_input_and_run(n3_input("n3-stuck-input"), drive_options())
            .await
            .unwrap();
        // 等到模型调用真正在执行，确保中断不能收束这个 Turn。
        tokio::time::timeout(Duration::from_secs(10), entered.notified())
            .await
            .expect("the stuck model call must be entered");

        let error = runtime.archive_thread(id.clone()).await.unwrap_err();
        let studio_error = error
            .downcast_ref::<pl_protocol::studio::StudioError>()
            .expect("archive failure must be a typed Studio error");
        assert_eq!(
            studio_error.code,
            pl_protocol::studio::StudioErrorCode::Busy
        );

        // 现场保留：未归档、owner 仍在、运行中的 Turn 仍是运行态。
        assert!(!runtime.read_thread(&id).await.unwrap().archived);
        assert!(runtime.threads.thread(&id).is_some());
        assert!(
            thread
                .snapshot()
                .turns
                .last()
                .is_some_and(|turn| turn.state == TurnState::Running)
        );

        // 释放替身以便本测试收尾（归档失败本身不触碰现场）。
        release.notify_one();
        wait_until_idle(&runtime, &id).await;
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn project_alias_reuses_identity_and_preserves_renamed_label() {
        let (_home, workspace, runtime, id) = runtime_with_thread_without_optional_tools().await;
        let thread = runtime.read_thread(&id).await.unwrap();
        let renamed = runtime
            .rename_project(&thread.project_id, "Readable project")
            .await
            .unwrap();
        let reopened = runtime
            .open_project(workspace.path().join("."))
            .await
            .unwrap();
        assert_eq!(renamed.id, reopened.id);
        assert_eq!(reopened.name, "Readable project");
        assert_eq!(runtime.list_projects().await.unwrap().len(), 1);
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
        let child_path = workspace.path().join("nested");
        std::fs::create_dir(&child_path).unwrap();
        let child = runtime.open_project(&child_path).await.unwrap();
        assert_ne!(child.id, reopened.id);
        assert_eq!(
            child.path,
            dunce::canonicalize(child_path).unwrap().to_string_lossy()
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn archived_session_restores_identity_and_can_activate_again() {
        let (_home, _workspace, runtime, id) = runtime_with_thread_without_optional_tools().await;
        let original = runtime.read_thread(&id).await.unwrap();
        let previous_owner = runtime.ensure_thread_owner(&id).await.unwrap();
        runtime.archive_thread(id.clone()).await.unwrap().unwrap();
        // Deterministic late completion from the retired tool-refresh owner.
        runtime.publish_tool_refresh_result(
            &id,
            &previous_owner,
            Some(&anyhow::anyhow!("retired catalog")),
        );
        assert!(
            runtime
                .recovery_issues()
                .iter()
                .all(|issue| issue.id != format!("tool-refresh:{id}"))
        );
        assert!(runtime.read_thread(&id).await.unwrap().archived);
        let restored = runtime.restore_thread(id.clone()).await.unwrap();
        assert_eq!(restored.id, id);
        assert_eq!(restored.title, original.title);
        assert!(!runtime.read_thread(&id).await.unwrap().archived);
        let owner = runtime
            .ensure_thread_owner(&id)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "restore activation failed: {error:#}; issues: {:?}",
                    runtime.recovery_issues()
                )
            });
        assert_ne!(
            owner.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        runtime.shutdown().await;
    }

    async fn runtime_with_thread() -> (tempfile::TempDir, tempfile::TempDir, StudioRuntime, String)
    {
        runtime_with_thread_config(|_| {}).await
    }

    async fn runtime_with_thread_without_optional_tools()
    -> (tempfile::TempDir, tempfile::TempDir, StudioRuntime, String) {
        runtime_with_thread_config(|config| {
            config.runtime.tool_capabilities.exec = false;
            config.runtime.tool_capabilities.workspace_files = false;
            config.skills.enabled = false;
            config.runtime.tool_capabilities.skills = false;
            config.runtime.tool_capabilities.mcp = false;
            config.runtime.tool_capabilities.lsp = false;
            config.runtime.tool_capabilities.ask_user = false;
            config.runtime.tool_capabilities.git = false;
        })
        .await
    }

    async fn runtime_with_thread_config(
        configure: impl FnOnce(&mut crate::config::StudioConfig),
    ) -> (tempfile::TempDir, tempfile::TempDir, StudioRuntime, String) {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        })
        .await
        .unwrap();
        let current = runtime.config_runtime.read().unwrap();
        runtime
            .config_runtime
            .update(current.revision, |config| {
                let mut next = config.clone();
                configure(&mut next);
                Ok(next)
            })
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
    async fn activation_evicts_idle_history_but_keeps_current_target_and_subscription() {
        let (_home, _workspace, runtime, selected_id) = runtime_with_thread().await;
        let project = runtime
            .read_owned_thread(&selected_id)
            .await
            .unwrap()
            .project_id;
        let mut subscription = runtime
            .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
                thread_id: selected_id.clone(),
            })
            .await
            .unwrap();
        let mut newest = selected_id.clone();
        for index in 0..6 {
            newest = runtime
                .create_thread(&project, &format!("idle {index}"))
                .await
                .unwrap()
                .id;
            assert!(
                runtime.threads.thread(&newest).is_some(),
                "current activation must survive capacity enforcement"
            );
        }
        assert!(runtime.threads.thread(&selected_id).is_some());
        assert!(subscription.recv().await.unwrap().is_some());
        assert!(runtime.threads.observed_threads().len() <= 6);
        drop(subscription);
        runtime.ensure_thread_owner(&newest).await.unwrap();
        assert!(runtime.threads.thread(&newest).is_some());
        assert!(runtime.threads.thread(&selected_id).is_none());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn unsaved_new_thread_reconnects_from_memory_and_flushes_latest_directory_after_retry() {
        use sea_orm::ConnectionTrait;
        let (_home, _workspace, runtime, existing_id) =
            runtime_with_thread_without_optional_tools().await;
        let project_id = runtime
            .read_owned_thread(&existing_id)
            .await
            .unwrap()
            .project_id;
        let repository = runtime.persistence_repository().await.unwrap();
        repository.flush().await.unwrap();
        // Catalog refresh is unrelated to this write-behind recovery proof and
        // would otherwise contend with the new owner's skill discovery.
        runtime.stop_tool_refresh().await.unwrap();
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
        assert!(repository.shutdown().await.is_err());
        assert!(
            runtime
                .store
                .read_thread(&thread.id)
                .await
                .unwrap()
                .is_none()
        );
        let owner = runtime.ensure_thread_owner(&thread.id).await.unwrap();
        assert_eq!(
            owner.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Open
        );
        for selected in [&existing_id, &thread.id] {
            let mut subscription = runtime
                .subscribe_thread(pl_protocol::ThreadSubscriptionRequest {
                    thread_id: selected.clone(),
                })
                .await
                .unwrap();
            let frame = subscription.recv().await.unwrap().unwrap();
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
        repository.retry_now();
        repository.flush().await.unwrap();
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
        runtime.shutdown().await;
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
        runtime.shutdown().await;
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
        runtime.shutdown().await;
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
        let archived = runtime.read_thread(&thread_id).await.unwrap();
        assert!(archived.archived);
        assert_eq!(archived.title, "Old title");
        let visible = runtime
            .query_threads(&Default::default(), None, 20)
            .await
            .unwrap();
        assert!(
            !visible
                .state
                .value()
                .unwrap()
                .threads
                .iter()
                .any(|thread| thread.id == thread_id)
        );
        runtime.shutdown().await;
    }

    async fn git_command(root: &std::path::Path, args: &[&str]) {
        let output = tokio::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    async fn commit_git_fixture(root: &std::path::Path) {
        git_command(root, &["init"]).await;
        git_command(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        )
        .await;
    }

    fn worktree_request(text: &str) -> pl_protocol::studio::CreateThreadRequest {
        pl_protocol::studio::CreateThreadRequest {
            title: None,
            input: pl_protocol::studio::StudioPromptInput {
                input_id: "request-1".into(),
                text: text.into(),
                attachment_draft_ids: Vec::new(),
            },
            mode: pl_protocol::ThreadModeId::simple().label().to_string(),
            workspace_mode: pl_protocol::ThreadWorkspaceMode::Worktree,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn root_session_worktree_is_created_bound_and_rebound_from_its_durable_lease() {
        use crate::studio::agent_host::worktree_lease::{
            WorktreeLeaseOwnerKind, WorktreeLeaseState,
        };
        use pl_protocol::{ThreadModeId, ThreadWorkspaceMode};
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let options = || StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        };
        let runtime = StudioRuntime::with_options(options()).await.unwrap();
        runtime
            .config_runtime
            .update(runtime.config_runtime.read().unwrap().revision, |config| {
                let mut next = config.clone();
                next.runtime.tool_capabilities.exec = false;
                next.runtime.tool_capabilities.workspace_files = false;
                next.skills.enabled = false;
                next.runtime.tool_capabilities.skills = false;
                next.runtime.tool_capabilities.mcp = false;
                next.runtime.tool_capabilities.lsp = false;
                next.runtime.tool_capabilities.ask_user = false;
                Ok(next)
            })
            .unwrap();
        runtime.start_runtime().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        commit_git_fixture(workspace.path()).await;

        let thread_id = crate::studio::ids::new_id("thread");
        let mut lease =
            crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                &runtime.agent_facility.worktrees,
                &runtime.ssh_manager,
                &project,
                &thread_id,
            )
            .await
            .unwrap();
        assert_eq!(lease.state, WorktreeLeaseState::Prepared);
        assert_eq!(lease.owner_kind, WorktreeLeaseOwnerKind::Session);
        assert_eq!(lease.owner_thread_id, thread_id);
        assert_eq!(lease.root_thread_id, thread_id);
        assert_eq!(lease.branch, format!("pure-session-{thread_id}"));
        // 平台无关地校验布局：与 `WorktreeManager` 的分配结果逐段比较，而不是比较字符串分隔符。
        assert_eq!(
            std::path::PathBuf::from(&lease.path),
            crate::agent::worktree::WorktreeManager::allocate_path(
                std::path::Path::new(&lease.repository_root),
                &thread_id,
                &crate::agent::worktree::WorktreeOwnership::Session {
                    thread_id: thread_id.clone(),
                },
            ),
        );
        assert!(std::path::Path::new(&lease.path).exists());
        assert!(lease.validate_identity().is_ok());
        lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();

        let (delta, thread) = DirectoryDelta::register_root_thread(
            thread_id.clone(),
            &project.id,
            "worktree session",
            ThreadModeId::simple(),
            ThreadWorkspaceMode::Worktree,
        );
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        assert_eq!(thread.workspace_mode, ThreadWorkspaceMode::Worktree);

        let owner = runtime.ensure_thread_owner(&thread.id).await.unwrap();
        assert_ne!(
            owner.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        let bound = std::path::PathBuf::from(&lease.path);
        assert_eq!(
            runtime
                .thread_factory
                .session_workspace_root_for_test(&thread.id),
            Some(bound.clone()),
            "worktree 会话的工作区根必须是其自身 worktree"
        );
        assert_ne!(
            runtime
                .thread_factory
                .session_workspace_root_for_test(&thread.id),
            Some(dunce::simplified(workspace.path()).to_path_buf()),
            "worktree 会话不得绑定到 Project 主目录"
        );

        let writer = runtime.persistence_repository().await.unwrap();
        writer.flush().await.unwrap();
        runtime.shutdown_runtime().await.unwrap();
        drop(runtime);

        // 冷启动：只按 durable lease 恢复绑定。
        let reopened = StudioRuntime::with_options(options()).await.unwrap();
        reopened.start_runtime().await.unwrap();
        let restored = reopened.ensure_thread_owner(&thread.id).await.unwrap();
        assert_ne!(
            restored.snapshot().lifecycle,
            pl_core::thread::ThreadLifecycle::Closed
        );
        assert_eq!(
            reopened
                .thread_factory
                .session_workspace_root_for_test(&thread.id),
            Some(bound)
        );
        assert_eq!(
            reopened.agent_facility.worktrees.get(&thread_id).unwrap(),
            lease
        );
        reopened.shutdown_runtime().await.unwrap();
    }

    async fn worktree_runtime(
        home: &tempfile::TempDir,
        workspace: &tempfile::TempDir,
    ) -> (StudioRuntime, crate::studio::ProjectRecord) {
        let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime
            .config_runtime
            .update(runtime.config_runtime.read().unwrap().revision, |config| {
                let mut next = config.clone();
                next.runtime.tool_capabilities.exec = false;
                next.runtime.tool_capabilities.workspace_files = false;
                next.skills.enabled = false;
                next.runtime.tool_capabilities.skills = false;
                next.runtime.tool_capabilities.mcp = false;
                next.runtime.tool_capabilities.lsp = false;
                next.runtime.tool_capabilities.ask_user = false;
                Ok(next)
            })
            .unwrap();
        runtime.start_runtime().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        commit_git_fixture(workspace.path()).await;
        (runtime, project)
    }

    /// 只有需要人工处置的资源才能进入 worktree 清理入口；在用 lease 既不上报也不可清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_worktree_recovery_only_targets_released_resources() {
        use crate::studio::agent_host::worktree_lease::{
            WorktreeLease, WorktreeLeaseOwnerKind, WorktreeLeaseState,
        };
        use pl_protocol::{ThreadModeId, ThreadWorkspaceMode};
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        let register = |thread_id: &str| {
            DirectoryDelta::register_root_thread(
                thread_id.to_string(),
                &project.id,
                "worktree session",
                ThreadModeId::simple(),
                ThreadWorkspaceMode::Worktree,
            )
        };
        let issues = async || {
            let mut issues = Vec::new();
            runtime
                .append_worktree_recovery_issues(&mut issues)
                .await
                .unwrap();
            issues
        };

        // (1) 健康 active 会话 lease：Thread 已注册 → 不发布、不可清理。
        let live = crate::studio::ids::new_id("thread");
        let mut lease =
            crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                &runtime.agent_facility.worktrees,
                &runtime.ssh_manager,
                &project,
                &live,
            )
            .await
            .unwrap();
        lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        let (delta, _) = register(&live);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let live_path = std::path::PathBuf::from(&lease.path);
        assert!(live_path.exists());
        assert!(
            issues()
                .await
                .iter()
                .all(|issue| issue.id != format!("worktree-lease-{live}")),
            "a healthy active lease must never enter the cleanup entry"
        );
        let error = runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &live,
                lease.revision,
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("still owned by a live Thread"),
            "{error}"
        );
        assert_eq!(
            runtime.agent_facility.worktrees.get(&live).unwrap().state,
            WorktreeLeaseState::Active
        );
        assert!(
            live_path.exists(),
            "a rejected cleanup must not touch the live worktree"
        );

        // (2) preserved 会话 lease：发布 preview 且可显式清理。
        lease.transition(WorktreeLeaseState::Preserved);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        let published = issues().await;
        let issue = published
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{live}"))
            .unwrap_or_else(|| panic!("preserved lease must publish recovery: {published:?}"));
        assert_eq!(issue.thread_id.as_deref(), Some(live.as_str()));
        let preview = issue.worktree.as_ref().expect("worktree preview");
        assert_eq!(
            preview.owner_kind,
            crate::StudioRecoveryWorktreeOwner::Session
        );
        assert_eq!(preview.owner_thread_id, live);
        assert_eq!(preview.lease_revision, lease.revision);
        assert_eq!(preview.branch, format!("pure-session-{live}"));

        // 顺带补覆盖缺口：陈旧 revision 必须被服务端 CAS 拒绝，且不触碰物理资源。
        let stale = runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &live,
                lease.revision.saturating_sub(1),
            )
            .await
            .unwrap_err();
        assert!(stale.to_string().contains("revision conflict"), "{stale}");
        assert!(live_path.exists());

        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &live,
                lease.revision,
            )
            .await
            .unwrap();
        assert!(!live_path.exists());
        assert_eq!(
            runtime.agent_facility.worktrees.get(&live).unwrap().state,
            WorktreeLeaseState::Cleaned
        );

        // (3) 孤儿 lease（无注册 Thread）：保留诊断且可清理。
        let orphan = crate::studio::ids::new_id("thread");
        let mut lease =
            crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                &runtime.agent_facility.worktrees,
                &runtime.ssh_manager,
                &project,
                &orphan,
            )
            .await
            .unwrap();
        lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        let orphan_path = std::path::PathBuf::from(&lease.path);
        let published = issues().await;
        let issue = published
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{orphan}"))
            .unwrap_or_else(|| panic!("orphan lease must publish recovery: {published:?}"));
        assert!(
            issue.message.contains("no published Thread remains"),
            "{}",
            issue.message
        );
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &orphan,
                lease.revision,
            )
            .await
            .unwrap();
        assert!(!orphan_path.exists());

        // (4) 身份不匹配与物理资源缺失的现场仍然发布。
        let repository_root = dunce::simplified(workspace.path())
            .to_string_lossy()
            .into_owned();
        let mismatched = crate::studio::ids::new_id("thread");
        let (delta, _) = register(&mismatched);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let mut bad = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Session,
            owner_thread_id: mismatched.clone(),
            root_thread_id: mismatched.clone(),
            project_id: project.id.clone(),
            ssh_alias: None,
            repository_root: repository_root.clone(),
            path: format!("{repository_root}/.anywork/worktrees/{mismatched}/session"),
            branch: format!("pure-agent-{mismatched}"),
            base_commit: "base".into(),
        };
        runtime
            .agent_facility
            .worktrees
            .record(bad.clone())
            .unwrap();
        bad.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(bad.clone())
            .unwrap();
        let published = issues().await;
        let issue = published
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{mismatched}"))
            .unwrap_or_else(|| panic!("identity mismatch must publish: {published:?}"));
        assert!(
            issue.message.contains("mismatched Pure-owned branch"),
            "{}",
            issue.message
        );
        assert!(
            runtime
                .cleanup_preserved_worktree(
                    crate::StudioRecoveryWorktreeOwner::Session,
                    &mismatched,
                    bad.revision,
                )
                .await
                .is_err()
        );

        let missing = crate::studio::ids::new_id("thread");
        let (delta, _) = register(&missing);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let mut gone = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Session,
            owner_thread_id: missing.clone(),
            root_thread_id: missing.clone(),
            project_id: project.id.clone(),
            ssh_alias: None,
            repository_root,
            path: format!(
                "{}/.anywork/worktrees/{missing}/session",
                dunce::simplified(workspace.path()).to_string_lossy()
            ),
            branch: format!("pure-session-{missing}"),
            base_commit: "base".into(),
        };
        runtime
            .agent_facility
            .worktrees
            .record(gone.clone())
            .unwrap();
        gone.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(gone.clone())
            .unwrap();
        let published = issues().await;
        let issue = published
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{missing}"))
            .unwrap_or_else(|| panic!("missing physical resource must publish: {published:?}"));
        assert!(
            issue.message.contains("worktree preview failed"),
            "{}",
            issue.message
        );
        runtime.shutdown().await;
    }

    /// 未启动会话的补偿：目录事实已发布但激活失败时不得留下已发布（可激活）的会话。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn failed_activation_archives_the_session_and_preserves_its_worktree() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        // 封闭 Thread 装配：新 owner 的激活必然失败，且发生在目录事实发布之后。
        assert!(runtime.threads.close_all().await.is_empty());

        let error = runtime
            .create_thread_command(project.id.clone(), worktree_request("hello"))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("Thread assembly is closed"),
            "{error:#}"
        );

        runtime
            .persistence_repository()
            .await
            .unwrap()
            .flush()
            .await
            .unwrap();
        assert!(
            runtime
                .store
                .list_root_threads(&project.id)
                .await
                .unwrap()
                .is_empty(),
            "a failed activation must not leave a published Thread"
        );
        let all = runtime
            .store
            .list_threads_for_project(&project.id)
            .await
            .unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].visibility, crate::studio::ThreadVisibility::Archived);
        assert_eq!(
            all[0].workspace_mode,
            pl_protocol::ThreadWorkspaceMode::Worktree
        );
        let leases = runtime.agent_facility.worktrees.snapshot();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].state, WorktreeLeaseState::Preserved);
        assert!(
            std::path::Path::new(&leases[0].path).exists(),
            "compensation must keep the physical session worktree for explicit cleanup"
        );

        // F2：失败当场收束为 `preserved` 后必须立即可见且可清理（不依赖下次启动审计）。
        let owner_thread_id = leases[0].owner_thread_id.clone();
        let lease_revision = leases[0].revision;
        let issues = runtime.recovery_issues();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{owner_thread_id}"))
            .unwrap_or_else(|| panic!("compensated session must publish recovery: {issues:?}"));
        let preview = issue
            .worktree
            .as_ref()
            .expect("compensated session worktree must publish a preview");
        assert_eq!(
            preview.owner_kind,
            crate::StudioRecoveryWorktreeOwner::Session
        );
        assert_eq!(preview.owner_thread_id, owner_thread_id);
        assert_eq!(preview.state, "preserved");
        assert_eq!(preview.branch, format!("pure-session-{owner_thread_id}"));
        assert_eq!(preview.lease_revision, lease_revision);
        assert_eq!(issue.thread_id.as_deref(), Some(owner_thread_id.as_str()));
        assert!(issue.message.contains("is preserved"), "{}", issue.message);
        assert_eq!(
            issues
                .iter()
                .filter(|issue| issue.id == format!("worktree-lease-{owner_thread_id}"))
                .count(),
            1,
            "the earlier activation diagnostic must be replaced, not duplicated"
        );

        let worktree_path = std::path::PathBuf::from(&leases[0].path);
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &owner_thread_id,
                lease_revision,
            )
            .await
            .unwrap();
        assert!(!worktree_path.exists());
        assert_eq!(
            runtime
                .agent_facility
                .worktrees
                .get(&owner_thread_id)
                .unwrap()
                .state,
            WorktreeLeaseState::Cleaned
        );
        assert!(
            runtime
                .recovery_issues()
                .iter()
                .all(|issue| issue.id != format!("worktree-lease-{owner_thread_id}"))
        );
        runtime.shutdown().await;
    }

    /// F3/R1：创建窗口内由进程内标记保护；收束失败后必须立即发布并可显式清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn creating_session_worktree_is_neither_published_nor_cleanable_until_it_converges() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        let thread_id = crate::studio::ids::new_id("thread");

        // R1：创建中标记早于 `prepared` 记录设置，此时 owner Thread 行尚未发布。
        runtime.agent_facility.worktrees.mark_creating(&thread_id);
        let mut lease =
            crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
                &runtime.agent_facility.worktrees,
                &runtime.ssh_manager,
                &project,
                &thread_id,
            )
            .await
            .unwrap();
        assert_eq!(lease.state, WorktreeLeaseState::Prepared);
        let worktree_path = std::path::PathBuf::from(&lease.path);
        assert!(worktree_path.exists());

        let mut issues = Vec::new();
        runtime
            .append_worktree_recovery_issues(&mut issues)
            .await
            .unwrap();
        assert!(
            issues
                .iter()
                .all(|issue| issue.id != format!("worktree-lease-{thread_id}")),
            "an in-creation lease must not enter the cleanup entry: {issues:?}"
        );
        let error = runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &thread_id,
                lease.revision,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("still being created"), "{error}");
        assert!(worktree_path.exists());

        // 失败收束：settle 为 preserved + 清除标记 + 立即发布（F3）。
        lease.transition(WorktreeLeaseState::Preserved);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        assert!(
            runtime.agent_facility.worktrees.is_creating(&thread_id),
            "settle alone must not clear the creating mark before publication"
        );
        runtime
            .settle_session_worktree_failure(
                &thread_id,
                Some("the session worktree could not be created"),
            )
            .await;
        assert!(!runtime.agent_facility.worktrees.is_creating(&thread_id));
        let issues = runtime.recovery_issues();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{thread_id}"))
            .unwrap_or_else(|| panic!("a settled creation must publish recovery: {issues:?}"));
        let preview = issue
            .worktree
            .as_ref()
            .expect("a settled creation must publish a preview");
        assert_eq!(
            preview.owner_kind,
            crate::StudioRecoveryWorktreeOwner::Session
        );
        assert_eq!(preview.state, "preserved");
        assert_eq!(preview.branch, format!("pure-session-{thread_id}"));
        assert_eq!(preview.lease_revision, lease.revision);
        assert!(
            issue
                .message
                .contains("the session worktree could not be created"),
            "{}",
            issue.message
        );
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &thread_id,
                lease.revision,
            )
            .await
            .unwrap();
        assert!(!worktree_path.exists());
        runtime.shutdown().await;
    }

    /// child worktree 的创建窗口同样受进程内标记保护，收束后恢复常规判定。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn creating_child_worktree_is_neither_published_nor_cleanable_until_it_converges() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        use pl_protocol::AgentWorkspaceMode;
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        let root_thread_id = crate::studio::ids::new_id("thread");
        let child_id = crate::studio::ids::new_id("thread");

        // 与 `child_resources.rs` 相同的创建窗口：标记先于 `prepared` 记录设置，child 产品行
        // 尚未提交。
        let guard = runtime.agent_facility.worktrees.creation_guard(&child_id);
        let (_, worktree) = crate::studio::agent_host::workspace_preparation::prepare_workspace(
            &runtime.agent_facility.worktrees,
            &runtime.ssh_manager,
            crate::studio::agent_host::workspace_preparation::WorkspacePreparation {
                project: &project,
                root_thread_id: &root_thread_id,
                child_id: &child_id,
                mode: AgentWorkspaceMode::Worktree,
                writable_paths: None,
                session_root: project.path.clone().into(),
            },
        )
        .await
        .unwrap();
        let mut lease = worktree.unwrap().lease;
        assert_eq!(lease.state, WorktreeLeaseState::Prepared);
        let child_path = std::path::PathBuf::from(&lease.path);
        assert!(child_path.exists());

        let mut issues = Vec::new();
        runtime
            .append_worktree_recovery_issues(&mut issues)
            .await
            .unwrap();
        assert!(
            issues
                .iter()
                .all(|issue| issue.id != format!("worktree-lease-{child_id}")),
            "an in-creation child lease must not enter the cleanup entry: {issues:?}"
        );
        let error = runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Child,
                &child_id,
                lease.revision,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("still being created"), "{error}");
        assert!(child_path.exists());

        // 创建收束：记录 `active` 后离开创建窗口，豁免随之消失。
        lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        drop(guard);
        assert!(!runtime.agent_facility.worktrees.is_creating(&child_id));
        let mut issues = Vec::new();
        runtime
            .append_worktree_recovery_issues(&mut issues)
            .await
            .unwrap();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{child_id}"))
            .unwrap_or_else(|| {
                panic!("a converged child lease without its Thread row must publish: {issues:?}")
            });
        assert_eq!(
            issue.worktree.as_ref().unwrap().owner_kind,
            crate::StudioRecoveryWorktreeOwner::Child
        );
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Child,
                &child_id,
                lease.revision,
            )
            .await
            .unwrap();
        assert!(!child_path.exists());
        runtime.shutdown().await;
    }

    /// 清理失败回落后卡片必须携带回落后的 canonical revision，解除阻塞后可直接重试成功。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cleanup_failure_refreshes_the_recovery_card_revision() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        use pl_protocol::AgentWorkspaceMode;
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        let root_thread_id = crate::studio::ids::new_id("thread");
        let child_id = crate::studio::ids::new_id("thread");
        let (_, worktree) = crate::studio::agent_host::workspace_preparation::prepare_workspace(
            &runtime.agent_facility.worktrees,
            &runtime.ssh_manager,
            crate::studio::agent_host::workspace_preparation::WorkspacePreparation {
                project: &project,
                root_thread_id: &root_thread_id,
                child_id: &child_id,
                mode: AgentWorkspaceMode::Worktree,
                writable_paths: None,
                session_root: project.path.clone().into(),
            },
        )
        .await
        .unwrap();
        let mut lease = worktree.unwrap().lease;
        lease.transition(WorktreeLeaseState::Preserved);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        let lease_revision = lease.revision;
        let card_revision = |runtime: &StudioRuntime| {
            runtime
                .recovery_issues()
                .into_iter()
                .find(|issue| issue.id == format!("worktree-lease-{child_id}"))
                .and_then(|issue| issue.worktree.map(|preview| preview.lease_revision))
        };

        // 让 discard 失败：锁住 worktree 后清理会被 Git 拒绝并回落到 `preserved`。
        git_command(workspace.path(), &["worktree", "lock", &lease.path]).await;
        let error = runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Child,
                &child_id,
                lease_revision,
            )
            .await
            .unwrap_err();
        let rolled_back = runtime.agent_facility.worktrees.get(&child_id).unwrap();
        assert_eq!(rolled_back.state, WorktreeLeaseState::Preserved);
        assert!(rolled_back.revision > lease_revision);
        assert!(
            error.to_string().contains(&lease.path) || error.to_string().contains("cleanup"),
            "{error}"
        );
        assert_eq!(
            card_revision(&runtime),
            Some(rolled_back.revision),
            "the recovery card must carry the rolled-back revision"
        );
        assert!(
            runtime
                .cleanup_preserved_worktree(
                    crate::StudioRecoveryWorktreeOwner::Child,
                    &child_id,
                    lease_revision,
                )
                .await
                .unwrap_err()
                .to_string()
                .contains("revision conflict"),
            "the stale card revision must be rejected"
        );

        // 解除阻塞后用卡片 revision 重试成功。
        git_command(workspace.path(), &["worktree", "unlock", &lease.path]).await;
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Child,
                &child_id,
                rolled_back.revision,
            )
            .await
            .unwrap();
        assert!(!std::path::Path::new(&lease.path).exists());
        assert!(card_revision(&runtime).is_none());
        runtime.shutdown().await;
    }

    /// R2：owner 存在 Child lease 时激活失败的发布必须带该 lease 的完整 preview。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn activation_failure_publishes_the_child_lease_preview_when_no_session_lease_exists() {
        use crate::studio::agent_host::worktree_lease::{
            WorktreeLease, WorktreeLeaseOwnerKind, WorktreeLeaseState,
        };
        use pl_protocol::{ThreadModeId, ThreadWorkspaceMode};
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        let thread_id = crate::studio::ids::new_id("thread");
        let (delta, thread) = DirectoryDelta::register_root_thread(
            thread_id.clone(),
            &project.id,
            "worktree session",
            ThreadModeId::simple(),
            ThreadWorkspaceMode::Worktree,
        );
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();

        // 同一 owner id 上只有 Child 归属 lease（模拟 worktree child 激活失败的现场）。
        let repository_root = dunce::simplified(workspace.path()).to_path_buf();
        let ownership = crate::agent::worktree::WorktreeOwnership::Child {
            child_id: thread_id.clone(),
        };
        let manager = crate::agent::worktree::WorktreeManager::new(
            repository_root.clone(),
            std::sync::Arc::new(crate::agent::worktree::LocalWorktreeBackend::default()),
        );
        let handle = manager
            .create(crate::agent::worktree::WorktreeCreateSpec {
                repo_root: repository_root.clone(),
                root_thread_id: thread_id.clone(),
                ownership,
                base_commit: manager.resolve_head(&repository_root).await.unwrap(),
            })
            .await
            .unwrap();
        let mut child_lease = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Child,
            owner_thread_id: thread_id.clone(),
            root_thread_id: thread_id.clone(),
            project_id: project.id.clone(),
            ssh_alias: None,
            repository_root: repository_root.to_string_lossy().into_owned(),
            path: handle.path.to_string_lossy().into_owned(),
            branch: handle.branch.clone(),
            base_commit: handle.base_commit.clone(),
        };
        runtime
            .agent_facility
            .worktrees
            .record(child_lease.clone())
            .unwrap();
        child_lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(child_lease.clone())
            .unwrap();

        // 没有 Session lease → 激活类型化失败，但发布必须带 Child lease 的 preview。
        let error = runtime.ensure_thread_owner(&thread.id).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("workspace is unavailable"),
            "{error:#}"
        );
        let issues = runtime.recovery_issues();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{thread_id}"))
            .unwrap_or_else(|| panic!("activation failure must publish recovery: {issues:?}"));
        let preview = issue
            .worktree
            .as_ref()
            .expect("a durable lease must always publish a preview");
        assert_eq!(
            preview.owner_kind,
            crate::StudioRecoveryWorktreeOwner::Child
        );
        assert_eq!(preview.lease_revision, child_lease.revision);
        assert_eq!(preview.branch, handle.branch);
        assert_eq!(preview.path, handle.path.to_string_lossy());
        assert!(
            issue.message.contains("cannot use its saved workspace"),
            "{}",
            issue.message
        );
        runtime.shutdown().await;
    }

    /// 首条 prompt 提交失败的收束同样必须立即发布带 preview 且可清理的 Recovery。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rejected_first_prompt_publishes_a_cleanable_session_worktree() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        use pl_protocol::studio::{
            AdmitAttachmentDraftsRequest, StudioAttachmentAdmissionContext,
            StudioAttachmentDraftSource,
        };
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let (runtime, project) = worktree_runtime(&home, &workspace).await;
        // 准入一个真实图片草稿（默认 Planner 路由支持 image/png）；随后把 attachment
        // objects 目录替换成普通文件，使升级阶段（激活之后的 `promote_attachment_drafts`）
        // 确定性失败。
        const ONE_PIXEL_PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x56, 0xc7,
            0x2f, 0x0d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let source = workspace.path().join("pixel.png");
        tokio::fs::write(&source, ONE_PIXEL_PNG).await.unwrap();
        let admitted = runtime
            .admit_attachment_drafts(AdmitAttachmentDraftsRequest {
                context: StudioAttachmentAdmissionContext::NewThread {
                    mode: pl_protocol::ThreadModeId::simple().label().to_string(),
                },
                sources: vec![StudioAttachmentDraftSource::LocalFile {
                    path: source.to_string_lossy().into_owned(),
                }],
            })
            .await
            .unwrap();
        let draft_id = admitted.drafts.first().unwrap().draft_id.clone();
        let objects = runtime.store.attachments_dir().join("objects");
        tokio::fs::write(&objects, b"not a directory")
            .await
            .unwrap();

        let error = runtime
            .create_thread_command(
                project.id.clone(),
                pl_protocol::studio::CreateThreadRequest {
                    title: None,
                    input: pl_protocol::studio::StudioPromptInput {
                        input_id: "request-1".into(),
                        text: "hello".into(),
                        attachment_draft_ids: vec![draft_id],
                    },
                    mode: pl_protocol::ThreadModeId::simple().label().to_string(),
                    workspace_mode: pl_protocol::ThreadWorkspaceMode::Worktree,
                },
            )
            .await
            .unwrap_err();
        // 不复述具体的 OS 错误文本（Unix 为 `Not a directory`，Windows 目录语义不同）；
        // “失败来自首条 prompt 受理”由后续断言共同证明：lease 已收束为 preserved、
        // 物理 worktree 仍存在，且 Recovery 条目的原因文本是首条 prompt 被拒。
        assert!(!format!("{error:#}").trim().is_empty());

        runtime
            .persistence_repository()
            .await
            .unwrap()
            .flush()
            .await
            .unwrap();
        assert!(
            runtime
                .store
                .list_root_threads(&project.id)
                .await
                .unwrap()
                .is_empty(),
            "a rejected first prompt must not leave a published Thread"
        );
        let leases = runtime.agent_facility.worktrees.snapshot();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].state, WorktreeLeaseState::Preserved);
        let owner_thread_id = leases[0].owner_thread_id.clone();
        let worktree_path = std::path::PathBuf::from(&leases[0].path);
        assert!(worktree_path.exists());

        let issues = runtime.recovery_issues();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{owner_thread_id}"))
            .unwrap_or_else(|| panic!("a rejected first prompt must publish recovery: {issues:?}"));
        assert!(issue.worktree.is_some(), "preview must be filled");
        assert!(
            issue
                .message
                .contains("the first prompt of the new session was rejected"),
            "{}",
            issue.message
        );
        assert!(issue.message.contains("is preserved"), "{}", issue.message);
        runtime
            .cleanup_preserved_worktree(
                crate::StudioRecoveryWorktreeOwner::Session,
                &owner_thread_id,
                leases[0].revision,
            )
            .await
            .unwrap();
        assert!(!worktree_path.exists());
        assert_eq!(
            runtime
                .agent_facility
                .worktrees
                .get(&owner_thread_id)
                .unwrap()
                .state,
            WorktreeLeaseState::Cleaned
        );
        runtime.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worktree_creation_failures_publish_no_thread_and_leave_no_artifact() {
        use pl_protocol::ThreadWorkspaceMode;
        let (_home, workspace, runtime, existing_id) =
            runtime_with_thread_without_optional_tools().await;
        let project_id = runtime
            .read_owned_thread(&existing_id)
            .await
            .unwrap()
            .project_id;
        let before = runtime
            .store
            .list_root_threads(&project_id)
            .await
            .unwrap()
            .len();

        // 非 Git 项目：前置校验即失败，未记录 lease，也没有任何物理残留。
        let error = runtime
            .create_thread_command(project_id.clone(), worktree_request("hello"))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("studio_workspace"),
            "非 Git 项目必须类型化拒绝：{error:#}"
        );
        assert_eq!(
            runtime
                .store
                .list_root_threads(&project_id)
                .await
                .unwrap()
                .len(),
            before,
            "失败命令不得发布 Thread"
        );
        assert!(!workspace.path().join(".anywork").exists());

        // Git 仓库但没有 HEAD：同样类型化失败且无残留。
        git_command(workspace.path(), &["init"]).await;
        let error = runtime
            .create_thread_command(project_id.clone(), worktree_request("hello"))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("studio_workspace"),
            "无 HEAD 的仓库必须类型化拒绝：{error:#}"
        );
        assert_eq!(
            runtime
                .store
                .list_root_threads(&project_id)
                .await
                .unwrap()
                .len(),
            before
        );
        assert!(!workspace.path().join(".anywork").exists());

        // 远程项目不再被 local-only 硬拒绝：解析仓库根经远端 backend，未配置的 alias
        // 在创建任何资源之前返回类型化失败。
        let remote = crate::studio::ProjectRecord {
            id: "project-remote".into(),
            name: "remote".into(),
            path: "/srv/repo".into(),
            ssh_alias: Some("ssh-1".into()),
            updated_at: 0,
        };
        let error = crate::studio::agent_host::workspace_preparation::create_root_session_worktree(
            &runtime.agent_facility.worktrees,
            &runtime.ssh_manager,
            &remote,
            "thread-remote",
        )
        .await
        .unwrap_err();
        let error_text = format!("{error:#}");
        assert!(
            !error_text.contains("only available for local projects"),
            "远程项目不得再被 local-only 拒绝：{error_text}"
        );
        assert!(
            error_text.contains("studio_workspace"),
            "远端仓库解析失败必须是类型化的会话工作区错误：{error_text}"
        );
        // 未配置的 alias 在创建任何资源之前就按别名解析失败，不进入真实网络等待；
        // 该消息同时证明远程创建路径确实经远端 backend 解析仓库根。
        assert!(
            error_text.contains("unknown SSH server 'ssh-1'"),
            "未配置的别名必须快速类型化失败：{error_text}"
        );
        assert!(
            runtime
                .agent_facility
                .worktrees
                .snapshot()
                .iter()
                .all(|lease| lease.owner_thread_id != "thread-remote"),
            "远程拒绝不得留下 lease"
        );
        assert_eq!(
            runtime
                .store
                .list_root_threads(&project_id)
                .await
                .unwrap()
                .len(),
            before,
            "远程失败不得发布 Thread"
        );

        // local 模式不受影响。
        let created = runtime
            .create_thread_command(
                project_id.clone(),
                pl_protocol::studio::CreateThreadRequest {
                    title: None,
                    input: pl_protocol::studio::StudioPromptInput {
                        input_id: "request-2".into(),
                        text: "local".into(),
                        attachment_draft_ids: Vec::new(),
                    },
                    mode: pl_protocol::ThreadModeId::simple().label().to_string(),
                    workspace_mode: ThreadWorkspaceMode::Local,
                },
            )
            .await;
        if let Ok(created) = created {
            assert_eq!(created.thread.workspace_mode, ThreadWorkspaceMode::Local);
        }
        runtime.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worktree_session_without_a_usable_lease_fails_explicitly_with_recovery() {
        use crate::studio::agent_host::worktree_lease::{
            WorktreeLease, WorktreeLeaseOwnerKind, WorktreeLeaseState,
        };
        use pl_protocol::{ThreadModeId, ThreadWorkspaceMode};
        let (_home, workspace, runtime, _existing) =
            runtime_with_thread_without_optional_tools().await;
        let project_id = runtime
            .read_owned_thread(&_existing)
            .await
            .unwrap()
            .project_id;
        let repository_root = dunce::simplified(workspace.path())
            .to_string_lossy()
            .into_owned();

        let register = |thread_id: &str| {
            DirectoryDelta::register_root_thread(
                thread_id.to_string(),
                &project_id,
                "worktree session",
                ThreadModeId::simple(),
                ThreadWorkspaceMode::Worktree,
            )
        };

        // (a) 完全没有 lease。
        let missing = crate::studio::ids::new_id("thread");
        let (delta, _) = register(&missing);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let error = runtime.ensure_thread_owner(&missing).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("workspace is unavailable"),
            "{error:#}"
        );
        assert!(
            runtime
                .thread_factory
                .session_workspace_root_for_test(&missing)
                .is_none(),
            "缺失 lease 时不得回落到 Project 主目录"
        );
        let issues = runtime.recovery_issues();
        let issue = issues
            .iter()
            .find(|issue| issue.id == format!("worktree-lease-{missing}"))
            .unwrap_or_else(|| panic!("missing lease must publish Recovery: {issues:?}"));
        assert_eq!(issue.thread_id.as_deref(), Some(missing.as_str()));

        // (b) lease 身份不符：分支不是 Pure-owned 会话分支。
        let mismatched = crate::studio::ids::new_id("thread");
        let (delta, _) = register(&mismatched);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let mut lease = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Session,
            owner_thread_id: mismatched.clone(),
            root_thread_id: mismatched.clone(),
            project_id: project_id.clone(),
            ssh_alias: None,
            repository_root: repository_root.clone(),
            path: format!("{repository_root}/.anywork/worktrees/{mismatched}/session"),
            branch: format!("pure-agent-{mismatched}"),
            base_commit: "base".into(),
        };
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        lease.transition(WorktreeLeaseState::Active);
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        let error = runtime.ensure_thread_owner(&mismatched).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("workspace is unavailable"),
            "{error:#}"
        );
        assert!(
            runtime
                .agent_facility
                .worktrees
                .get(&mismatched)
                .unwrap()
                .validate_identity()
                .is_err()
        );
        assert!(
            runtime
                .thread_factory
                .session_workspace_root_for_test(&mismatched)
                .is_none()
        );

        // (c) lease 已被清理。
        let cleaned = crate::studio::ids::new_id("thread");
        let (delta, _) = register(&cleaned);
        runtime
            .agent_facility
            .product_events
            .commit_directory(delta)
            .await
            .unwrap();
        let mut lease = WorktreeLease {
            revision: 1,
            state: WorktreeLeaseState::Prepared,
            owner_kind: WorktreeLeaseOwnerKind::Session,
            owner_thread_id: cleaned.clone(),
            root_thread_id: cleaned.clone(),
            project_id: project_id.clone(),
            ssh_alias: None,
            repository_root,
            path: format!(
                "{}/.anywork/worktrees/{cleaned}/session",
                dunce::simplified(workspace.path()).to_string_lossy()
            ),
            branch: format!("pure-session-{cleaned}"),
            base_commit: "base".into(),
        };
        runtime
            .agent_facility
            .worktrees
            .record(lease.clone())
            .unwrap();
        for state in [
            WorktreeLeaseState::Active,
            WorktreeLeaseState::Preserved,
            WorktreeLeaseState::CleanupRequested,
            WorktreeLeaseState::Cleaned,
        ] {
            lease.transition(state);
            runtime
                .agent_facility
                .worktrees
                .record(lease.clone())
                .unwrap();
        }
        let error = runtime.ensure_thread_owner(&cleaned).await.unwrap_err();
        assert!(
            format!("{error:#}").contains("workspace is unavailable"),
            "{error:#}"
        );
        assert!(
            runtime
                .thread_factory
                .session_workspace_root_for_test(&cleaned)
                .is_none()
        );
        runtime.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worktree_lifecycle_and_new_submissions_do_not_wait_for_sqlite() {
        use crate::studio::agent_host::worktree_lease::WorktreeLeaseState;
        use pl_protocol::AgentWorkspaceMode;
        use sea_orm::TransactionTrait;
        let (_home, workspace, runtime, root_id) = runtime_with_thread().await;
        for args in [
            vec!["init"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        ] {
            let output = tokio::process::Command::new("git")
                .args(args)
                .current_dir(workspace.path())
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let root = runtime.ensure_thread_owner(&root_id).await.unwrap();
        root.pause_inputs().await.unwrap();
        let writer = runtime.persistence_repository().await.unwrap();
        writer.flush().await.unwrap();
        let project_id = runtime
            .read_owned_thread(&root_id)
            .await
            .unwrap()
            .project_id;
        let project = runtime
            .agent_facility
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == project_id)
            .unwrap();
        let blocked = runtime.store.database().begin().await.unwrap();
        let operation = async {
            let (_, worktree) =
                crate::studio::agent_host::workspace_preparation::prepare_workspace(
                    &runtime.agent_facility.worktrees,
                    &runtime.ssh_manager,
                    crate::studio::agent_host::workspace_preparation::WorkspacePreparation {
                        project: &project,
                        root_thread_id: &root_id,
                        child_id: "memory-worktree-child",
                        mode: AgentWorkspaceMode::Worktree,
                        writable_paths: None,
                        session_root: std::path::PathBuf::from(&project.path),
                    },
                )
                .await
                .unwrap();
            let mut lease = worktree.unwrap().lease;
            let child = lease.owner_thread_id.clone();
            let path = std::path::PathBuf::from(&lease.path);
            assert!(path.exists());
            lease.transition(WorktreeLeaseState::Preserved);
            runtime
                .agent_facility
                .worktrees
                .record(lease.clone())
                .unwrap();
            for (index, detail) in ["first delivery", "new repair delivery"]
                .into_iter()
                .enumerate()
            {
                root.submit_input(pl_core::thread::input::ThreadInput {
                    id: format!("queued-{index}"),
                    payload: pl_core::context::OpaquePayload::text(detail),
                    context: vec![pl_core::context::ContextContent::Text {
                        text: detail.into(),
                    }],
                })
                .await
                .unwrap();
            }
            assert_eq!(root.snapshot().inputs.len(), 2);
            assert!(writer.pending_commit_count() > 0);
            let locked = tokio::process::Command::new("git")
                .args(["worktree", "lock"])
                .arg(&path)
                .current_dir(workspace.path())
                .output()
                .await
                .unwrap();
            assert!(locked.status.success());
            assert!(
                runtime
                    .cleanup_preserved_worktree(
                        crate::StudioRecoveryWorktreeOwner::Child,
                        &child,
                        lease.revision,
                    )
                    .await
                    .is_err()
            );
            let failed = runtime.agent_facility.worktrees.get(&child).unwrap();
            assert_eq!(failed.state, WorktreeLeaseState::Preserved);
            assert!(path.exists());
            let unlocked = tokio::process::Command::new("git")
                .args(["worktree", "unlock"])
                .arg(&path)
                .current_dir(workspace.path())
                .output()
                .await
                .unwrap();
            assert!(unlocked.status.success());
            let branch_lock = std::path::Path::new(&failed.repository_root)
                .join(".git/refs/heads")
                .join(format!("{}.lock", failed.branch));
            tokio::fs::create_dir_all(branch_lock.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(&branch_lock, "held").await.unwrap();
            assert!(
                runtime
                    .cleanup_preserved_worktree(
                        crate::StudioRecoveryWorktreeOwner::Child,
                        &child,
                        failed.revision,
                    )
                    .await
                    .is_err()
            );
            assert!(
                !path.exists(),
                "first cleanup removed the directory before branch deletion failed"
            );
            let partial = runtime.agent_facility.worktrees.get(&child).unwrap();
            assert_eq!(partial.state, WorktreeLeaseState::Preserved);
            tokio::fs::remove_file(branch_lock).await.unwrap();
            runtime
                .cleanup_preserved_worktree(
                    crate::StudioRecoveryWorktreeOwner::Child,
                    &child,
                    partial.revision,
                )
                .await
                .unwrap();
            assert!(!path.exists());
            let lease = runtime.agent_facility.worktrees.get(&child).unwrap();
            assert_eq!(lease.state, WorktreeLeaseState::Cleaned);
            assert!(
                runtime
                    .cleanup_preserved_worktree(
                        crate::StudioRecoveryWorktreeOwner::Child,
                        &child,
                        lease.revision,
                    )
                    .await
                    .is_err(),
                "cleaned leases cannot be cleaned twice"
            );
            lease
        };
        let lease = tokio::time::timeout(std::time::Duration::from_secs(5), operation)
            .await
            .expect("live lifecycle attempted to wait for the occupied database connection");
        blocked.rollback().await.unwrap();
        writer.retry_now();
        writer.flush().await.unwrap();
        let durable = crate::studio::agent_host::worktree_lease::load_lease(
            &runtime.store,
            &lease.owner_thread_id,
        )
        .await
        .unwrap();
        assert_eq!(durable, Some(lease));
        let orphan_root = dunce::simplified(workspace.path());
        #[cfg(windows)]
        let orphan_root =
            std::path::PathBuf::from(orphan_root.to_string_lossy().replace('\\', "/"));
        let orphan = orphan_root.join(".anywork/worktrees/orphan-root/orphan-child");
        let created = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", "pure-agent-orphan-child"])
            .arg(&orphan)
            .arg("HEAD")
            .current_dir(workspace.path())
            .output()
            .await
            .unwrap();
        assert!(created.status.success());
        let mut issues = Vec::new();
        runtime
            .append_worktree_recovery_issues(&mut issues)
            .await
            .unwrap();
        let expected_orphan = dunce::canonicalize(&orphan).unwrap();
        const MESSAGE_PREFIX: &str = "Unregistered worktree preserved at ";
        const MESSAGE_SUFFIX: &str = "; ownership must be inspected before explicit cleanup";
        let reported = issues.iter().any(|issue| {
            let Some(reported_path) = issue
                .message
                .strip_prefix(MESSAGE_PREFIX)
                .and_then(|message| message.strip_suffix(MESSAGE_SUFFIX))
            else {
                return false;
            };
            issue.worktree.is_none()
                && matches!(
                    dunce::canonicalize(reported_path),
                    Ok(path) if path == expected_orphan
                )
        });
        assert!(
            reported,
            "unregistered physical worktree must be reported without inventing ownership; orphan={orphan:?}; issues={issues:#?}"
        );
        assert!(orphan.exists());
        runtime.shutdown().await;
    }
}
