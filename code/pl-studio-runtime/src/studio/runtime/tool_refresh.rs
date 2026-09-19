//! Event-driven catalog replacement preserves the active owner and physical process leases.
use super::{
    StudioRuntime,
    background_task::{self, BackgroundTask},
};
use crate::protocol::StudioProductEventKind;
use pl_core::thread::{ThreadHandle, ThreadLifecycle, ToolOutcome};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
struct CatalogSources(BTreeMap<String, String>);
impl CatalogSources {
    fn for_ssh_server(&self, server: Option<&str>) -> Self {
        let key = server.map(|server| format!("ssh-ready:{server}"));
        Self(
            self.0
                .iter()
                .filter(|(name, _)| !name.starts_with("ssh-ready:") || key.as_ref() == Some(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        )
    }

    fn observe(&mut self, event: StudioProductEventKind) {
        let entry = match event {
            StudioProductEventKind::ProjectDirectoryChanged(value) => {
                Some(("projects".into(), serde_json::json!(value.state.value())))
            }
            StudioProductEventKind::SkillsStateChanged(value) => Some((
                format!("skills:{}", value.project_id),
                serde_json::json!(
                    value
                        .state
                        .value()
                        .map(|data| (&data.config_fingerprint, data.catalog_revision))
                ),
            )),
            StudioProductEventKind::LspStateChanged(value) => Some((
                "lsp".into(),
                serde_json::json!(value.state.value().map(|data| {
                    data.lsp_servers
                        .iter()
                        .map(|server| (&server.id, &server.extensions, &server.language_ids))
                        .collect::<Vec<_>>()
                })),
            )),
            StudioProductEventKind::ThreadModeCatalogChanged(value) => {
                Some(("modes".into(), serde_json::json!(value)))
            }
            StudioProductEventKind::ThreadDirectoryChanged(_)
            | StudioProductEventKind::AgentDirectoryChanged(_)
            | StudioProductEventKind::SettingsStateChanged(_)
            | StudioProductEventKind::RecoveryStateChanged(_)
            | StudioProductEventKind::McpStateChanged(_)
            | StudioProductEventKind::ProviderUsageStateChanged(_)
            | StudioProductEventKind::ModelPerformanceStateChanged(_)
            | StudioProductEventKind::UpdaterStateChanged(_)
            | StudioProductEventKind::PersistenceStateChanged(_) => None,
        };
        if let Some((key, value)) = entry {
            self.0.insert(key, crate::hash::canonical_json_hash(&value));
        }
    }
}

impl StudioRuntime {
    pub(super) async fn start_tool_refresh(&self) {
        let mut slot = self.tool_refresh.lock().await;
        if slot.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let stopping = self.rejected_tools.start();
        let mut settings = self.settings_updates.subscribe();
        let mut mcp = self.external_runtimes.mcp.subscribe();
        let mut events = self.agent_facility.product_events.subscribe();
        let mut ssh_ready = self.ssh_manager.subscribe_ready();
        let runtime = self.clone();
        *slot = Some(BackgroundTask::new(tokio::spawn(async move {
            let mut sources = CatalogSources::default();
            let mut workers: BTreeMap<
                String,
                (ThreadHandle, tokio::sync::watch::Sender<CatalogSources>),
            > = BTreeMap::new();
            let mut tasks = tokio::task::JoinSet::new();
            let mut mcp_changed = true;
            loop {
                if stopping.is_cancelled() {
                    break;
                }
                if mcp_changed {
                    let generation = match runtime.external_runtimes.mcp.acquire_turn_lease().await
                    {
                        Ok(lease) => lease.generation().0.to_string(),
                        Err(error) => format!("unavailable:{error}"),
                    };
                    sources.0.insert("mcp".into(), generation);
                    mcp_changed = false;
                }
                sources.0.insert(
                    "settings".into(),
                    settings.borrow_and_update().revision.to_string(),
                );
                for (server, revision) in ssh_ready.borrow_and_update().iter() {
                    sources
                        .0
                        .insert(format!("ssh-ready:{server}"), revision.to_string());
                }
                let active = runtime.threads.observed_threads();
                workers.retain(|id, (owner, _)| {
                    active.iter().any(|(current_id, current)| {
                        current_id == id && current.same_instance(owner)
                    })
                });
                for (id, thread) in active {
                    let server = runtime.thread_factory.ssh_alias(&id);
                    let selected = sources.for_ssh_server(server.as_deref());
                    if let Some((_, updates)) = workers.get(&id) {
                        updates.send_replace(selected);
                        continue;
                    }
                    let (updates, mut receiver) = tokio::sync::watch::channel(selected);
                    workers.insert(id.clone(), (thread.clone(), updates));
                    let runtime = runtime.clone();
                    let stopping = stopping.clone();
                    tasks.spawn(async move {
                        let mut attempted = BTreeMap::new();
                        loop {
                            if stopping.is_cancelled() {
                                break;
                            }
                            let sources = receiver.borrow_and_update().clone();
                            runtime
                                .refresh_selected_catalogs(
                                    &sources,
                                    &mut attempted,
                                    vec![(id.clone(), thread.clone())],
                                )
                                .await;
                            tokio::select! {
                                () = stopping.cancelled() => break,
                                changed = receiver.changed() => if changed.is_err() { break; },
                            }
                        }
                    });
                }
                tokio::select! {
                    () = stopping.cancelled() => break,
                    finished = tasks.join_next(), if !tasks.is_empty() => {
                        if let Some(Err(error)) = finished { tracing::error!(%error, "tool refresh worker failed"); }
                        workers.retain(|_, (_, updates)| !updates.is_closed());
                    },
                    () = runtime.tool_catalog_updates.notified() => {
                        let revision = sources.0.get("ssh").and_then(|value| value.parse::<u64>().ok()).unwrap_or_default().saturating_add(1);
                        sources.0.insert("ssh".into(), revision.to_string());
                    },
                    changed = settings.changed() => if changed.is_err() { break; },
                    changed = ssh_ready.changed() => if changed.is_err() { break; },
                    changed = mcp.recv() => {
                        if matches!(changed, Err(tokio::sync::broadcast::error::RecvError::Closed)) { break; }
                        mcp_changed = true;
                    },
                    event = events.recv() => match event {
                        Ok(event) => sources.observe(event.kind),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { sources.0.insert("lag".into(), runtime.agent_facility.product_events.current_sequence().to_string()); },
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                }
            }
            drop(workers);
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result {
                    tracing::error!(%error, "tool refresh worker failed during shutdown");
                }
            }
        })));
    }

    #[cfg(test)]
    async fn refresh_tool_catalogs(
        &self,
        sources: &CatalogSources,
        attempted: &mut BTreeMap<String, (ThreadHandle, String)>,
    ) {
        self.refresh_selected_catalogs(sources, attempted, self.threads.observed_threads())
            .await;
    }

    async fn refresh_selected_catalogs(
        &self,
        sources: &CatalogSources,
        attempted: &mut BTreeMap<String, (ThreadHandle, String)>,
        active: Vec<(String, ThreadHandle)>,
    ) {
        let ids: std::collections::BTreeSet<_> = active.iter().map(|(id, _)| id.clone()).collect();
        attempted.retain(|id, _| ids.contains(id));
        for (id, thread) in active {
            let cleanup = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.rejected_tools.retry(Some(&thread)),
            )
            .await;
            if !matches!(cleanup, Ok(Ok(()))) {
                tracing::warn!(thread_id = %id, error = ?cleanup, "candidate cleanup remains pending");
                continue;
            }
            let snapshot = thread.snapshot();
            if let Some(fact) = self.thread_factory.skill_catalog_fact(&id, &snapshot)
                && let Err(error) = thread.queue_runtime_facts(vec![fact]).await
                && !matches!(error, pl_core::thread::ThreadError::Closed)
            {
                tracing::warn!(thread_id = %id, %error, "could not queue refreshed Skill catalog facts");
            }
            if snapshot.lifecycle != ThreadLifecycle::Open {
                continue;
            }
            let server = self.thread_factory.ssh_alias(&id);
            let fingerprint =
                catalog_fingerprint(&sources.for_ssh_server(server.as_deref()), &snapshot);
            if attempted.get(&id).is_some_and(|(previous, key)| {
                previous.same_instance(&thread) && key == &fingerprint
            }) {
                continue;
            }
            let result = self.install_refreshed_tools(&id, &thread, &snapshot).await;
            let result = match result {
                Err(error) => match error.downcast::<pl_core::thread::ThreadError>() {
                    Ok(pl_core::thread::ThreadError::RejectedTools(resources)) => {
                        tracing::warn!(thread_id = %id, error = %resources, "retaining rejected candidate cleanup owner");
                        self.rejected_tools.retain(thread.clone(), resources);
                        // Retry the original owner once promptly. Persistent failure waits for a source event
                        // or shutdown; no new candidates accumulate while cleanup remains pending.
                        self.tool_catalog_updates.notify_one();
                        continue;
                    }
                    Ok(error) => Err(anyhow::Error::new(error)),
                    Err(error) => Err(error),
                },
                Ok(()) => Ok(()),
            };
            if result.as_ref().err().is_some_and(|error| {
                matches!(
                    error.downcast_ref::<pl_core::thread::ThreadError>(),
                    Some(pl_core::thread::ThreadError::ExtensionSequenceConflict { .. })
                )
            }) {
                // The stale candidate was closed by the transfer API. Reprepare against the
                // new producer state; never mark this fingerprint attempted or clear its tools.
                self.tool_catalog_updates.notify_one();
                continue;
            }
            // Cache attempts, including failures, until an input revision changes.
            // Recovery issues remain visible; only successful transfer commits a binding.
            attempted.insert(id.clone(), (thread.clone(), fingerprint));
            self.publish_tool_refresh_result(&id, &thread, result.as_ref().err());
        }
    }

    pub(in crate::studio::runtime) fn publish_tool_refresh_result(
        &self,
        id: &str,
        thread: &ThreadHandle,
        error: Option<&anyhow::Error>,
    ) {
        let issue_id = format!("tool-refresh:{id}");
        let issue = error.map(|error| crate::studio::StudioRecoveryIssue {
            id: issue_id.clone(),
            scope: crate::studio::StudioRecoveryIssueScope::Thread,
            category: crate::studio::StudioRecoveryIssueCategory::AgentState,
            action: crate::studio::StudioRecoveryIssueAction::CleanupThread,
            project_id: None,
            thread_id: Some(id.to_string()),
            worktree: None,
            message: format!(
                "Tool catalog unavailable; correct configuration or reactivate this Thread: {error}"
            ),
        });
        self.recovery.update_if_current(
            &issue_id,
            issue,
            || {
                self.threads
                    .thread(id)
                    .is_some_and(|current| current.same_instance(thread))
            },
            |issues| {
                self.agent_facility
                    .product_events
                    .emit_recovery_state(issues);
            },
        );
    }

    async fn install_refreshed_tools(
        &self,
        id: &str,
        thread: &ThreadHandle,
        snapshot: &pl_core::thread::ThreadSnapshot,
    ) -> anyhow::Result<()> {
        let prepared = self
            .thread_factory
            .refresh_thread_tools(id, snapshot, tokio_util::sync::CancellationToken::new())
            .await;
        let (tools, exposure, binding) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                thread
                    .register_tools_if_extensions(snapshot.extension_sequence, Vec::new())
                    .await?;
                return Err(error.into());
            }
        };
        let mut registrations = tools.into_registrations();
        match exposure {
            crate::thread_assembler::AgentControlExposure::Enabled => {
                registrations.extend(self.threads.agent_control_tools()?);
            }
            crate::thread_assembler::AgentControlExposure::Disabled => {}
        }
        thread
            .register_tools_if_extensions(snapshot.extension_sequence, registrations)
            .await?;
        self.publish_tool_binding(id, thread, snapshot.extension_sequence, binding)
            .await?;
        if let Some(fact) = self
            .thread_factory
            .skill_catalog_fact(id, &thread.snapshot())
        {
            thread.queue_runtime_facts(vec![fact]).await?;
        }
        Ok(())
    }

    // The refresh loop serializes installations. An extension change can still
    // supersede this catalog, so rollback uses the same producer sequence guard.
    async fn publish_tool_binding(
        &self,
        id: &str,
        thread: &ThreadHandle,
        extension_sequence: u64,
        binding: crate::studio::thread_factory::RefreshedToolBinding,
    ) -> anyhow::Result<()> {
        if thread.snapshot().lifecycle != ThreadLifecycle::Open {
            return Err(pl_core::thread::ThreadError::Closed.into());
        }
        if let Err(error) = self.thread_factory.commit_refreshed_binding(id, binding) {
            thread
                .register_tools_if_extensions(extension_sequence, Vec::new())
                .await?;
            return Err(error.into());
        }
        Ok(())
    }

    pub(super) async fn stop_tool_refresh(&self) -> anyhow::Result<()> {
        self.rejected_tools.stop();
        background_task::finish(&self.tool_refresh)
            .await
            .map_err(anyhow::Error::new)?;
        self.rejected_tools.retry(None).await
    }
}

