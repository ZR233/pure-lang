//! Deferred model configuration is consumed only at the next Turn boundary.
use super::*;

/// A host-frozen configuration; identity is compared without interpreting provider parameters.
#[derive(Debug, Clone)]
pub struct DeferredModelUpdate {
    identity: String,
    factory: crate::model::ModelFactory,
    preparation: Option<context_preparation::ContextPreparer>,
}

/// Host-owned facts that must still match before a deferred model binding is queued.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredModelUpdatePrecondition {
    commit_sequence: Option<u64>,
    extensions: Vec<(String, Option<u64>)>,
}

impl DeferredModelUpdatePrecondition {
    /// Requires the journal watermark to match before queueing the binding.
    pub fn commit_sequence(commit_sequence: u64) -> Self {
        Self {
            commit_sequence: Some(commit_sequence),
            extensions: Vec::new(),
        }
    }

    /// Requires one application extension to retain the observed revision or absence.
    pub fn extension(id: impl Into<String>, revision: Option<u64>) -> Self {
        Self {
            commit_sequence: None,
            extensions: vec![(id.into(), revision)],
        }
    }
}

impl DeferredModelUpdate {
    /// Freezes one host-selected model binding for application at the next Turn boundary.
    pub fn new(
        identity: String,
        factory: crate::model::ModelFactory,
        preparation: Option<context_preparation::ContextPreparer>,
    ) -> Self {
        Self {
            identity,
            factory,
            preparation,
        }
    }
}
impl Owner {
    pub(super) fn pending_model_update(
        &self,
        update: DeferredModelUpdate,
    ) -> Result<Option<DeferredModelUpdate>, ThreadError> {
        if update.identity.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if self.model_identity.as_deref() == Some(&update.identity) {
            Ok(None)
        } else {
            Ok(Some(update))
        }
    }
    pub(super) fn queue_model_update(
        &mut self,
        update: DeferredModelUpdate,
        precondition: DeferredModelUpdatePrecondition,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        let pending = self.pending_model_update(update)?;
        if let Some(expected) = precondition.commit_sequence
            && self.state.commit_sequence != expected
        {
            return Err(ThreadError::ContextConflict {
                expected,
                actual: self.state.commit_sequence,
            });
        }
        for (id, expected) in precondition.extensions {
            if id.is_empty() {
                return Err(ThreadError::InvalidIdentity);
            }
            let actual = self.state.extensions.get(&id).map(|record| record.revision);
            if actual != expected {
                return Err(ThreadError::ExtensionConflict {
                    id,
                    expected,
                    actual,
                });
            }
        }
        let mut candidate = self.state.clone();
        extensions::stage_extensions(&mut candidate, mutations)?;
        self.state = candidate;
        self.pending_model_update = pending;
        self.publish();
        Ok(self.state.clone())
    }
    pub(super) async fn apply_model_update(&mut self) -> Result<(), ThreadError> {
        let Some(update) = self.pending_model_update.take() else {
            return Ok(());
        };
        if self.model_identity.as_deref() == Some(&update.identity) {
            return Ok(());
        }
        // While cleanup/open is in flight, reverting to the previous identity is a real update.
        self.model_identity = None;
        match self.replace_model(update.factory.clone()).await {
            Ok(()) => {
                self.model_identity = Some(update.identity);
                self.context_preparation = update.preparation;
                Ok(())
            }
            Err(error) => {
                // A newer configuration can supersede a failed update while cleanup is awaited.
                if self.pending_model_update.is_none() {
                    self.pending_model_update = Some(update);
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Model, ModelSession, PreparedModelCall};
    use crate::thread::tests::history_snapshot;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct LabeledModel {
        label: &'static str,
        opened: Arc<AtomicUsize>,
    }
    impl Model for LabeledModel {
        async fn open_session(&self) -> Result<DynModelSession, ModelError> {
            self.opened.fetch_add(1, Ordering::SeqCst);
            Ok(DynModelSession::new(Session {
                label: self.label,
                gate: None,
            }))
        }
    }
    struct Session {
        label: &'static str,
        gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    }
    impl ModelSession for Session {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let label = self.label;
            let gate = self.gate.clone();
            Ok(PreparedModelCall::new(async move {
                if let Some((started, release)) = gate {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text { text: label.into() }],
                    tool_calls: vec![],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    fn input(id: &str) -> TurnInput {
        TurnInput {
            turn_id: id.into(),
            attempt_prefix: id.into(),
            content: vec![ContextContent::Text {
                text: "input".into(),
            }],
            max_model_steps: crate::thread::ModelStepLimit::Limited(
                std::num::NonZeroU32::new(1).unwrap(),
            ),
            cancellation: Default::default(),
        }
    }

    #[tokio::test]
    async fn runtime_facts_acknowledge_during_model_execution_without_changing_admitted_input() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "facts".into(),
            DynModelSession::new(Session {
                label: "old",
                gate: Some((started.clone(), release.clone())),
            }),
        )
        .unwrap();
        let owner = thread.clone();
        let first = tokio::spawn(async move { owner.run_turn(input("first")).await });
        started.notified().await;
        let admitted = thread.snapshot().context;
        let fact = RuntimeFact {
            source_id: "skills".into(),
            content: vec![ContextContent::Text {
                text: "updated".into(),
            }],
        };
        let queued = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            thread.queue_runtime_facts(vec![fact.clone()]),
        )
        .await;
        assert!(
            queued.is_ok(),
            "runtime fact refresh blocked behind the running Turn"
        );
        queued.unwrap().unwrap();
        let latest = RuntimeFact {
            source_id: "skills".into(),
            content: vec![ContextContent::Text {
                text: "latest".into(),
            }],
        };
        thread
            .queue_runtime_facts(vec![latest.clone()])
            .await
            .unwrap();
        assert!(matches!(
            thread.queue_runtime_facts(vec![fact.clone(), fact]).await,
            Err(ThreadError::InvalidContext)
        ));
        assert_eq!(thread.snapshot().context, admitted);
        release.notify_one();
        first.await.unwrap().unwrap();
        release.notify_one();
        thread.run_turn(input("second")).await.unwrap();
        assert_eq!(
            thread.snapshot().runtime_facts.as_ref(),
            std::slice::from_ref(&latest)
        );
        thread.close().await.unwrap();
        assert!(matches!(
            thread.queue_runtime_facts(vec![latest]).await,
            Err(ThreadError::Closed)
        ));
    }

    #[tokio::test]
    async fn updates_and_extensions_commit_during_execution_and_apply_model_at_next_turn() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "configuration".into(),
            DynModelSession::new(Session {
                label: "old",
                gate: Some((started.clone(), release.clone())),
            }),
        )
        .unwrap();
        let owner = thread.clone();
        let first = tokio::spawn(async move { owner.run_turn(input("first")).await });
        started.notified().await;
        let opened = Arc::new(AtomicUsize::new(0));
        let factory = crate::model::ModelFactory::new(LabeledModel {
            label: "new",
            opened: opened.clone(),
        });
        let updated = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            thread.queue_model_update_with_extensions(
                "new-config".into(),
                factory.clone(),
                None,
                vec![extensions::ExtensionMutation::Put {
                    id: "model-route".into(),
                    expected_revision: None,
                    payload: OpaquePayload::text("new-config"),
                }],
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            updated.extensions["model-route"].payload.content(),
            "new-config"
        );
        assert_eq!(opened.load(Ordering::SeqCst), 0);
        release.notify_one();
        let first = first.await.unwrap().unwrap();
        assert_eq!(
            first.last_output.content[0],
            ContextContent::Text { text: "old".into() }
        );
        let second = thread.run_turn(input("second")).await.unwrap();
        assert_eq!(
            second.last_output.content[0],
            ContextContent::Text { text: "new".into() }
        );
        thread
            .queue_model_update("new-config".into(), factory, None)
            .await
            .unwrap();
        thread.run_turn(input("third")).await.unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), 1);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn extension_conflict_does_not_leak_pending_model_update() {
        let thread = ThreadHandle::start(
            "configuration-conflict".into(),
            DynModelSession::new(Session {
                label: "old",
                gate: None,
            }),
        )
        .unwrap();
        let opened = Arc::new(AtomicUsize::new(0));
        let error = thread
            .queue_model_update_with_extensions(
                "rejected-config".into(),
                crate::model::ModelFactory::new(LabeledModel {
                    label: "rejected",
                    opened: opened.clone(),
                }),
                None,
                vec![extensions::ExtensionMutation::Put {
                    id: "model-route".into(),
                    expected_revision: Some(42),
                    payload: OpaquePayload::text("rejected-config"),
                }],
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ThreadError::ExtensionConflict {
                expected: Some(42),
                actual: None,
                ..
            }
        ));
        let turn = thread.run_turn(input("after-conflict")).await.unwrap();
        assert_eq!(
            turn.last_output.content[0],
            ContextContent::Text { text: "old".into() }
        );
        assert_eq!(opened.load(Ordering::SeqCst), 0);
        assert!(!thread.snapshot().extensions.contains_key("model-route"));
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn stale_snapshot_does_not_replace_the_latest_pending_model_update() {
        let thread = ThreadHandle::start(
            "configuration-version-conflict".into(),
            DynModelSession::new(Session {
                label: "old",
                gate: None,
            }),
        )
        .unwrap();
        let stale = DeferredModelUpdatePrecondition::extension("model-route", None);
        thread
            .queue_model_update_with_extensions(
                "latest-config".into(),
                crate::model::ModelFactory::new(LabeledModel {
                    label: "latest",
                    opened: Arc::new(AtomicUsize::new(0)),
                }),
                None,
                vec![extensions::ExtensionMutation::Put {
                    id: "model-route".into(),
                    expected_revision: None,
                    payload: OpaquePayload::text("latest-config"),
                }],
            )
            .await
            .unwrap();

        let error = thread
            .queue_deferred_model_update_if_current(
                DeferredModelUpdate::new(
                    "stale-config".into(),
                    crate::model::ModelFactory::new(LabeledModel {
                        label: "stale",
                        opened: Arc::new(AtomicUsize::new(0)),
                    }),
                    None,
                ),
                stale,
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                ThreadError::ExtensionConflict {
                    ref id,
                    expected: None,
                    actual: Some(1)
                } if id == "model-route"
            ),
            "unexpected stale snapshot error: {error:?}"
        );

        let turn = thread.run_turn(input("after-stale-refresh")).await.unwrap();
        assert_eq!(
            turn.last_output.content[0],
            ContextContent::Text {
                text: "latest".into()
            }
        );
        thread.close().await.unwrap();
    }
    struct PausedOpen {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl Model for PausedOpen {
        async fn open_session(&self) -> Result<DynModelSession, ModelError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(DynModelSession::new(Session {
                label: "explicit",
                gate: None,
            }))
        }
    }

