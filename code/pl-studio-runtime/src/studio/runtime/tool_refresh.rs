//! Event-driven catalog replacement preserves the active owner and physical process leases.
use super::{
    StudioRuntime,
    background_task::{self, BackgroundTask},
};
use crate::protocol::StudioProductEventKind;
use pl_core::thread::{ThreadHandle, ThreadLifecycle, ToolOutcome};
use std::collections::BTreeMap;

#[derive(Default)]
struct CatalogSources(BTreeMap<String, String>);
impl CatalogSources {
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
        let runtime = self.clone();
        *slot = Some(BackgroundTask::new(tokio::spawn(async move {
            let mut sources = CatalogSources::default();
            let mut installed: BTreeMap<String, (ThreadHandle, String)> = BTreeMap::new();
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
                runtime
                    .refresh_tool_catalogs(&sources, &mut installed)
                    .await;
                tokio::select! {
                    () = stopping.cancelled() => break,
                    () = runtime.tool_catalog_updates.notified() => {
                        let revision = sources.0.get("ssh").and_then(|value| value.parse::<u64>().ok()).unwrap_or_default().saturating_add(1);
                        sources.0.insert("ssh".into(), revision.to_string());
                    },
                    changed = settings.changed() => if changed.is_err() { break; },
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
        })));
    }

    async fn refresh_tool_catalogs(
        &self,
        sources: &CatalogSources,
        installed: &mut BTreeMap<String, (ThreadHandle, String)>,
    ) {
        let active = self.threads.observed_threads();
        let ids: std::collections::BTreeSet<_> = active.iter().map(|(id, _)| id.clone()).collect();
        installed.retain(|id, _| ids.contains(id));
        for (id, thread) in active {
            if let Err(error) = self.rejected_tools.retry(Some(&thread)).await {
                tracing::warn!(thread_id = %id, %error, "candidate cleanup remains pending");
                continue;
            }
            let snapshot = thread.snapshot();
            if !snapshot
                .attempts
                .iter()
                .any(|attempt| matches!(attempt.outcome, pl_core::thread::AttemptOutcome::Running))
                && let Some(fact) = self.thread_factory.skill_catalog_fact(&id, &snapshot)
            {
                match thread.patch_runtime_facts(vec![fact]).await {
                    Ok(_)
                    | Err(
                        pl_core::thread::ThreadError::PendingTools
                        | pl_core::thread::ThreadError::Closed,
                    ) => {}
                    Err(error) => {
                        tracing::warn!(thread_id = %id, %error, "could not append refreshed Skill catalog facts")
                    }
                }
            }
            if snapshot.lifecycle != ThreadLifecycle::Open {
                continue;
            }
            let fingerprint = catalog_fingerprint(sources, &snapshot);
            if installed.get(&id).is_some_and(|(previous, key)| {
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
                // new producer state; never mark this fingerprint installed or clear its tools.
                self.tool_catalog_updates.notify_one();
                continue;
            }
            installed.insert(id.clone(), (thread, fingerprint));
            let issue_id = format!("tool-refresh:{id}");
            match result {
                Ok(()) => {
                    if self
                        .recovery
                        .snapshot()
                        .iter()
                        .any(|issue| issue.id == issue_id)
                    {
                        self.agent_facility
                            .product_events
                            .emit_recovery_state(self.recovery.remove(&issue_id));
                    }
                }
                Err(error) => {
                    let issues = self.recovery.upsert(crate::studio::StudioRecoveryIssue {
                        id: issue_id, scope: crate::studio::StudioRecoveryIssueScope::Thread,
                        category: crate::studio::StudioRecoveryIssueCategory::AgentState,
                        action: crate::studio::StudioRecoveryIssueAction::CleanupThread,
                        project_id: None, thread_id: Some(id), worktree: None,
                        message: format!("Tool catalog unavailable; correct configuration or reactivate this Thread: {error}"),
                    });
                    self.agent_facility
                        .product_events
                        .emit_recovery_state(issues);
                }
            }
        }
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
        let (tools, exposure) = match prepared {
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
                registrations.push(self.threads.progress_tool_registration()?);
            }
            crate::thread_assembler::AgentControlExposure::ProgressOnly => {
                registrations.push(self.threads.progress_tool_registration()?)
            }
            crate::thread_assembler::AgentControlExposure::Disabled => {}
        }
        thread
            .register_tools_if_extensions(snapshot.extension_sequence, registrations)
            .await?;
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
    async fn cold_activation_keeps_tools_and_directory_available_after_restart() {
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
            .reveal_tools(vec!["read_file".into(), "complete".into()])
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
        let (old, _) = runtime
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
}
