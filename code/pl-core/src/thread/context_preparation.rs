//! Optional host-owned context transformation before an ordinary model request is admitted.
use super::*;
use extensions::{ExtensionMutation, ExtensionRecord};
use std::{collections::BTreeMap, fmt, future::Future};

/// Frozen committed context and application versions. New input has not been consumed yet.
#[derive(Debug, Clone)]
pub struct ContextPreparationRequest {
    pub model: ModelRequest,
    pub extensions: Arc<BTreeMap<String, ExtensionRecord>>,
    pub extension_sequence: u64,
    pub previous_usage: Option<crate::model::ModelUsage>,
}

/// Prepared replacement and producer facts. Successful replacement and receipts commit together.
#[derive(Debug)]
pub enum ContextPreparation {
    Unchanged,
    Replaced {
        replacement: ReplaceContext,
        mutations: Vec<ExtensionMutation>,
    },
    Failed {
        error: ModelError,
        mutations: Vec<ExtensionMutation>,
    },
}

/// An optional context policy; implementation owns model protocols and its reduction algorithm.
pub trait ContextPreparationHook: Send + Sync + fmt::Debug + 'static {
    /// Produces a candidate without mutating its caller. Must finish cleanup on cancellation.
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> impl Future<Output = ContextPreparation> + Send;
}
trait ErasedHook: Send + Sync + fmt::Debug {
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> futures::future::BoxFuture<'_, ContextPreparation>;
}
impl<T: ContextPreparationHook> ErasedHook for T {
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> futures::future::BoxFuture<'_, ContextPreparation> {
        Box::pin(ContextPreparationHook::before_step(self, request))
    }
}

/// Shareable immutable policy. Invocation state belongs to the Thread-owned awaited future.
#[derive(Debug, Clone)]
pub struct ContextPreparer(Arc<dyn ErasedHook>);
impl ContextPreparer {
    /// Erases a policy without invoking it or opening model resources.
    pub fn new(hook: impl ContextPreparationHook) -> Self {
        Self(Arc::new(hook))
    }
}