    #[tokio::test]
    async fn configuration_queued_during_explicit_model_open_is_used_by_the_next_turn() {
        let thread = ThreadHandle::start(
            "update-during-open".into(),
            DynModelSession::new(Session {
                label: "old",
                gate: None,
            }),
        )
        .unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let opening = {
            let thread = thread.clone();
            let factory = crate::model::ModelFactory::new(PausedOpen {
                started: started.clone(),
                release: release.clone(),
            });
            tokio::spawn(async move { thread.replace_model(factory).await })
        };
        started.notified().await;
        thread
            .queue_model_update(
                "latest".into(),
                crate::model::ModelFactory::new(LabeledModel {
                    label: "latest",
                    opened: Arc::new(AtomicUsize::new(0)),
                }),
                None,
            )
            .await
            .unwrap();
        release.notify_one();
        opening.await.unwrap().unwrap();
        thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: crate::thread::ModelStepLimit::Limited(
                    std::num::NonZeroU32::new(1).unwrap(),
                ),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let snapshot = history_snapshot(&thread).await;
        let AttemptOutcome::Committed(output) = &snapshot.attempts[0].outcome else {
            panic!("committed latest binding response");
        };
        assert_eq!(
            output.content,
            vec![ContextContent::Text {
                text: "latest".into()
            }]
        );
        thread.close().await.unwrap();
    }
}
