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
        "modelRoute": snapshot.extensions.get(crate::studio::model_route::MODEL_ROUTE_EXTENSION),
        "agentProfile": snapshot.extensions.get(crate::studio::model_route::AGENT_PROFILE_EXTENSION),
    }))
}
