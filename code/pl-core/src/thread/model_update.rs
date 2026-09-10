//! Deferred model configuration is consumed only at the next Turn boundary.
use super::*;

/// A host-frozen configuration; identity is compared without interpreting provider parameters.
#[derive(Debug, Clone)]
pub(super) struct ModelUpdate {
    pub(super) identity: String,
    pub(super) factory: crate::model::ModelFactory,
    pub(super) preparation: Option<context_preparation::ContextPreparer>,
}
impl Owner {
    pub(super) fn queue_model_update(&mut self, update: ModelUpdate) -> Result<(), ThreadError> {
        if update.identity.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if self.model_identity.as_deref() == Some(&update.identity) {
            self.pending_model_update = None;
        } else {
            self.pending_model_update = Some(update);
        }
        Ok(())
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
            max_model_steps: std::num::NonZeroU32::new(1).unwrap(),
            cancellation: Default::default(),
        }
    }

    #[tokio::test]
    async fn updates_acknowledge_during_execution_apply_at_next_turn_and_deduplicate_fingerprints()
    {
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
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            thread.queue_model_update("new-config".into(), factory.clone(), None),
        )
        .await
        .unwrap()
        .unwrap();
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
                max_model_steps: std::num::NonZeroU32::new(1).unwrap(),
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
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
