//! One idle transaction for host-owned state, instruction and tool changes.
use super::*;

/// A host-selected transaction. Content meanings and tool group membership remain outside core.
#[derive(Debug)]
pub struct IdleReconfiguration {
    pub expected_sequence: u64,
    pub application: extensions::ApplicationUpdate,
    pub context: Option<ReplaceContext>,
    pub remove_tools: Vec<String>,
    pub tools: Vec<crate::tool::opaque::Registration>,
}

impl Owner {
    pub(super) fn reconfigure(
        &mut self,
        update: IdleReconfiguration,
    ) -> Result<ThreadSnapshot, ThreadError> {
        self.tools.retain_candidates(&update.tools);
        if self.interrupt.is_closing() || self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if self.state.commit_sequence != update.expected_sequence {
            return Err(ThreadError::ContextConflict {
                expected: update.expected_sequence,
                actual: self.state.commit_sequence,
            });
        }
        if self.active_input.is_some()
            || self
                .state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            || self
                .state
                .inputs
                .iter()
                .any(|input| input.state == input::InputState::Pending)
            || self
                .state
                .tasks
                .values()
                .any(|task| task.status == task::TaskStatus::Running)
            || !self.pending.is_empty()
            || !self.uncommitted_tools.is_empty()
        {
            return Err(ThreadError::InputRequiresIdle);
        }
        if self
            .state
            .interactions
            .values()
            .any(|record| record.state == interactions::InteractionState::Pending)
            || self
                .state
                .permissions
                .values()
                .any(|record| matches!(record.state, permissions::PermissionState::Pending))
        {
            return Err(ThreadError::PendingInteraction);
        }
        let mut candidate = self.state.clone();
        extensions::stage_extensions(&mut candidate, update.application.mutations)?;
        facts::stage_facts(&mut candidate, update.application.facts)?;
        if let Some(mut replacement) = update.context {
            if replacement.expected_revision != self.state.context.revision {
                return Err(ThreadError::ContextConflict {
                    expected: replacement.expected_revision,
                    actual: self.state.context.revision,
                });
            }
            candidate.context = self.state.context.clone();
            replacement.expected_revision = candidate.context.revision;
            replacement::stage_replacement(&mut candidate, replacement)?;
        }
        self.tools.patch(&update.remove_tools, update.tools)?;
        candidate.discovered_tools = self.tools.discovery();
        self.state = candidate;
        self.retry_plan = None;
        self.publish();
        Ok(self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{ModelSession, PreparedModelCall},
        tool::{
            ToolOutput,
            opaque::{CallContext, Registration, Tool, ToolError},
        },
    };
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    struct NoModel;
    impl ModelSession for NoModel {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            Err(ModelError {
                kind: crate::model::ModelFailureKind::Unavailable,
                usage: Default::default(),
                details: None,
                source: None,
            })
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    #[derive(Debug)]
    struct ClosingTool {
        label: &'static str,
        closed: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Tool for ClosingTool {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            Err(ToolError::new(std::io::Error::other(
                "execution is not part of this registry transaction",
            )))
        }
        async fn close(&self) -> Result<(), ToolError> {
            self.closed.lock().unwrap().push(self.label);
            Ok(())
        }
    }
    fn registration(
        id: &str,
        label: &'static str,
        closed: &Arc<Mutex<Vec<&'static str>>>,
    ) -> Registration {
        Registration::new(
            id.into(),
            OpaquePayload::text(label),
            ClosingTool {
                label,
                closed: closed.clone(),
            },
        )
        .unwrap()
    }
    fn update(sequence: u64, tools: Vec<Registration>) -> IdleReconfiguration {
        IdleReconfiguration {
            expected_sequence: sequence,
            application: extensions::ApplicationUpdate {
                mutations: vec![],
                facts: vec![],
            },
            context: None,
            remove_tools: vec![],
            tools,
        }
    }

    #[tokio::test]
    async fn stale_catalog_closes_candidates_and_preserves_atomically_reconfigured_tools() {
        let thread = ThreadHandle::start("catalog".into(), DynModelSession::new(NoModel)).unwrap();
        let closed = Arc::new(Mutex::new(Vec::new()));
        let frozen = thread.snapshot();
        let mut change = update(
            frozen.commit_sequence,
            vec![registration("new", "new", &closed).deferred()],
        );
        change
            .application
            .mutations
            .push(extensions::ExtensionMutation::Put {
                id: "producer.mode".into(),
                expected_revision: None,
                payload: OpaquePayload::text("new-mode"),
            });
        thread.reconfigure(change).await.unwrap();
        let before = thread.snapshot();
        let error = thread
            .register_tools_if_extensions(
                frozen.extension_sequence,
                vec![registration("old", "stale", &closed).deferred()],
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ThreadError::ExtensionSequenceConflict {
                expected: 0,
                actual: 1
            }
        ));
        assert_eq!(*closed.lock().unwrap(), vec!["stale"]);
        assert_eq!(thread.snapshot().commit_sequence, before.commit_sequence);
        thread.reveal_tools(vec!["new".into()]).await.unwrap();
        assert_eq!(
            thread
                .snapshot()
                .discovered_tools
                .iter()
                .map(|tool| tool.tool_id.as_str())
                .collect::<Vec<_>>(),
            vec!["new"]
        );
        assert!(thread.reveal_tools(vec!["old".into()]).await.is_err());
        thread
            .register_tools_if_extensions(
                before.extension_sequence,
                vec![registration("fresh", "fresh", &closed).deferred()],
            )
            .await
            .unwrap();
        thread.reveal_tools(vec!["fresh".into()]).await.unwrap();
        assert_eq!(
            thread
                .snapshot()
                .discovered_tools
                .iter()
                .map(|tool| tool.tool_id.as_str())
                .collect::<Vec<_>>(),
            vec!["fresh"]
        );
        thread.close().await.unwrap();
        assert_eq!(*closed.lock().unwrap(), vec!["stale", "new", "fresh"]);
    }

    #[tokio::test]
    async fn rejected_reconfiguration_and_duplicate_batches_retain_all_candidate_resources_until_close()
     {
        let thread =
            ThreadHandle::start("resources".into(), DynModelSession::new(NoModel)).unwrap();
        let closed = Arc::new(Mutex::new(Vec::new()));
        thread
            .register_tools(vec![registration("old", "old", &closed)])
            .await
            .unwrap();
        let before = thread.snapshot();
        assert!(
            thread
                .reconfigure(update(
                    before.commit_sequence + 1,
                    vec![registration("stale", "stale", &closed)]
                ))
                .await
                .is_err()
        );
        let mut invalid = update(
            before.commit_sequence,
            vec![registration("invalid", "invalid-context", &closed)],
        );
        invalid.context = Some(ReplaceContext {
            expected_revision: before.context.revision + 1,
            reason: ContextReplacementReason::Rebuild,
            records: vec![],
        });
        assert!(thread.reconfigure(invalid).await.is_err());
        assert!(
            thread
                .register_tools(vec![
                    registration("duplicate", "duplicate-first", &closed),
                    registration("duplicate", "duplicate-second", &closed),
                    registration("tail", "tail-after-error", &closed)
                ])
                .await
                .is_err()
        );
        assert_eq!(thread.snapshot().commit_sequence, before.commit_sequence);
        assert!(
            closed.lock().unwrap().is_empty(),
            "rejected candidates remain owned, not silently dropped"
        );
        thread.close().await.unwrap();
        let mut actual = closed.lock().unwrap().clone();
        actual.sort_unstable();
        assert_eq!(
            actual,
            vec![
                "duplicate-first",
                "duplicate-second",
                "invalid-context",
                "old",
                "stale",
                "tail-after-error"
            ]
        );
    }
    #[tokio::test]
    async fn closed_mailbox_closes_unsent_registration_and_reconfiguration_candidates() {
        let thread = ThreadHandle::start("closed".into(), DynModelSession::new(NoModel)).unwrap();
        thread.close().await.unwrap();
        let closed = Arc::new(Mutex::new(Vec::new()));
        assert!(matches!(
            thread
                .register_tools(vec![registration("tool", "register", &closed)])
                .await,
            Err(ThreadError::Closed)
        ));
        assert!(matches!(
            thread
                .reconfigure(update(
                    0,
                    vec![registration("tool", "reconfigure", &closed)]
                ))
                .await,
            Err(ThreadError::Closed)
        ));
        assert_eq!(*closed.lock().unwrap(), vec!["register", "reconfigure"]);
    }

    #[derive(Debug)]
    struct RetryClosingTool(Arc<std::sync::atomic::AtomicUsize>);
    impl Tool for RetryClosingTool {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            Err(ToolError::new(std::io::Error::other("not installed")))
        }
        async fn close(&self) -> Result<(), ToolError> {
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                return Err(ToolError::new(std::io::Error::other("retry cleanup")));
            }
            Ok(())
        }
    }
    #[tokio::test]
    async fn failed_uninstalled_cleanup_returns_original_instances_for_retry_without_double_close()
    {
        let thread = ThreadHandle::start("closed".into(), DynModelSession::new(NoModel)).unwrap();
        thread.close().await.unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool = Registration::new(
            "retry".into(),
            OpaquePayload::text("retry"),
            RetryClosingTool(calls.clone()),
        )
        .unwrap();
        let Err(ThreadError::RejectedTools(mut retained)) = thread.register_tools(vec![tool]).await
        else {
            panic!("failed cleanup must return its retained owner");
        };
        assert!(matches!(retained.rejection(), Some(ThreadError::Closed)));
        assert!(retained.to_string().contains("retry cleanup"));
        retained.retry_close().await.unwrap();
        retained.retry_close().await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stale_catalog_preserves_both_conflict_and_failed_cleanup_owner_for_retry() {
        let thread = ThreadHandle::start("conflict".into(), DynModelSession::new(NoModel)).unwrap();
        thread
            .mutate_extensions(vec![extensions::ExtensionMutation::Put {
                id: "producer.mode".into(),
                expected_revision: None,
                payload: OpaquePayload::text("task"),
            }])
            .await
            .unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tool = Registration::new(
            "retry".into(),
            OpaquePayload::text("retry"),
            RetryClosingTool(calls.clone()),
        )
        .unwrap();
        let Err(ThreadError::RejectedTools(mut retained)) =
            thread.register_tools_if_extensions(0, vec![tool]).await
        else {
            panic!("cleanup failure must retain ownership and the original conflict");
        };
        assert!(matches!(
            retained.rejection(),
            Some(ThreadError::ExtensionSequenceConflict {
                expected: 0,
                actual: 1
            })
        ));
        assert!(retained.to_string().contains("retry cleanup"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        retained.retry_close().await.unwrap();
        retained.retry_close().await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        thread.close().await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    struct PausedClose {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl ModelSession for PausedClose {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            NoModel.prepare(request).await
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(())
        }
    }
    #[tokio::test]
    async fn owner_closing_rejects_candidates_back_to_the_sender_for_cleanup() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "closing".into(),
            DynModelSession::new(PausedClose {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let handle = thread.clone();
        let closing = tokio::spawn(async move { handle.close().await });
        started.notified().await;
        let closed = Arc::new(Mutex::new(Vec::new()));
        assert!(matches!(
            thread
                .register_tools(vec![registration("tool", "during-close", &closed)])
                .await,
            Err(ThreadError::Closed)
        ));
        assert!(matches!(
            thread
                .reconfigure(update(
                    0,
                    vec![registration("tool", "reconfigure-during-close", &closed)]
                ))
                .await,
            Err(ThreadError::Closed)
        ));
        assert_eq!(
            *closed.lock().unwrap(),
            vec!["during-close", "reconfigure-during-close"]
        );
        release.notify_one();
        closing.await.unwrap().unwrap();
    }
}