impl Owner {
    pub(super) async fn prepare_context(
        &mut self,
        input: &StepInput,
        tools: Arc<[ModelToolDeclaration]>,
    ) -> Result<(), ThreadError> {
        let Some(hook) = self.context_preparation.clone() else {
            return Ok(());
        };
        let request = ContextPreparationRequest {
            model: ModelRequest {
                tool_call_mode: self.tools.freeze().call_mode(),
                solo_tool_ids: self.tools.freeze().solo_tool_ids(),
                thread_id: self.id.clone(),
                turn_id: input.turn_id.clone(),
                attempt_id: input.attempt_id.clone(),
                context: self.state.context.clone(),
                tools,
                committed_private_context: self.state.private_context.clone(),
                resources: self.resources.clone(),
                cancellation: input.cancellation.clone(),
                progress: None,
            },
            extensions: Arc::new(self.state.extensions.clone()),
            extension_sequence: self.state.extension_sequence,
            previous_usage: self
                .state
                .attempts
                .last()
                .and_then(RequestAttempt::usage)
                .cloned(),
        };
        let result = self
            .await_with_mailbox(crate::error_record::catch_boundary(
                "context preparation",
                hook.0.before_step(request),
            ))
            .await
            .map_err(|source| {
                ThreadError::Model(Arc::new(ModelError {
                    kind: crate::model::ModelFailureKind::ImplementationPanicked,
                    details: None,
                    usage: Default::default(),
                    source: Some(Box::new(source)),
                }))
            })?;
        let (replacement, mutations, failure) = match result {
            ContextPreparation::Unchanged => return Ok(()),
            ContextPreparation::Replaced {
                replacement,
                mutations,
            } => (Some(replacement), mutations, None),
            ContextPreparation::Failed { error, mutations } => (None, mutations, Some(error)),
        };
        let mut candidate = self.state.clone();
        extensions::stage_extensions(&mut candidate, mutations)?;
        let cancelled = input.cancellation.is_cancelled() || self.interrupt.is_closing();
        let replacement_error = if !cancelled {
            replacement.and_then(|replacement| {
                super::replacement::stage_replacement(&mut candidate, replacement).err()
            })
        } else {
            None
        };
        // A rejected or cancelled replacement must still retain actual producer accounting.
        self.state = candidate;
        self.publish();
        if cancelled {
            return Err(ThreadError::Cancelled);
        }
        if let Some(error) = replacement_error {
            return Err(error);
        }
        if let Some(error) = failure {
            return Err(ThreadError::Model(Arc::new(error)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelSession, PreparedModelCall};
    use pretty_assertions::assert_eq;

    struct Echo;
    impl ModelSession for Echo {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: "answer".into(),
                    }],
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
    #[derive(Debug)]
    struct Reduction {
        started: Arc<tokio::sync::Notify>,
        wait_for_cancellation: bool,
    }
    impl ContextPreparationHook for Reduction {
        async fn before_step(&self, request: ContextPreparationRequest) -> ContextPreparation {
            self.started.notify_one();
            if self.wait_for_cancellation {
                request.model.cancellation.cancelled().await;
            }
            ContextPreparation::Replaced {
                replacement: ReplaceContext {
                    expected_revision: request.model.context.revision,
                    reason: ContextReplacementReason::Compaction,
                    records: vec![ContextRecord {
                        id: "summary".into(),
                        turn_id: None,
                        source: ContextSource::Runtime {
                            source_id: "summary".into(),
                        },
                        content: vec![ContextContent::Text {
                            text: "reduced history".into(),
                        }],
                        tool_calls: vec![],
                    }],
                },
                mutations: vec![ExtensionMutation::Put {
                    id: "accounting".into(),
                    expected_revision: None,
                    payload: OpaquePayload::text("service usage: 7"),
                }],
            }
        }
    }
    async fn thread() -> ThreadHandle {
        let thread = ThreadHandle::start("hook-test".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .replace_context(ReplaceContext {
                expected_revision: 0,
                reason: ContextReplacementReason::Rebuild,
                records: vec![ContextRecord {
                    id: "old".into(),
                    turn_id: None,
                    source: ContextSource::User,
                    content: vec![ContextContent::Text {
                        text: "old history".into(),
                    }],
                    tool_calls: vec![],
                }],
            })
            .await
            .unwrap();
        thread
    }
    #[tokio::test]
    async fn automatic_replacement_and_accounting_commit_together_before_new_input_admission() {
        let thread = thread().await;
        thread
            .set_context_preparation(Some(ContextPreparer::new(Reduction {
                started: Default::default(),
                wait_for_cancellation: false,
            })))
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: "new input".into(),
                }],
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.attempts[0].input.records[0].id, "summary");
        assert_eq!(
            snapshot.attempts[0].input.records[1].content,
            vec![ContextContent::Text {
                text: "new input".into()
            }]
        );
        let journal = thread.journal().await.unwrap();
        let mut first = None;
        for end in 1..=journal.len() {
            let state = journal::replay(&journal[..end]).unwrap();
            if state.extensions.contains_key("accounting") {
                first = Some(state);
                break;
            }
        }
        let state = first.unwrap();
        assert_eq!(state.context.records[0].id, "summary");
        assert!(
            state.attempts.is_empty(),
            "preparation must commit before the main request"
        );
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn cancelled_reduction_keeps_observed_accounting_without_consuming_input_or_replacing_history()
     {
        let thread = thread().await;
        let before = thread.snapshot().context;
        let started = Arc::new(tokio::sync::Notify::new());
        thread
            .set_context_preparation(Some(ContextPreparer::new(Reduction {
                started: started.clone(),
                wait_for_cancellation: true,
            })))
            .await
            .unwrap();
        let cancellation = CancellationToken::new();
        let handle = thread.clone();
        let token = cancellation.clone();
        let step = tokio::spawn(async move {
            handle
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: vec![ContextContent::Text {
                        text: "new input".into(),
                    }],
                    cancellation: token,
                })
                .await
        });
        started.notified().await;
        cancellation.cancel();
        assert!(matches!(step.await.unwrap(), Err(ThreadError::Cancelled)));
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.context, before);
        assert!(snapshot.attempts.is_empty());
        assert_eq!(
            snapshot.extensions["accounting"].payload.content(),
            "service usage: 7"
        );
        thread.close().await.unwrap();
    }
}