fn catalog_fingerprint(
    sources: &CatalogSources,
    snapshot: &pl_core::thread::ThreadSnapshot,
) -> String {
    let mutation = snapshot
        .deliveries
        .iter()
        .rev()
        .find(|delivery| {
            delivery.tool_id == "skill_manage"
                && delivery.output.payload().format() == "pl.tool.skill-mutation"
                && matches!(delivery.outcome, ToolOutcome::Succeeded)
        })
        .map(|delivery| &delivery.call_id);
    crate::hash::canonical_json_hash(&serde_json::json!({
        "sources": sources.0,
        "skillMutation": mutation,
        "mode": snapshot.extensions.get("studio.mode"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn blocked_candidate_cleanup_does_not_block_another_thread_refresh() -> anyhow::Result<()>
    {
        use pl_core::{
            context::OpaquePayload,
            tool::opaque::{CallContext, Registration, Tool, ToolError},
        };
        use std::sync::{
            Arc,
            atomic::{AtomicU8, Ordering},
        };
        #[derive(Debug)]
        struct Cleanup {
            phase: AtomicU8,
            entered: tokio::sync::Notify,
            release: tokio::sync::Notify,
        }
        #[derive(Debug)]
        struct Candidate(Arc<Cleanup>);
        impl Tool for Candidate {
            async fn execute(
                &self,
                _: OpaquePayload,
                _: CallContext,
            ) -> Result<pl_core::tool::ToolOutput, ToolError> {
                unreachable!()
            }
            async fn close(&self) -> Result<(), ToolError> {
                match self.0.phase.load(Ordering::SeqCst) {
                    0 => Err(ToolError::new(std::io::Error::other("retain candidate"))),
                    1 => {
                        self.0.entered.notify_one();
                        self.0.release.notified().await;
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
        }
        let home = tempfile::tempdir()?;
        let workspace = tempfile::tempdir()?;
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().into()),
            host: crate::StudioHostKind::Test,
        })
        .await?;
        runtime.start_runtime().await?;
        runtime.stop_tool_refresh().await?;
        let project = runtime.open_project(workspace.path()).await?;
        let blocked_record = runtime
            .create_thread(&project.id, "blocked cleanup")
            .await?;
        let blocked = runtime.ensure_thread_owner(&blocked_record.id).await?;
        let other_record = runtime
            .create_thread(&project.id, "independent refresh")
            .await?;
        let other = runtime.ensure_thread_owner(&other_record.id).await?;
        let cleanup = Arc::new(Cleanup {
            phase: AtomicU8::new(0),
            entered: Default::default(),
            release: Default::default(),
        });
        let error = blocked
            .register_tools_if_extensions(
                u64::MAX,
                vec![Registration::new(
                    "duplicate".into(),
                    OpaquePayload::text("candidate"),
                    Candidate(cleanup.clone()),
                )?],
            )
            .await
            .unwrap_err();
        let pl_core::thread::ThreadError::RejectedTools(resources) = error else {
            panic!("missing rejected cleanup ownership: {error}");
        };
        runtime.rejected_tools.retain(blocked, resources);
        cleanup.phase.store(1, Ordering::SeqCst);
        let previous = runtime
            .thread_factory
            .binding_incarnation_for_test(&other_record.id)
            .unwrap();
        runtime.start_tool_refresh().await;
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            cleanup.entered.notified().await;
            while runtime
                .thread_factory
                .binding_incarnation_for_test(&other_record.id)
                .is_none_or(|current| Arc::ptr_eq(&previous, &current))
            {
                tokio::task::yield_now().await;
            }
            other.reveal_tools(vec!["read_file".into()]).await
        })
        .await;
        cleanup.phase.store(2, Ordering::SeqCst);
        cleanup.release.notify_one();
        runtime.stop_tool_refresh().await?;
        runtime.shutdown_runtime().await?;
        result??;
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires embedded-remote-helpers, PURE_SSH_TEST_SERVER/USERNAME/WORKSPACE and key authentication"]
    async fn ssh_reconnect_restores_existing_thread_tools() -> anyhow::Result<()> {
        use pl_core::{context::OpaquePayload, model::*, thread::*};
        use pl_tool::remote::SshServerProfile;
        use tokio_util::sync::CancellationToken;

        #[derive(Clone)]
        struct Call(ModelToolCall);
        impl Model for Call {
            async fn open_session(&self) -> Result<DynModelSession, ModelError> {
                Ok(DynModelSession::new(self.clone()))
            }
        }
        impl ModelSession for Call {
            async fn prepare(
                &mut self,
                request: ModelRequest,
            ) -> Result<PreparedModelCall, ModelError> {
                let call = self.0.clone();
                Ok(PreparedModelCall::new(async move {
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        tool_calls: vec![call],
                        private_context: None,
                        usage: Default::default(),
                    })
                }))
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }
        #[derive(Clone)]
        struct ReconnectingTurn {
            step: usize,
            entered: std::sync::Arc<tokio::sync::Notify>,
            release: std::sync::Arc<tokio::sync::Notify>,
        }
        impl Model for ReconnectingTurn {
            async fn open_session(&self) -> Result<DynModelSession, ModelError> {
                Ok(DynModelSession::new(self.clone()))
            }
        }
        impl ModelSession for ReconnectingTurn {
            async fn prepare(
                &mut self,
                request: ModelRequest,
            ) -> Result<PreparedModelCall, ModelError> {
                let step = self.step;
                self.step += 1;
                let entered = self.entered.clone();
                let release = self.release.clone();
                Ok(PreparedModelCall::new(async move {
                    if step == 0 {
                        entered.notify_one();
                        release.notified().await;
                    }
                    let tool_calls = if step < 2 {
                        [
                            ("read_file", serde_json::json!({"path":"fixture.txt"})),
                            (
                                "exec",
                                serde_json::json!({"command":"cat fixture.txt","cwd":"."}),
                            ),
                        ]
                        .into_iter()
                        .map(|(tool, args)| ModelToolCall {
                            call_id: format!("running-reconnect-{step}-{tool}"),
                            tool_id: tool.into(),
                            arguments: OpaquePayload::new("application/json", 1, args.to_string())
                                .unwrap(),
                        })
                        .collect()
                    } else {
                        Vec::new()
                    };
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        tool_calls,
                        private_context: None,
                        usage: Default::default(),
                    })
                }))
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }

        async fn start_call(
            thread: &ThreadHandle,
            tool: &str,
            arguments: serde_json::Value,
        ) -> anyhow::Result<String> {
            static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let call_id = format!(
                "probe-{}",
                CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            thread
                .replace_model(ModelFactory::new(Call(ModelToolCall {
                    call_id: call_id.clone(),
                    tool_id: tool.into(),
                    arguments: OpaquePayload::new("application/json", 1, arguments.to_string())?,
                })))
                .await?;
            thread.reveal_tools(vec![tool.into()]).await?;
            thread
                .step(StepInput {
                    turn_id: call_id.clone(),
                    attempt_id: call_id.clone(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new(),
                })
                .await?;
            let _ = thread
                .execute_tool(call_id.clone(), CancellationToken::new())
                .await?;
            Ok(call_id)
        }
        async fn invoke(
            thread: &ThreadHandle,
            tool: &str,
            arguments: serde_json::Value,
        ) -> anyhow::Result<ToolDelivery> {
            let call_id = start_call(thread, tool, arguments).await?;
            Ok(thread
                .wait_task(&format!("task:{call_id}"), CancellationToken::new())
                .await?)
        }
        async fn probe(thread: &ThreadHandle) -> anyhow::Result<()> {
            for (tool, args) in [
                ("read_file", serde_json::json!({"path": "fixture.txt"})),
                (
                    "exec",
                    serde_json::json!({"command": "cat fixture.txt", "cwd": "."}),
                ),
            ] {
                let delivery = invoke(thread, tool, args).await?;
                anyhow::ensure!(
                    matches!(delivery.outcome, ToolOutcome::Succeeded),
                    "{tool}: {:?}",
                    delivery.output
                );
                anyhow::ensure!(
                    delivery
                        .output
                        .payload()
                        .content()
                        .contains("reconnect-fixture"),
                    "missing fixture: {:?}",
                    delivery.output
                );
            }
            Ok(())
        }

        let home = tempfile::tempdir()?;
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().to_owned()),
            host: crate::StudioHostKind::Test,
        })
        .await?;
        let config = runtime.config_runtime.read()?;
        runtime.config_runtime.update(config.revision, |config| {
            let mut config = config.clone();
            config.runtime.permission_mode = crate::approval::PermissionMode::FullAccess;
            config.runtime.tool_capabilities.skills = false;
            config.runtime.tool_capabilities.lsp = false;
            config.runtime.tool_capabilities.mcp = false;
            Ok(config)
        })?;
        runtime.start_runtime().await?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), async {
            runtime
                .save_ssh_server(SshServerProfile {
                    alias: "reconnect".into(),
                    host_name: std::env::var("PURE_SSH_TEST_SERVER")?,
                    port: 22,
                    username: std::env::var("PURE_SSH_TEST_USERNAME")?,
                    identity_file: None,
                })
                .await?;
            let path = std::env::var("PURE_SSH_TEST_WORKSPACE")?;
            let project = runtime
                .open_remote_project("reconnect", path.clone())
                .await?;
            let record = runtime
                .create_thread(&project.id, "reconnect regression")
                .await?;
            let thread = runtime.ensure_thread_owner(&record.id).await?;
            runtime.stop_tool_refresh().await?;
            let child_id = format!("{}-child", record.id);
            let profile = runtime.config_runtime.resolve_agent_profile("executor")?;
            let spec = crate::thread_assembler::StudioChildResources::prepare(
                &runtime.thread_factory,
                &crate::thread_assembler::ChildThreadRequest {
                    id: child_id.clone(), caller: record.id.clone(), call_id: "child-fixture".into(),
                    profile_id: "executor".into(), cancellation: CancellationToken::new(),
                    task_summary: String::from("Reconnect fixture").try_into()?,
                    writable_paths: None, metadata: OpaquePayload::text("reconnect fixture"),
                }, &profile,
            ).await?;
            let child = runtime.threads.assemble(spec).await?;
            probe(&thread).await?;
            probe(&child).await?;
            let waiting = start_call(&thread, "exec", serde_json::json!({
                "command": "read value; printf '%s' \"$value\"", "cwd": "."
            })).await?;
            runtime.install_refreshed_tools(&record.id, &thread, &thread.snapshot()).await?;
            invoke(&thread, "write_stdin", serde_json::json!({
                "taskId": format!("task:{waiting}"), "chars": "preserved-process\n"
            })).await?;
            let completed = thread.wait_task(&format!("task:{waiting}"), CancellationToken::new()).await?;
            anyhow::ensure!(matches!(completed.outcome, ToolOutcome::Succeeded)
                && completed.output.payload().content().contains("preserved-process"), "catalog refresh interrupted a live command");
            let initial = thread.snapshot().context.records.to_vec();
            for automatic in [false, true, false] {
                let old = runtime
                    .ssh_manager
                    .open_workspace_host("reconnect", path.clone())
                    .await?;
                let previous = runtime.thread_factory.binding_incarnation_for_test(&record.id).unwrap();
                runtime.start_tool_refresh().await;
                // Let the initial catalog pass finish before disconnecting, so only
                // the Ready notification can trigger the recovery being tested.
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    while runtime.thread_factory.binding_incarnation_for_test(&record.id)
                        .is_none_or(|current| std::sync::Arc::ptr_eq(&previous, &current)) {
                        tokio::task::yield_now().await;
                    }
                }).await?;
                if automatic {
                    // Kill only this test-owned helper transport, causing the production
                    // disconnect monitor and backoff loop to establish its replacement.
                    let interrupted = invoke(
                        &thread,
                        "exec",
                        serde_json::json!({
                            "command": "kill -KILL \"$(awk '/^PPid:/{print $2}' /proc/$PPID/status)\"", "cwd": "."
                        }),
                    )
                    .await;
                    match interrupted {
                        Ok(delivery) => anyhow::ensure!(!matches!(delivery.outcome, ToolOutcome::Succeeded), "lost transport reported success"),
                        Err(error) => anyhow::ensure!(format!("{error:#}").contains("remoteDisconnected"), "unexpected disconnect error: {error:#}"),
                    }
                } else {
                    runtime.reconnect_ssh_server("reconnect").await?;
                }
                // Observe committed binding replacement, not Ready alone. No command
                // retries or manual refresh may conceal a missing reconnect notification.
                tokio::time::timeout(std::time::Duration::from_secs(20), async {
                    while [&record.id, &child_id].iter().any(|id| runtime.thread_factory.remote_binding_for_test(id)
                        .is_none_or(|host| host.files.is_same_binding(&old.files))) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .map_err(|_| {
                    anyhow::anyhow!("Ready did not replace the original Thread binding")
                })?;
                runtime.stop_tool_refresh().await?;
                let current = runtime
                    .ssh_manager
                    .open_workspace_host("reconnect", path.clone())
                    .await?;
                anyhow::ensure!(
                    !old.files.is_same_binding(&current.files),
                    "transport was not replaced"
                );
                probe(&thread).await?;
                probe(&child).await?;
            }
            // A whole Turn stays active across reconnect; admitted old calls fail, the
            // following request binds fresh tools without waiting for Turn completion.
            let entered = std::sync::Arc::new(tokio::sync::Notify::new());
            let release = std::sync::Arc::new(tokio::sync::Notify::new());
            thread.replace_model(ModelFactory::new(ReconnectingTurn { step: 0, entered: entered.clone(), release: release.clone() })).await?;
            thread.reveal_tools(vec!["read_file".into(), "exec".into()]).await?;
            let active_owner = thread.clone();
            let running = tokio::spawn(async move { active_owner.run_turn(TurnInput {
                turn_id: "running-reconnect".into(), attempt_prefix: "running-reconnect".into(), content: Vec::new(),
                max_model_steps: ModelStepLimit::Unlimited, cancellation: CancellationToken::new(),
            }).await });
            entered.notified().await;
            thread.queue_runtime_facts(vec![RuntimeFact { source_id: "regression.skills".into(), content: vec![pl_core::context::ContextContent::Text { text: "updated skills".into() }] }]).await?;
            let old = runtime.thread_factory.remote_binding_for_test(&record.id).unwrap();
            runtime.start_tool_refresh().await;
            runtime.reconnect_ssh_server("reconnect").await?;
            tokio::time::timeout(std::time::Duration::from_secs(20), async {
                while runtime.thread_factory.remote_binding_for_test(&record.id).is_none_or(|current| current.files.is_same_binding(&old.files)) { tokio::task::yield_now().await; }
            }).await?;
            release.notify_one();
            let completed = running.await??;
            anyhow::ensure!(completed.outcome == TurnOutcome::Completed, "running Turn did not recover");
            for tool in ["read_file", "exec"] {
                let snapshot = thread.snapshot();
                let delivery = snapshot.deliveries.iter().find(|delivery| delivery.call_id == format!("running-reconnect-1-{tool}")).unwrap();
                anyhow::ensure!(matches!(delivery.outcome, ToolOutcome::Succeeded) && delivery.output.payload().content().contains("reconnect-fixture"), "running Turn failed to restore {tool}: {delivery:?}");
            }
            runtime.stop_tool_refresh().await?;
            assert_eq!(
                &thread.snapshot().context.records[..initial.len()],
                initial.as_slice()
            );
            invoke(&thread, "exec", serde_json::json!({"command": "mkdir unavailable; cp fixture.txt unavailable/fixture.txt", "cwd": "."})).await?;
            let missing_project = runtime.open_remote_project("reconnect", format!("{path}/unavailable")).await?;
            let missing_record = runtime.create_thread(&missing_project.id, "missing workspace").await?;
            let missing_thread = runtime.ensure_thread_owner(&missing_record.id).await?;
            let previous = runtime.thread_factory.binding_incarnation_for_test(&missing_record.id).unwrap();
            invoke(&thread, "exec", serde_json::json!({"command": "mv unavailable unavailable-moved", "cwd": "."})).await?;
            runtime.reconnect_ssh_server("reconnect").await?;
            anyhow::ensure!(runtime.install_refreshed_tools(&missing_record.id, &missing_thread, &missing_thread.snapshot()).await.is_err(), "missing workspace silently recovered");
            anyhow::ensure!(std::sync::Arc::ptr_eq(&previous, &runtime.thread_factory.binding_incarnation_for_test(&missing_record.id).unwrap()), "failed reopen published a binding");
            runtime.install_refreshed_tools(&record.id, &thread, &thread.snapshot()).await?;
            invoke(&thread, "exec", serde_json::json!({"command": "mv unavailable-moved unavailable", "cwd": "."})).await?;
            runtime.install_refreshed_tools(&missing_record.id, &missing_thread, &missing_thread.snapshot()).await?;
            probe(&missing_thread).await?;
            missing_thread.close().await?;
            anyhow::ensure!(runtime.install_refreshed_tools(&missing_record.id, &missing_thread, &missing_thread.snapshot()).await.is_err(), "closed Thread accepted refreshed tools");
            let cancelled = CancellationToken::new();
            cancelled.cancel();
            anyhow::ensure!(runtime.thread_factory.refresh_thread_tools(&record.id, &thread.snapshot(), cancelled).await.is_err(), "cancelled refresh succeeded");
            let snapshot = thread.snapshot();
            let (tools, _, binding) = runtime.thread_factory.refresh_thread_tools(&record.id, &snapshot, CancellationToken::new()).await?;
            thread.register_tools_if_extensions(snapshot.extension_sequence, tools.into_registrations()).await?;
            runtime.thread_factory.invalidate_ssh_bindings("reconnect");
            anyhow::ensure!(runtime.publish_tool_binding(&record.id, &thread, snapshot.extension_sequence, binding).await.is_err(), "changed credentials reused a binding");
            anyhow::ensure!(thread.reveal_tools(vec!["read_file".into()]).await.is_err(), "invalidated candidate tools remained registered");
            Ok::<_, anyhow::Error>(())
        })
        .await;
        runtime.shutdown_runtime().await?;
        result??;
        Ok(())
    }

    #[tokio::test]
    async fn cold_activation_keeps_tools_and_directory_available_after_restart() {
        use pl_core::context::OpaquePayload;
        use tokio_util::sync::CancellationToken;

        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let options = || crate::StudioRuntimeOptions {
            studio_home: Some(home.path().to_owned()),
            host: crate::StudioHostKind::Test,
        };
        let runtime = StudioRuntime::with_options(options()).await.unwrap();
        runtime.start_runtime().await.unwrap();
        runtime.stop_tool_refresh().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        let record = runtime
            .create_thread(&project.id, "cold tools")
            .await
            .unwrap();
        let profile = runtime
            .config_runtime
            .resolve_agent_profile("explorer")
            .unwrap();
        let child_id = format!("{}-child", record.id);
        let child_spec = crate::thread_assembler::StudioChildResources::prepare(
            &runtime.thread_factory,
            &crate::thread_assembler::ChildThreadRequest {
                id: child_id.clone(),
                caller: record.id.clone(),
                call_id: "cold-child".into(),
                profile_id: "explorer".into(),
                cancellation: CancellationToken::new(),
                task_summary: String::from("  核对\n 消息恢复  ").try_into().unwrap(),
                writable_paths: None,
                metadata: OpaquePayload::text("cold child fixture"),
            },
            &profile,
        )
        .await
        .unwrap();
        let child = runtime.threads.assemble(child_spec).await.unwrap();
        for (id, text) in [("initial", "初始任务"), ("followup", "后续补充")] {
            child
                .send_message(pl_core::thread::inbox::ThreadMessage {
                    id: id.into(),
                    source_id: format!("agent:{}", record.id),
                    payload: OpaquePayload::text(text),
                    context: vec![pl_core::context::ContextContent::Text { text: text.into() }],
                })
                .await
                .unwrap();
        }
        assert_eq!(
            runtime.read_owned_thread(&child_id).await.unwrap().title,
            "核对 消息恢复"
        );
        runtime.shutdown_runtime().await.unwrap();
        drop(runtime);

        let reopened = StudioRuntime::with_options(options()).await.unwrap();
        reopened.start_runtime().await.unwrap();
        // Drive the refresh explicitly so the regression does not depend on event timing.
        reopened.stop_tool_refresh().await.unwrap();
        assert!(
            reopened
                .agent_facility
                .product_events
                .thread_snapshot(&record.id)
                .is_none()
        );
        let thread = reopened.ensure_thread_owner(&record.id).await.unwrap();
        reopened
            .refresh_tool_catalogs(&CatalogSources::default(), &mut BTreeMap::new())
            .await;
        let issues = reopened.recovery_issues();
        assert!(
            issues.is_empty(),
            "cold activation must not invalidate tools: {issues:?}"
        );
        thread
            .reveal_tools(vec!["read_file".into(), "finish_turn".into()])
            .await
            .unwrap();
        reopened
            .synchronize_thread_observation(&record.id)
            .await
            .unwrap();
        let directory = reopened
            .agent_facility
            .product_events
            .thread_snapshot(&record.id)
            .unwrap();
        assert_eq!(directory.status, pl_protocol::ThreadStatus::Idle);
        assert_eq!(directory.title, record.title);
        reopened.read_owned_thread(&record.id).await.unwrap();
        assert_eq!(
            reopened.read_owned_thread(&child_id).await.unwrap().title,
            "核对 消息恢复"
        );
        let page = reopened
            .list_timeline_items(&child_id, pl_protocol::TimelineQuery::Latest, 100)
            .await
            .unwrap();
        let messages = page
            .items
            .iter()
            .filter_map(|item| item.text())
            .filter(|text| text.channel() == pl_protocol::ThreadTextChannel::ParentAgent)
            .map(|text| text.text())
            .collect::<Vec<_>>();
        assert_eq!(messages, ["初始任务", "后续补充"]);
        reopened.shutdown_runtime().await.unwrap();
    }

    #[tokio::test]
    async fn delayed_simple_catalog_cannot_remove_tools_installed_by_task_mode_switch() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let runtime = StudioRuntime::with_options(crate::StudioRuntimeOptions {
            studio_home: Some(home.path().to_owned()),
            host: crate::StudioHostKind::Test,
        })
        .await
        .unwrap();
        runtime.start_runtime().await.unwrap();
        // Hold the old preparation explicitly; no timing sleeps or external models are needed.
        runtime.stop_tool_refresh().await.unwrap();
        let project = runtime.open_project(workspace.path()).await.unwrap();
        let record = runtime
            .create_thread(&project.id, "mode catalog race")
            .await
            .unwrap();
        let thread = runtime.ensure_thread_owner(&record.id).await.unwrap();
        let frozen = thread.snapshot();
        let (old, _, old_binding) = runtime
            .thread_factory
            .refresh_thread_tools(
                &record.id,
                &frozen,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        let old = old.into_registrations();
        assert!(
            !old.iter()
                .any(|tool| tool.tool_id() == "workflow_transition")
        );
        runtime
            .set_thread_mode(
                &record.id,
                pl_protocol::ThreadModeId::new("mode.task").unwrap(),
            )
            .await
            .unwrap();
        let expected = thread.snapshot();
        let error = thread
            .register_tools_if_extensions(frozen.extension_sequence, old)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            pl_core::thread::ThreadError::ExtensionSequenceConflict { .. }
        ));
        assert_eq!(thread.snapshot().commit_sequence, expected.commit_sequence);
        thread
            .reveal_tools(vec![
                "workflow_transition".into(),
                "workflow_current".into(),
            ])
            .await
            .unwrap();
        runtime
            .install_refreshed_tools(&record.id, &thread, &thread.snapshot())
            .await
            .unwrap();
        assert!(
            runtime
                .thread_factory
                .commit_refreshed_binding(&record.id, old_binding)
                .is_err(),
            "a rejected candidate cannot overwrite the successfully installed binding"
        );
        thread
            .reveal_tools(vec![
                "workflow_transition".into(),
                "workflow_current".into(),
            ])
            .await
            .unwrap();
        runtime.shutdown_runtime().await.unwrap();
    }

    #[test]
    fn mode_identity_changes_catalog_fingerprint_but_unrelated_progress_does_not() {
        use pl_core::{context::OpaquePayload, thread::extensions::ExtensionRecord};
        let sources = CatalogSources::default();
        let mut snapshot = pl_core::thread::ThreadSnapshot::default();
        let initial = catalog_fingerprint(&sources, &snapshot);
        snapshot.extensions.insert(
            "studio.mode".into(),
            ExtensionRecord {
                revision: 1,
                payload: OpaquePayload::new("pl.studio.mode", 1, "\"mode.task\"").unwrap(),
            },
        );
        let task = catalog_fingerprint(&sources, &snapshot);
        assert_ne!(initial, task);
        snapshot.extensions.insert(
            "studio.progress".into(),
            ExtensionRecord {
                revision: 2,
                payload: OpaquePayload::text("progress"),
            },
        );
        assert_eq!(catalog_fingerprint(&sources, &snapshot), task);
    }

    #[test]
    fn ordinary_thread_events_do_not_change_catalog_sources() {
        let mut sources = CatalogSources::default();
        sources.observe(StudioProductEventKind::ThreadDirectoryChanged(
            crate::protocol::StudioThreadDirectoryDelta {
                revision: 3,
                updated_at: 12,
                upserted: Vec::new(),
                removed: Vec::new(),
            },
        ));
        assert_eq!(sources.0, BTreeMap::new());
    }

    #[test]
    fn reconnect_invalidates_only_catalogs_bound_to_that_server() {
        let snapshot = pl_core::thread::ThreadSnapshot::default();
        let mut sources = CatalogSources::default();
        sources.0.insert("ssh-ready:a".into(), "1".into());
        let for_server = |sources: &CatalogSources, server| {
            catalog_fingerprint(&sources.for_ssh_server(server), &snapshot)
        };
        let before = [None, Some("a"), Some("b")].map(|server| for_server(&sources, server));
        sources.0.insert("ssh-ready:a".into(), "2".into());
        let after = [None, Some("a"), Some("b")].map(|server| for_server(&sources, server));
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        assert_eq!(before[2], after[2]);
    }
}
