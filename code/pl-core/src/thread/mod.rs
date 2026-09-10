//! Serial Thread ownership over protocol-independent model context and committed private material.

use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};
mod background;
mod cancellation;
pub mod cold;
pub mod context_preparation;
pub mod extensions;
mod facts;
pub mod inbox;
pub mod input;
pub mod interactions;
pub mod journal;
mod owner;
pub mod permissions;
mod recovery;
mod subscription;
pub mod task;
mod task_access;
pub use task_access::{TaskAccess, TaskWaitSnapshot};
mod turn;
use owner::{Owner, PendingCall};
pub use subscription::ThreadSubscription;
use tokio_util::sync::CancellationToken;

use crate::context::{
    ContextContent, ContextRecord, ContextSnapshot, ContextSource, OpaquePayload,
};
use crate::model::{
    DynModelSession, ModelError, ModelRequest, ModelStepOutput, ModelToolDeclaration,
};

mod handle;
mod reconfiguration;
pub use reconfiguration::IdleReconfiguration;
mod mailbox;
mod model_step;
mod model_update;
mod replacement;
mod tool_execution;
mod types;
use handle::Command;
pub use handle::ThreadHandle;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelSession, PreparedModelCall};

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
                        text: Arc::from("response"),
                    }],
                    tool_calls: Vec::new(),
                    private_context: Some(OpaquePayload::text("next private state")),
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn model_context_and_private_material_commit_together_and_cancelled_input_is_not_consumed()
     {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let input = |attempt: &str, cancellation| StepInput {
            turn_id: "turn".into(),
            attempt_id: attempt.into(),
            content: vec![ContextContent::Text {
                text: Arc::from("user"),
            }],
            cancellation,
        };
        assert!(matches!(
            thread.step(input("cancelled", cancellation)).await,
            Err(ThreadError::Cancelled)
        ));
        assert!(thread.snapshot().context.records.is_empty());
        thread
            .step(input("accepted", CancellationToken::new()))
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.context.records.len(), 2);
        assert_eq!(
            snapshot.private_context.unwrap().content(),
            "next private state"
        );
        assert!(matches!(
            snapshot.attempts[0].outcome,
            AttemptOutcome::Committed(_)
        ));
        assert!(matches!(
            thread
                .step(input("accepted", CancellationToken::new()))
                .await,
            Err(ThreadError::InvalidIdentity)
        ));
        thread.close().await.unwrap();
    }
    struct FailFirstClose(bool);
    impl ModelSession for FailFirstClose {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Echo.prepare(request).await
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            if !self.0 {
                self.0 = true;
                Err(ModelError {
                    details: None,
                    kind: crate::model::ModelFailureKind::Unavailable,
                    usage: Default::default(),
                    source: None,
                })
            } else {
                Ok(())
            }
        }
    }

    struct EchoFactory {
        opens: Arc<std::sync::atomic::AtomicUsize>,
        fail: bool,
    }
    impl crate::model::Model for EchoFactory {
        async fn open_session(&self) -> Result<DynModelSession, ModelError> {
            self.opens.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail {
                return Err(ModelError {
                    details: None,
                    kind: crate::model::ModelFailureKind::Unavailable,
                    usage: Default::default(),
                    source: None,
                });
            }
            Ok(DynModelSession::new(Echo))
        }
    }

    #[tokio::test]
    async fn model_replacement_retains_history_and_retries_failed_cleanup_before_opening_new_session()
     {
        let thread =
            ThreadHandle::start("thread".into(), DynModelSession::new(FailFirstClose(false)))
                .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("original input"),
                }],
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let context = thread.snapshot().context;
        let opens = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let factory = crate::model::ModelFactory::new(EchoFactory {
            opens: opens.clone(),
            fail: false,
        });
        assert!(matches!(
            thread.replace_model(factory.clone()).await,
            Err(ThreadError::Model(_))
        ));
        assert!(!thread.snapshot().model_available);
        assert_eq!(opens.load(std::sync::atomic::Ordering::SeqCst), 0);
        thread.replace_model(factory).await.unwrap();
        assert!(thread.snapshot().model_available);
        assert_eq!(opens.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(thread.snapshot().context, context);
        assert!(thread.snapshot().private_context.is_none());
        let failing = crate::model::ModelFactory::new(EchoFactory {
            opens: opens.clone(),
            fail: true,
        });
        assert!(thread.replace_model(failing).await.is_err());
        assert!(!thread.snapshot().model_available);
        assert!(matches!(
            thread
                .step(StepInput {
                    turn_id: "next".into(),
                    attempt_id: "not-admitted".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new()
                })
                .await,
            Err(ThreadError::ModelUnavailable)
        ));
        assert_eq!(thread.snapshot().attempts.len(), 1);
        thread
            .replace_model(crate::model::ModelFactory::new(EchoFactory {
                opens,
                fail: false,
            }))
            .await
            .unwrap();
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.context, context);
        assert!(!replayed.model_available);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn failed_close_keeps_retry_owner_but_rejects_new_execution() {
        let thread =
            ThreadHandle::start("thread".into(), DynModelSession::new(FailFirstClose(false)))
                .unwrap();
        assert!(matches!(thread.close().await, Err(ThreadError::Model(_))));
        assert_eq!(thread.snapshot().lifecycle, ThreadLifecycle::Closing);
        assert!(matches!(
            thread.register_tools(Vec::new()).await,
            Err(ThreadError::Closed)
        ));
        assert!(matches!(
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new()
                })
                .await,
            Err(ThreadError::Closed)
        ));
        thread.close().await.unwrap();
        assert_eq!(thread.snapshot().lifecycle, ThreadLifecycle::Closed);
    }
    struct DelayedModel {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }
    impl ModelSession for DelayedModel {
        fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> impl std::future::Future<Output = Result<PreparedModelCall, ModelError>> + Send
        {
            let started = self.started.clone();
            let release = self.release.clone();
            async move {
                Ok(PreparedModelCall::new(async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: vec![ContextContent::Text {
                            text: Arc::from("late response"),
                        }],
                        tool_calls: Vec::new(),
                        private_context: Some(OpaquePayload::text("late private state")),
                        usage: crate::model::ModelUsage {
                            input_tokens: Some(13),
                            ..Default::default()
                        },
                    })
                }))
            }
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn closing_a_thread_commits_pending_question_cancellation_and_keeps_history_readable() {
        let thread = ThreadHandle::start("questions".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .request_interaction(interactions::InteractionRequest {
                id: "question".into(),
                turn_id: "turn".into(),
                payload: OpaquePayload::text("original question"),
            })
            .await
            .unwrap();
        thread.close().await.unwrap();
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(
            replayed.interactions["question"].state,
            interactions::InteractionState::Cancelled
        );
        assert_eq!(
            replayed.interactions["question"].request.payload.content(),
            "original question"
        );
        assert_eq!(replayed.lifecycle, ThreadLifecycle::Closed);
        assert!(replayed.attempts.is_empty());
    }

    #[tokio::test]
    async fn expected_turn_interrupt_rejects_a_stale_identity_and_preserves_late_usage() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "interrupt-owner".into(),
            DynModelSession::new(DelayedModel {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let handle = thread.clone();
        let running = tokio::spawn(async move {
            handle
                .run_turn(TurnInput {
                    turn_id: "actual".into(),
                    attempt_prefix: "attempt".into(),
                    content: Vec::new(),
                    max_model_steps: std::num::NonZeroU32::new(2).unwrap(),
                    cancellation: CancellationToken::new(),
                })
                .await
        });
        started.notified().await;
        assert!(matches!(
            thread.interrupt_turn(Some("stale".into())).await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert_eq!(thread.snapshot().turns[0].state, TurnState::Running);
        assert!(thread.interrupt_turn(Some("actual".into())).await.unwrap());
        release.notify_one();
        assert!(matches!(
            running.await.unwrap(),
            Err(ThreadError::Cancelled)
        ));
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.turns[0].state, TurnState::Cancelled);
        assert!(snapshot.private_context.is_none());
        assert_eq!(snapshot.attempts[0].usage().unwrap().input_tokens, Some(13));
        assert!(!thread.interrupt_turn(Some("actual".into())).await.unwrap());
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn messages_commit_during_model_execution_without_changing_its_frozen_input() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(DelayedModel {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let handle = thread.clone();
        let running = tokio::spawn(async move {
            handle
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: vec![ContextContent::Text {
                        text: Arc::from("user"),
                    }],
                    cancellation: CancellationToken::new(),
                })
                .await
        });
        started.notified().await;
        let admitted = thread.snapshot().context;
        let sequence = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            thread.send_message(inbox::ThreadMessage {
                id: "notice".into(),
                source_id: "child".into(),
                payload: OpaquePayload::text("raw notification"),
                context: vec![ContextContent::Text {
                    text: Arc::from("notification context"),
                }],
            }),
        )
        .await
        .expect("mailbox must progress before the model is released")
        .unwrap();
        assert_eq!(sequence, 1);
        let journal = tokio::time::timeout(std::time::Duration::from_secs(2), thread.journal())
            .await
            .expect("history must remain readable during execution")
            .unwrap();
        let replayed = journal::replay(&journal).unwrap();
        assert_eq!(replayed.context, admitted);
        assert_eq!(replayed.inbox.len(), 1);
        assert_eq!(replayed.consumed_messages, 0);
        release.notify_one();
        running.await.unwrap().unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.attempts[0].input, admitted);
        assert_eq!(snapshot.inbox, replayed.inbox);
        assert_eq!(snapshot.consumed_messages, 0);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn late_cancelled_output_is_retained_as_fact_without_advancing_context_or_continuation() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(DelayedModel {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let cancellation = CancellationToken::new();
        let input = StepInput {
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("user"),
            }],
            cancellation: cancellation.clone(),
        };
        let handle = thread.clone();
        let running = tokio::spawn(async move { handle.step(input).await });
        started.notified().await;
        let admitted = thread.snapshot().context;
        assert!(thread.interrupt());
        assert!(!cancellation.is_cancelled());
        release.notify_one();
        assert!(matches!(
            running.await.unwrap(),
            Err(ThreadError::Cancelled)
        ));
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.context, admitted);
        assert!(snapshot.private_context.is_none());
        assert_eq!(snapshot.attempts[0].usage().unwrap().input_tokens, Some(13));
        let AttemptOutcome::Cancelled {
            result: Ok(observed),
        } = &snapshot.attempts[0].outcome
        else {
            panic!("retain cancelled response");
        };
        assert_eq!(
            observed.private_context.as_ref().unwrap().content(),
            "late private state"
        );
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn context_replacement_keeps_old_versions_and_clears_private_continuation() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("original"),
                }],
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let previous = thread.snapshot();
        let record = ContextRecord {
            tool_calls: Vec::new(),
            id: "summary".into(),
            turn_id: None,
            source: ContextSource::Runtime {
                source_id: "summary".into(),
            },
            content: vec![ContextContent::Text {
                text: Arc::from("saved summary"),
            }],
        };
        assert!(matches!(
            thread
                .replace_context(ReplaceContext {
                    expected_revision: 0,
                    reason: ContextReplacementReason::Compaction,
                    records: vec![record.clone()]
                })
                .await,
            Err(ThreadError::ContextConflict { .. })
        ));
        assert_eq!(thread.snapshot().context, previous.context);
        let current = thread
            .replace_context(ReplaceContext {
                expected_revision: previous.context.revision,
                reason: ContextReplacementReason::Compaction,
                records: vec![record],
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(current.revision, previous.context.revision + 1);
        assert_eq!(snapshot.context_replacements[0].previous, previous.context);
        assert_eq!(
            snapshot.context_replacements[0].previous_private_context,
            previous.private_context
        );
        assert!(snapshot.private_context.is_none());
        assert_eq!(snapshot.context.records.len(), 1);
        let commits = thread.journal().await.unwrap();
        let decoded = commits
            .iter()
            .map(|commit| {
                Arc::new(journal::ThreadCommit::decode(&commit.encode().unwrap()).unwrap())
            })
            .collect::<Vec<_>>();
        assert!(journal::replay(&decoded).unwrap().private_context.is_none());
        assert!(matches!(
            ThreadHandle::restore("wrong-thread".into(), DynModelSession::new(Echo), decoded),
            Err(ThreadError::InvalidIdentity)
        ));
        thread.close().await.unwrap();
    }
    struct ParallelReadCalls {
        mix_control: bool,
    }
    impl ModelSession for ParallelReadCalls {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            assert_eq!(request.tool_call_mode, crate::model::ToolCallMode::Parallel);
            assert_eq!(request.solo_tool_ids.as_ref(), &["control"]);
            let second = if self.mix_control { "control" } else { "read" };
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: ["read", second]
                        .into_iter()
                        .enumerate()
                        .map(|(index, id)| crate::model::ModelToolCall {
                            call_id: format!("call-{index}"),
                            tool_id: id.into(),
                            arguments: OpaquePayload::text("input"),
                        })
                        .collect(),
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn mixed_catalog_accepts_parallel_reads_but_rejects_a_control_in_the_batch() {
        for mix_control in [false, true] {
            let thread = ThreadHandle::start(
                "parallel".into(),
                DynModelSession::new(ParallelReadCalls { mix_control }),
            )
            .unwrap();
            thread
                .register_tools(vec![
                    crate::tool::opaque::Registration::new(
                        "read".into(),
                        OpaquePayload::text("schema"),
                        EchoTool,
                    )
                    .unwrap(),
                    crate::tool::opaque::Registration::new(
                        "control".into(),
                        OpaquePayload::text("schema"),
                        EchoTool,
                    )
                    .unwrap()
                    .foreground(),
                ])
                .await
                .unwrap();
            let result = thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new(),
                })
                .await;
            if mix_control {
                assert!(matches!(result, Err(ThreadError::InvalidOutput)));
            } else {
                assert_eq!(result.unwrap().tool_calls.len(), 2);
            }
            assert!(
                thread.snapshot().deliveries.is_empty(),
                "validation must not execute a rejected call"
            );
            thread.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn independent_runtime_fact_patches_preserve_other_sources_and_clear_only_requested_source()
     {
        let thread = ThreadHandle::start("facts".into(), DynModelSession::new(Echo)).unwrap();
        let fact = |source: &str, text: &str| RuntimeFact {
            source_id: source.into(),
            content: vec![ContextContent::Text { text: text.into() }],
        };
        let workflow = fact("workflow", "new stage");
        let skills = fact("skills", "new catalog");
        let (first, second) = tokio::join!(
            thread.patch_runtime_facts(vec![workflow.clone()]),
            thread.patch_runtime_facts(vec![skills.clone()]),
        );
        first.unwrap();
        second.unwrap();
        assert_eq!(
            thread.snapshot().runtime_facts.as_ref(),
            &[skills, workflow.clone()]
        );
        let clear = RuntimeFact {
            source_id: "skills".into(),
            content: Vec::new(),
        };
        thread
            .patch_runtime_facts(vec![clear.clone()])
            .await
            .unwrap();
        assert_eq!(
            thread.snapshot().runtime_facts.as_ref(),
            &[clear.clone(), workflow]
        );
        let before = thread.snapshot();
        assert!(matches!(
            thread.patch_runtime_facts(vec![clear.clone(), clear]).await,
            Err(ThreadError::InvalidContext)
        ));
        assert_eq!(thread.snapshot().context, before.context);
        thread.patch_runtime_facts(Vec::new()).await.unwrap();
        assert_eq!(thread.snapshot().context, before.context);
        assert_eq!(
            journal::replay(&thread.journal().await.unwrap())
                .unwrap()
                .runtime_facts,
            before.runtime_facts
        );
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn runtime_facts_append_only_on_change_and_return_after_compaction() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let fact = RuntimeFact {
            source_id: "plugin.status".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("current status"),
            }],
        };
        let first = thread.update_facts(vec![fact.clone()]).await.unwrap();
        assert_eq!(
            thread.update_facts(vec![fact.clone()]).await.unwrap(),
            first
        );
        let compacted = thread
            .replace_context(ReplaceContext {
                expected_revision: first.revision,
                reason: ContextReplacementReason::Compaction,
                records: Vec::new(),
            })
            .await
            .unwrap();
        assert_eq!(compacted.records.len(), 1);
        assert_eq!(compacted.records[0].content, fact.content);
        assert!(
            matches!(&compacted.records[0].source, ContextSource::Runtime { source_id } if source_id == "plugin.status")
        );
        let cleared = thread.update_facts(Vec::new()).await.unwrap();
        assert_eq!(cleared.records.len(), 2);
        assert_eq!(thread.update_facts(Vec::new()).await.unwrap(), cleared);
        let replayed = thread
            .replace_context(ReplaceContext {
                expected_revision: cleared.revision,
                reason: ContextReplacementReason::Rewind,
                records: first.records.to_vec(),
            })
            .await
            .unwrap();
        assert_eq!(replayed.records.len(), 2);
        assert_eq!(replayed.records[1].content, cleared.records[1].content);
        assert_eq!(first.records[0].content, fact.content);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn unknown_estimate_is_rejected_before_input_admission_when_exact_capacity_is_required() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .set_capacity(ContextCapacity::RequireExact {
                max_input_tokens: 100,
            })
            .await
            .unwrap();
        let input = || StepInput {
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("input"),
            }],
            cancellation: CancellationToken::new(),
        };
        assert!(matches!(
            thread.step(input()).await,
            Err(ThreadError::UnknownCapacity)
        ));
        assert!(thread.snapshot().context.records.is_empty());
        assert!(thread.snapshot().attempts.is_empty());
        thread
            .set_capacity(ContextCapacity::AllowUnknown {
                max_input_tokens: 100,
            })
            .await
            .unwrap();
        thread.step(input()).await.unwrap();
        assert!(thread.snapshot().attempts[0].input_estimate.is_none());
        thread.close().await.unwrap();
    }

    #[test]
    fn approximate_estimates_are_not_treated_as_exact_or_as_zero() {
        let approximate = Some(crate::model::TokenEstimate {
            tokens: 101,
            accuracy: crate::model::EstimateAccuracy::Approximate,
        });
        assert!(matches!(
            ContextCapacity::RequireExact {
                max_input_tokens: 1000
            }
            .admit(approximate),
            Err(ThreadError::UnknownCapacity)
        ));
        assert!(matches!(
            ContextCapacity::AcceptApproximate {
                max_input_tokens: 100
            }
            .admit(approximate),
            Err(ThreadError::ContextCapacity { .. })
        ));
        assert!(
            ContextCapacity::AcceptApproximate {
                max_input_tokens: 101
            }
            .admit(approximate)
            .is_ok()
        );
        assert!(
            ContextCapacity::AcceptApproximate {
                max_input_tokens: 101
            }
            .admit(None)
            .is_err()
        );
    }
    struct CallsOnce;
    impl ModelSession for CallsOnce {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            request.context.validate_complete().unwrap();
            let calls = if request
                .context
                .records
                .iter()
                .any(|record| matches!(record.source, ContextSource::ToolResult { .. }))
            {
                Vec::new()
            } else {
                vec![crate::model::ModelToolCall {
                    call_id: "call".into(),
                    tool_id: "echo".into(),
                    arguments: OpaquePayload::text("raw input"),
                }]
            };
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: calls,
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
    struct EchoTool;
    impl crate::tool::opaque::Tool for EchoTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            Ok(crate::tool::ToolOutput::new(
                input,
                vec![ContextContent::Text {
                    text: Arc::from("tool result"),
                }],
            ))
        }
    }

    #[derive(Debug)]
    struct ObservedFailureTool;
    impl crate::tool::opaque::Tool for ObservedFailureTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            let observed = crate::tool::ToolOutput::new(
                OpaquePayload::new("plugin.execution", 99, "  full observed result\n").unwrap(),
                vec![ContextContent::Text {
                    text: Arc::from("observed preview"),
                }],
            )
            .ending_turn();
            Err(
                crate::tool::opaque::ToolError::new(std::io::Error::other("archive failed"))
                    .with_output(observed),
            )
        }
    }

    #[tokio::test]
    async fn failed_tool_preserves_observed_payload_without_applying_turn_control() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    ObservedFailureTool,
                )
                .unwrap()
                .with_turn_completion(),
            ])
            .await
            .unwrap();
        let result = thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(result.outcome, TurnOutcome::Completed);
        let snapshot = thread.snapshot();
        assert!(matches!(
            snapshot.deliveries[0].outcome,
            ToolOutcome::Failed(_)
        ));
        assert_eq!(
            snapshot.deliveries[0].output.payload().content(),
            "  full observed result\n"
        );
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.deliveries[0].output, snapshot.deliveries[0].output);
        assert_eq!(replayed.context, snapshot.context);
        thread.close().await.unwrap();
    }

    #[derive(Debug)]
    struct CaptureTaskAccess(Arc<std::sync::Mutex<Option<TaskAccess>>>);
    impl crate::tool::opaque::Tool for CaptureTaskAccess {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            *self.0.lock().unwrap() = context.tasks;
            Ok(crate::tool::ToolOutput::new(input, Vec::new()))
        }
    }

    #[tokio::test]
    async fn task_control_permissions_expire_without_preventing_read_only_inspection() {
        for granted in [false, true] {
            let thread =
                ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
            let captured = Arc::new(std::sync::Mutex::new(None));
            let registration = crate::tool::opaque::Registration::new(
                "echo".into(),
                OpaquePayload::text("schema"),
                CaptureTaskAccess(captured.clone()),
            )
            .unwrap();
            let registration = if granted {
                registration.with_task_cancellation()
            } else {
                registration
            };
            thread.register_tools(vec![registration]).await.unwrap();
            thread
                .run_turn(TurnInput {
                    turn_id: "turn".into(),
                    attempt_prefix: "attempt".into(),
                    content: Vec::new(),
                    max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                    cancellation: CancellationToken::new(),
                })
                .await
                .unwrap();
            let access = captured.lock().unwrap().clone().unwrap();
            let error = access.cancel("task:call".into()).await.unwrap_err();
            if granted {
                assert!(matches!(error, ThreadError::TaskAccessExpired));
            } else {
                assert!(matches!(error, ThreadError::TaskAccessDenied));
            }
            assert_eq!(
                access.get("task:call").unwrap().status,
                task::TaskStatus::Succeeded
            );
            assert!(access.result("task:call").unwrap().is_some());
            thread.close().await.unwrap();
        }
    }

    #[derive(Debug)]
    struct DelayedTool {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        closes: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl crate::tool::opaque::Tool for DelayedTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(crate::tool::ToolOutput::new(
                OpaquePayload::text("original executor"),
                Vec::new(),
            ))
        }
        async fn close(&self) -> Result<(), crate::tool::opaque::ToolError> {
            self.closes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn background_task_survives_its_turn_and_delivers_only_through_a_later_admission() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    DelayedTool {
                        started: Arc::new(tokio::sync::Notify::new()),
                        release: release.clone(),
                        closes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    },
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let origin = CancellationToken::new();
        let result = thread
            .run_turn(TurnInput {
                turn_id: "origin".into(),
                attempt_prefix: "origin".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: origin.clone(),
            })
            .await
            .unwrap();
        assert_eq!(result.outcome, TurnOutcome::Completed);
        let before = thread.snapshot();
        assert_eq!(before.tasks["task:call"].status, task::TaskStatus::Running);
        assert!(before.tasks["task:call"].acknowledgement.is_some());
        let restored = ThreadHandle::restore(
            "thread".into(),
            DynModelSession::new(CallsOnce),
            thread.journal().await.unwrap(),
        )
        .unwrap();
        let interrupted = restored
            .wait_task("task:call", CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(interrupted.outcome, ToolOutcome::Interrupted));
        assert!(matches!(
            interrupted.target,
            ToolDeliveryTarget::Inbox { .. }
        ));
        assert_eq!(restored.snapshot().context, before.context);
        journal::replay(&restored.journal().await.unwrap()).unwrap();
        restored.close().await.unwrap();
        origin.cancel();
        release.notify_one();
        let delivery = thread
            .wait_task("task:call", CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(delivery.outcome, ToolOutcome::Succeeded));
        assert!(matches!(delivery.target, ToolDeliveryTarget::Inbox { .. }));
        assert_eq!(thread.snapshot().context, before.context);
        assert_eq!(thread.snapshot().consumed_messages, 0);
        thread
            .step(StepInput {
                turn_id: "next".into(),
                attempt_id: "next".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(thread.snapshot().consumed_messages, 1);
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.tasks, thread.snapshot().tasks);
        assert_eq!(replayed.context, thread.snapshot().context);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn cancelling_one_task_waits_for_cleanup_without_cancelling_its_turn() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    DelayedTool {
                        started: started.clone(),
                        release: release.clone(),
                        closes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    },
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let cancellation = CancellationToken::new();
        let handle = thread.clone();
        let token = cancellation.clone();
        let running = tokio::spawn(async move {
            handle
                .run_turn(TurnInput {
                    turn_id: "turn".into(),
                    attempt_prefix: "attempt".into(),
                    content: Vec::new(),
                    max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                    cancellation: token,
                })
                .await
        });
        started.notified().await;
        let context = thread.snapshot().context;
        assert_eq!(
            thread.cancel_task("task:call".into()).await.unwrap(),
            task::TaskCancellationReceipt::Requested
        );
        let requested = thread.snapshot();
        assert_eq!(
            requested.tasks["task:call"].status,
            task::TaskStatus::Running
        );
        assert!(requested.tasks["task:call"].cancel_requested);
        assert_eq!(requested.context, context);
        assert!(!cancellation.is_cancelled());
        assert_eq!(
            thread.cancel_task("task:call".into()).await.unwrap(),
            task::TaskCancellationReceipt::AlreadyRequested
        );
        assert_eq!(thread.snapshot().commit_sequence, requested.commit_sequence);
        release.notify_one();
        let completed = running.await.unwrap().unwrap();
        assert_eq!(completed.outcome, TurnOutcome::Completed);
        assert_eq!(
            thread.snapshot().tasks["task:call"].status,
            task::TaskStatus::Cancelled
        );
        assert_eq!(
            thread.cancel_task("task:call".into()).await.unwrap(),
            task::TaskCancellationReceipt::AlreadyFinished
        );
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.tasks, thread.snapshot().tasks);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn running_task_recovery_records_interruption_without_reexecuting_the_call() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    DelayedTool {
                        started: started.clone(),
                        release: release.clone(),
                        closes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    },
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let context = thread.snapshot().context;
        let handle = thread.clone();
        let running = tokio::spawn(async move {
            handle
                .execute_tool("call".into(), CancellationToken::new())
                .await
        });
        started.notified().await;
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.context, context);
        assert_eq!(
            snapshot.tasks["task:call"].status,
            task::TaskStatus::Running
        );
        let saved = thread.journal().await.unwrap();
        let restored =
            ThreadHandle::restore("thread".into(), DynModelSession::new(CallsOnce), saved).unwrap();
        assert_eq!(
            restored.snapshot().tasks["task:call"].status,
            task::TaskStatus::Interrupted
        );
        assert_eq!(restored.snapshot().attempts.len(), snapshot.attempts.len());
        let replayed = journal::replay(&restored.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.tasks, restored.snapshot().tasks);
        assert!(matches!(
            replayed.deliveries[0].outcome,
            ToolOutcome::Interrupted
        ));
        restored.close().await.unwrap();
        release.notify_one();
        running.await.unwrap().unwrap();
        thread
            .wait_task("task:call", CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            thread.snapshot().tasks["task:call"].status,
            task::TaskStatus::Succeeded
        );
        let commits = thread.journal().await.unwrap();
        let terminal = commits.last().unwrap();
        assert_eq!(terminal.tasks[0].status, task::TaskStatus::Succeeded);
        assert_eq!(terminal.deliveries[0].call_id, "call");
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn replacing_tools_during_execution_keeps_the_started_executor_and_updates_next_step() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let closes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("old schema"),
                    DelayedTool {
                        started: started.clone(),
                        release: release.clone(),
                        closes: closes.clone(),
                    },
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "first".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let handle = thread.clone();
        let running = tokio::spawn(async move {
            handle
                .execute_tool("call".into(), CancellationToken::new())
                .await
        });
        started.notified().await;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            thread.register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("new schema"),
                    EchoTool,
                )
                .unwrap(),
            ]),
        )
        .await
        .expect("directory replacement must progress during execution")
        .unwrap();
        assert_eq!(closes.load(std::sync::atomic::Ordering::SeqCst), 0);
        release.notify_one();
        running.await.unwrap().unwrap();
        let output = thread
            .wait_task("task:call", CancellationToken::new())
            .await
            .unwrap()
            .output;
        assert_eq!(output.payload(), &OpaquePayload::text("original executor"));
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "second".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(
            thread.snapshot().attempts[1].tools[0].declaration,
            OpaquePayload::text("new schema")
        );
        thread.close().await.unwrap();
        assert_eq!(closes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn turn_loop_delivers_tools_before_next_inference_without_duplicating_user_input() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    EchoTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let completed = thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("user"),
                }],
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(completed.outcome, TurnOutcome::Completed);
        assert_eq!(completed.model_steps, 2);
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(
            snapshot.turns[0].state,
            TurnState::Finished(TurnOutcome::Completed)
        );
        assert_eq!(snapshot.turns[0].model_steps, 2);
        assert_eq!(
            snapshot
                .context
                .records
                .iter()
                .filter(|record| record.source == ContextSource::User)
                .count(),
            1
        );
        snapshot.context.validate_complete().unwrap();
        assert_eq!(snapshot.deliveries.len(), 1);
        assert_eq!(
            snapshot.deliveries[0].output.payload().content(),
            "raw input"
        );
        let commits = thread.journal().await.unwrap();
        let encoded = commits
            .iter()
            .map(|commit| commit.encode().unwrap())
            .collect::<Vec<_>>();
        let decoded = encoded
            .iter()
            .map(|payload| Arc::new(journal::ThreadCommit::decode(payload).unwrap()))
            .collect::<Vec<_>>();
        let restored = journal::replay(&decoded).unwrap();
        assert_eq!(restored.context, snapshot.context);
        assert_eq!(restored.private_context, snapshot.private_context);
        assert_eq!(restored.commit_sequence, snapshot.commit_sequence);
        assert_eq!(restored.deliveries[0].output, snapshot.deliveries[0].output);
        assert_eq!(restored.attempts.len(), snapshot.attempts.len());
        assert_eq!(restored.turns, snapshot.turns);
        assert_eq!(restored.attempts[1].input, snapshot.attempts[1].input);
        assert!(
            commits
                .iter()
                .filter_map(|commit| commit.context.as_ref())
                .all(|change| matches!(change, journal::ContextChange::Append { .. }))
        );
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn journal_rejects_changed_attempt_identity_and_repeated_terminal_results() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("input"),
                }],
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let commits = thread.journal().await.unwrap();
        journal::replay(&commits).unwrap();
        let mut tampered = commits.clone();
        let terminal = Arc::make_mut(tampered.last_mut().unwrap());
        terminal.attempt.as_mut().unwrap().turn_id = "different-turn".into();
        assert!(matches!(
            journal::replay(&tampered),
            Err(ThreadError::InvalidOutput)
        ));
        let mut repeated = commits.clone();
        let mut extra = (**commits.last().unwrap()).clone();
        extra.sequence += 1;
        extra.context = None;
        repeated.push(Arc::new(extra));
        assert!(matches!(
            journal::replay(&repeated),
            Err(ThreadError::InvalidOutput)
        ));
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct FailingStore(Arc<std::sync::atomic::AtomicBool>);
    impl cold::ColdStore for FailingStore {
        fn admit(&self, _: &str, _: u64, _: OpaquePayload) -> Result<(), cold::ColdStoreError> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Err(cold::ColdStoreError {
                    source: Box::new(std::io::Error::other("storage unavailable")),
                })
            } else {
                Ok(())
            }
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn storage_failure_keeps_runtime_facts_and_flush_retries_without_model_execution() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let failing = Arc::new(std::sync::atomic::AtomicBool::new(true));
        thread
            .attach_storage(cold::ColdStoreHandle::new(FailingStore(failing.clone())))
            .await
            .unwrap();
        thread
            .update_facts(vec![RuntimeFact {
                source_id: "plugin".into(),
                content: vec![ContextContent::Text {
                    text: Arc::from("retained fact"),
                }],
            }])
            .await
            .unwrap();
        let retained = thread.snapshot().context;
        assert!(thread.snapshot().persistence.error.is_some());
        assert!(thread.flush().await.is_err());
        failing.store(false, std::sync::atomic::Ordering::SeqCst);
        thread.flush().await.unwrap();
        let restored = thread.snapshot();
        assert_eq!(restored.context, retained);
        assert_eq!(
            restored.persistence.durable_sequence,
            restored.commit_sequence
        );
        assert!(restored.attempts.is_empty());
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct FlushFailure(Arc<std::sync::atomic::AtomicBool>);
    impl cold::ColdStore for FlushFailure {
        fn admit(&self, _: &str, _: u64, _: OpaquePayload) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Err(cold::ColdStoreError {
                    source: Box::new(std::io::Error::other("flush unavailable")),
                })
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn close_is_not_confirmed_until_the_terminal_commit_is_durable() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let failure = Arc::new(std::sync::atomic::AtomicBool::new(true));
        thread
            .attach_storage(cold::ColdStoreHandle::new(FlushFailure(failure.clone())))
            .await
            .unwrap();
        assert!(matches!(thread.close().await, Err(ThreadError::Storage(_))));
        let pending = thread.snapshot();
        assert_eq!(pending.lifecycle, ThreadLifecycle::Closing);
        assert!(pending.persistence.durable_sequence < pending.commit_sequence);
        assert!(pending.persistence.error.is_some());
        let commits = thread.journal().await.unwrap();
        failure.store(false, std::sync::atomic::Ordering::SeqCst);
        thread.close().await.unwrap();
        let closed = thread.snapshot();
        assert_eq!(closed.lifecycle, ThreadLifecycle::Closed);
        assert_eq!(closed.persistence.durable_sequence, closed.commit_sequence);
        assert_eq!(closed.commit_sequence, commits.len() as u64);
    }
    #[derive(Debug)]
    struct PressureStore(Arc<std::sync::atomic::AtomicU64>);
    impl cold::ColdStore for PressureStore {
        fn pressure(&self, _: &str) -> cold::StoragePressure {
            let bytes = self.0.load(std::sync::atomic::Ordering::SeqCst);
            cold::StoragePressure {
                thread_bytes: bytes,
                store_bytes: bytes,
                error: None,
            }
        }
        fn admit(&self, _: &str, _: u64, _: OpaquePayload) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn cold_pressure_pauses_admission_until_the_recovery_watermark() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let bytes = Arc::new(std::sync::atomic::AtomicU64::new(64 * 1024 * 1024));
        thread
            .attach_storage(cold::ColdStoreHandle::new(PressureStore(bytes.clone())))
            .await
            .unwrap();
        let input = || StepInput {
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        };
        assert!(matches!(
            thread.step(input()).await,
            Err(ThreadError::StoragePressure)
        ));
        bytes.store(48 * 1024 * 1024, std::sync::atomic::Ordering::SeqCst);
        assert!(matches!(
            thread.step(input()).await,
            Err(ThreadError::StoragePressure)
        ));
        assert!(thread.snapshot().attempts.is_empty());
        thread.flush().await.unwrap();
        bytes.store(32 * 1024 * 1024, std::sync::atomic::Ordering::SeqCst);
        thread.step(input()).await.unwrap();
        assert!(!thread.snapshot().persistence.pressure_paused);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn recovery_settles_unexecuted_tool_calls_without_replaying_side_effects() {
        let original =
            ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        original
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    EchoTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        original
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert!(original.snapshot().deliveries.is_empty());
        let commits = original.journal().await.unwrap();
        original.close().await.unwrap();
        let restored =
            ThreadHandle::restore("thread".into(), DynModelSession::new(Echo), commits).unwrap();
        let snapshot = restored.snapshot();
        snapshot.context.validate_complete().unwrap();
        assert_eq!(snapshot.deliveries.len(), 1);
        assert!(matches!(
            snapshot.deliveries[0].outcome,
            ToolOutcome::Interrupted
        ));
        assert!(snapshot.private_context.is_none());
        assert!(matches!(
            restored
                .execute_tool("call".into(), CancellationToken::new())
                .await,
            Err(ThreadError::MissingCall)
        ));
        let replayed = journal::replay(&restored.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.context, snapshot.context);
        assert!(matches!(
            replayed.deliveries[0].outcome,
            ToolOutcome::Interrupted
        ));
        restored.close().await.unwrap();
    }
    struct FailFirstAttempt(bool);
    impl ModelSession for FailFirstAttempt {
        fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> impl std::future::Future<Output = Result<PreparedModelCall, ModelError>> + Send
        {
            let fail = !self.0;
            self.0 = true;
            async move {
                Ok(PreparedModelCall::new(async move {
                    if fail {
                        return Err(ModelError {
                            details: None,
                            kind: crate::model::ModelFailureKind::Unavailable,
                            usage: Default::default(),
                            source: None,
                        });
                    }
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        tool_calls: Vec::new(),
                        private_context: None,
                        usage: Default::default(),
                    })
                }))
            }
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn explicit_retry_reuses_admitted_input_and_records_attempt_relationship() {
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(FailFirstAttempt(false)),
        )
        .unwrap();
        assert!(matches!(
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "first".into(),
                    content: vec![ContextContent::Text {
                        text: Arc::from("user")
                    }],
                    cancellation: CancellationToken::new()
                })
                .await,
            Err(ThreadError::Model(_))
        ));
        let admitted = thread.snapshot().context;
        thread
            .retry_attempt("first".into(), "retry".into(), CancellationToken::new())
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.attempts[0].input, admitted);
        assert_eq!(snapshot.attempts[1].input, admitted);
        assert_eq!(snapshot.attempts[1].retry_of.as_deref(), Some("first"));
        assert_eq!(
            snapshot
                .context
                .records
                .iter()
                .filter(|record| record.source == ContextSource::User)
                .count(),
            1
        );
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.attempts[1].retry_of.as_deref(), Some("first"));
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct EmptyResources;
    impl crate::context::ResourceReader for EmptyResources {
        async fn read(
            &self,
            _: crate::context::ResourceReference,
            _: CancellationToken,
        ) -> Result<Arc<[u8]>, crate::context::ResourceReadError> {
            Ok(Arc::from([]))
        }
    }
    #[tokio::test]
    async fn resource_service_replacement_requires_a_new_request_instead_of_retrying_old_binding() {
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(FailFirstAttempt(false)),
        )
        .unwrap();
        let resources = crate::context::ResourceAccess::new(EmptyResources);
        thread.set_resources(resources.clone()).await.unwrap();
        assert!(
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "first".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new()
                })
                .await
                .is_err()
        );
        thread.set_resources(resources).await.unwrap();
        thread
            .set_resources(crate::context::ResourceAccess::new(EmptyResources))
            .await
            .unwrap();
        assert!(matches!(
            thread
                .retry_attempt("first".into(), "retry".into(), CancellationToken::new())
                .await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert_eq!(thread.snapshot().attempts.len(), 1);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn extension_batches_are_atomic_opaque_and_replay_deleted_versions() {
        use extensions::ExtensionMutation;
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let payload =
            OpaquePayload::new("plugin.plan", 99, "  not JSON\0{approved:true}\n").unwrap();
        let records = thread
            .mutate_extensions(vec![ExtensionMutation::Put {
                id: "plan".into(),
                expected_revision: None,
                payload: payload.clone(),
            }])
            .await
            .unwrap();
        let revision = records["plan"].revision;
        assert!(
            thread
                .mutate_extensions(vec![
                    ExtensionMutation::Put {
                        id: "other".into(),
                        expected_revision: None,
                        payload: payload.clone()
                    },
                    ExtensionMutation::Delete {
                        id: "plan".into(),
                        expected_revision: revision + 1
                    }
                ])
                .await
                .is_err()
        );
        assert_eq!(thread.snapshot().extensions, records);
        assert!(thread.snapshot().context.records.is_empty());
        thread
            .mutate_extensions(vec![ExtensionMutation::Delete {
                id: "plan".into(),
                expected_revision: revision,
            }])
            .await
            .unwrap();
        let commits = thread.journal().await.unwrap();
        let decoded = commits
            .iter()
            .map(|commit| {
                Arc::new(journal::ThreadCommit::decode(&commit.encode().unwrap()).unwrap())
            })
            .collect::<Vec<_>>();
        let current = journal::replay(&decoded).unwrap();
        assert!(current.extensions.is_empty());
        assert_eq!(current.extension_sequence, 2);
        let extensions::ExtensionChange::Put { record, .. } = &current.extension_changes[0] else {
            panic!("original version");
        };
        assert_eq!(record.payload, payload);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn application_checkpoint_commits_payload_and_projection_together_or_neither() {
        use extensions::{ApplicationUpdate, ExtensionMutation};
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let mutation = || ExtensionMutation::Put {
            id: "workflow".into(),
            expected_revision: None,
            payload: OpaquePayload::text("business state"),
        };
        let fact = RuntimeFact {
            source_id: "workflow".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("model projection"),
            }],
        };
        assert!(
            thread
                .update_application(ApplicationUpdate {
                    mutations: vec![mutation()],
                    facts: vec![fact.clone(), fact.clone()]
                })
                .await
                .is_err()
        );
        let unchanged = thread.snapshot();
        assert!(unchanged.extensions.is_empty());
        assert!(unchanged.context.records.is_empty());
        assert_eq!(unchanged.commit_sequence, 0);
        let committed = thread
            .update_application(ApplicationUpdate {
                mutations: vec![mutation()],
                facts: vec![fact],
            })
            .await
            .unwrap();
        assert_eq!(committed.commit_sequence, 1);
        assert_eq!(
            committed.extensions["workflow"].payload.content(),
            "business state"
        );
        let journal = thread.journal().await.unwrap();
        assert_eq!(journal.len(), 1);
        assert_eq!(journal[0].extensions.len(), 1);
        assert!(journal[0].context.is_some());
        assert_eq!(
            journal::replay(&journal).unwrap().context,
            committed.context
        );
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct ExtensionTool;
    impl crate::tool::opaque::Tool for ExtensionTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            Ok(crate::tool::ToolOutput::new(
                input,
                vec![ContextContent::Text {
                    text: Arc::from("state updated"),
                }],
            )
            .with_extension_mutations(vec![extensions::ExtensionMutation::Put {
                id: "custom.state".into(),
                expected_revision: context
                    .extensions
                    .get("custom.state")
                    .map(|record| record.revision),
                payload: OpaquePayload::new("plugin.future-state", 27, "unknown state\0").unwrap(),
            }]))
        }
    }

    #[tokio::test]
    async fn tool_state_changes_and_actual_result_are_committed_together() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    ExtensionTool,
                )
                .unwrap()
                .with_extension_updates(),
            ])
            .await
            .unwrap();
        thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(
            snapshot.extensions["custom.state"].payload.content(),
            "unknown state\0"
        );
        let commits = thread.journal().await.unwrap();
        let delivery = commits
            .iter()
            .find(|commit| !commit.deliveries.is_empty())
            .unwrap();
        assert_eq!(delivery.extensions.len(), 1);
        assert!(delivery.context.is_some());
        assert_eq!(
            journal::replay(&commits).unwrap().extensions,
            snapshot.extensions
        );
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct CancelledControlTool;
    impl crate::tool::opaque::Tool for CancelledControlTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            context.cancellation.cancel();
            Ok(crate::tool::ToolOutput::new(
                input,
                vec![ContextContent::Text {
                    text: Arc::from("late business success"),
                }],
            )
            .with_extension_mutations(vec![extensions::ExtensionMutation::Put {
                id: "late".into(),
                expected_revision: None,
                payload: OpaquePayload::text("must not apply"),
            }])
            .ending_turn())
        }
    }
    #[tokio::test]
    async fn cancelled_tool_preserves_observed_output_without_applying_control_or_state_changes() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    CancelledControlTool,
                )
                .unwrap()
                .with_turn_completion()
                .with_extension_updates(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert!(matches!(
            thread
                .execute_tool("call".into(), CancellationToken::new())
                .await,
            Err(ThreadError::Cancelled)
        ));
        let snapshot = thread.snapshot();
        assert!(snapshot.extensions.is_empty());
        snapshot.context.validate_complete().unwrap();
        let delivery = &snapshot.deliveries[0];
        assert!(matches!(delivery.outcome, ToolOutcome::Cancelled));
        assert_eq!(delivery.output.control(), crate::tool::ToolControl::EndTurn);
        assert_ne!(delivery.delivered_context, delivery.output.context());
        assert_eq!(
            snapshot.context.records.last().unwrap().content,
            delivery.delivered_context
        );
        let restored = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(
            restored.deliveries[0].delivered_context,
            delivery.delivered_context
        );
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn pending_tool_batch_cannot_be_interleaved_with_runtime_facts() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    EchoTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let before = thread.snapshot().context;
        let fact = RuntimeFact {
            source_id: "external".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("new facts"),
            }],
        };
        assert!(matches!(
            thread.update_facts(vec![fact.clone()]).await,
            Err(ThreadError::PendingTools)
        ));
        assert!(matches!(
            thread.patch_runtime_facts(vec![fact.clone()]).await,
            Err(ThreadError::PendingTools)
        ));
        assert_eq!(thread.snapshot().context, before);
        thread
            .execute_tool("call".into(), CancellationToken::new())
            .await
            .unwrap();
        thread.update_facts(vec![fact]).await.unwrap();
        thread.snapshot().context.validate_complete().unwrap();
        let commits = thread.journal().await.unwrap();
        journal::replay(&commits).unwrap();
        let mut changed = commits;
        let delivery = changed
            .iter_mut()
            .find(|commit| !commit.deliveries.is_empty())
            .unwrap();
        Arc::make_mut(&mut Arc::make_mut(delivery).deliveries)[0]
            .delivered_context
            .clear();
        assert!(matches!(
            journal::replay(&changed),
            Err(ThreadError::InvalidOutput)
        ));
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn messages_are_consumed_only_by_admitted_requests_and_not_by_failed_preparation() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let message = inbox::ThreadMessage {
            id: "message".into(),
            source_id: "external".into(),
            payload: OpaquePayload::new("plugin.message", 91, "private\0payload").unwrap(),
            context: vec![ContextContent::Text {
                text: Arc::from("delivered notice"),
            }],
        };
        assert_eq!(thread.send_message(message.clone()).await.unwrap(), 1);
        assert_eq!(thread.send_message(message).await.unwrap(), 1);
        let input = || StepInput {
            turn_id: "turn".into(),
            attempt_id: "attempt".into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        };
        thread
            .set_capacity(ContextCapacity::RequireExact {
                max_input_tokens: 100,
            })
            .await
            .unwrap();
        assert!(matches!(
            thread.step(input()).await,
            Err(ThreadError::UnknownCapacity)
        ));
        assert_eq!(thread.snapshot().consumed_messages, 0);
        assert!(thread.snapshot().context.records.is_empty());
        thread
            .set_capacity(ContextCapacity::Unbounded)
            .await
            .unwrap();
        thread.step(input()).await.unwrap();
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.consumed_messages, 1);
        assert_eq!(snapshot.inbox.len(), 1);
        let commits = thread.journal().await.unwrap();
        let consumed = commits
            .iter()
            .find(|commit| commit.consumed_messages == Some(1))
            .unwrap();
        assert!(matches!(
            consumed.attempt.as_ref().unwrap().outcome,
            AttemptOutcome::Running
        ));
        assert!(consumed.context.is_some());
        let restored = journal::replay(&commits).unwrap();
        assert_eq!(restored.inbox, snapshot.inbox);
        assert_eq!(restored.consumed_messages, 1);
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct CountingTool(Arc<std::sync::atomic::AtomicUsize>);
    impl crate::tool::opaque::Tool for CountingTool {
        async fn execute(
            &self,
            input: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::tool::ToolOutput::new(input, Vec::new()))
        }
    }
    #[tokio::test]
    async fn removed_tool_does_not_execute_a_call_returned_by_an_older_plan() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    CountingTool(calls.clone()),
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        thread.register_tools(Vec::new()).await.unwrap();
        assert!(matches!(
            thread
                .execute_tool("call".into(), CancellationToken::new())
                .await,
            Err(ThreadError::Tool(_))
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let snapshot = thread.snapshot();
        snapshot.context.validate_complete().unwrap();
        assert_eq!(snapshot.tasks["task:call"].status, task::TaskStatus::Failed);
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.context, snapshot.context);
        assert_eq!(replayed.tasks, snapshot.tasks);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn closing_cancels_unstarted_calls_without_running_their_executor() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    CountingTool(calls.clone()),
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(thread.snapshot().context.pending_calls().unwrap().len(), 1);
        thread.close().await.unwrap();
        let closed = thread.snapshot();
        closed.context.validate_complete().unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(closed.deliveries.len(), 1);
        assert!(matches!(
            closed.deliveries[0].outcome,
            ToolOutcome::Cancelled
        ));
    }
    #[tokio::test]
    async fn close_seals_queued_execution_before_the_close_command_is_processed() {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(DelayedModel {
                started: started.clone(),
                release: release.clone(),
            }),
        )
        .unwrap();
        let input = |id: &str| StepInput {
            turn_id: "turn".into(),
            attempt_id: id.into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        };
        let handle = thread.clone();
        let first_input = input("first");
        let first = tokio::spawn(async move { handle.step(first_input).await });
        started.notified().await;
        let queued = thread.step(input("queued"));
        tokio::pin!(queued);
        assert!(futures::poll!(&mut queued).is_pending());
        let closing = thread.close();
        tokio::pin!(closing);
        assert!(futures::poll!(&mut closing).is_pending());
        release.notify_one();
        assert!(matches!(first.await.unwrap(), Err(ThreadError::Cancelled)));
        assert!(matches!(queued.await, Err(ThreadError::Closed)));
        closing.await.unwrap();
        assert_eq!(thread.snapshot().attempts.len(), 1);
        assert_eq!(thread.snapshot().lifecycle, ThreadLifecycle::Closed);
    }
    #[derive(Debug)]
    struct PanickingTool;
    impl crate::tool::opaque::Tool for PanickingTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            panic!("fixture implementation panic")
        }
    }
    #[tokio::test]
    async fn panicking_tool_commits_failure_without_destroying_the_thread_owner() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    PanickingTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let snapshot = thread.snapshot();
        assert!(matches!(
            snapshot.deliveries[0].outcome,
            ToolOutcome::Failed(_)
        ));
        snapshot.context.validate_complete().unwrap();
        let encoded = thread
            .journal()
            .await
            .unwrap()
            .iter()
            .map(|commit| {
                Arc::new(journal::ThreadCommit::decode(&commit.encode().unwrap()).unwrap())
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            journal::replay(&encoded).unwrap().deliveries[0].outcome,
            ToolOutcome::Failed(_)
        ));
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn opaque_interaction_resolution_is_idempotent_and_preserves_actual_context() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let request = interactions::InteractionRequest {
            id: "question".into(),
            turn_id: "turn".into(),
            payload: OpaquePayload::new("plugin.question", 77, "unknown question format").unwrap(),
        };
        let pending = thread.request_interaction(request.clone()).await.unwrap();
        assert_eq!(thread.request_interaction(request).await.unwrap(), pending);
        assert!(thread.snapshot().context.records.is_empty());
        let response = interactions::InteractionResponse {
            payload: OpaquePayload::text("{approved:true,endTurn:true}"),
            context: vec![ContextContent::Text {
                text: Arc::from("actual answer"),
            }],
        };
        let resolved = thread
            .resolve_interaction(interactions::InteractionResolution {
                continuation: None,
                id: "question".into(),
                expected_revision: pending.revision,
                response: response.clone(),
                mutations: Vec::new(),
            })
            .await
            .unwrap();
        let committed = thread.snapshot().commit_sequence;
        assert_eq!(
            thread
                .resolve_interaction(interactions::InteractionResolution {
                    continuation: None,
                    id: "question".into(),
                    expected_revision: pending.revision,
                    response,
                    mutations: Vec::new()
                })
                .await
                .unwrap(),
            resolved
        );
        assert_eq!(thread.snapshot().commit_sequence, committed);
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.lifecycle, ThreadLifecycle::Open);
        assert!(matches!(
            snapshot.context.records[0].source,
            ContextSource::Runtime { .. }
        ));
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.interactions, snapshot.interactions);
        assert_eq!(replayed.context, snapshot.context);
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn interaction_cancellation_is_atomic_idempotent_and_replayable() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let pending = thread
            .request_interaction(interactions::InteractionRequest {
                id: "cancel-question".into(),
                turn_id: "turn".into(),
                payload: OpaquePayload::text("question"),
            })
            .await
            .unwrap();
        let command = interactions::InteractionCancellation {
            id: pending.request.id.clone(),
            expected_revision: pending.revision,
        };
        let terminal = thread.cancel_interaction(command.clone()).await.unwrap();
        assert_eq!(terminal.state, interactions::InteractionState::Cancelled);
        let snapshot = thread.snapshot();
        assert_eq!(thread.cancel_interaction(command).await.unwrap(), terminal);
        assert_eq!(thread.snapshot().commit_sequence, snapshot.commit_sequence);
        assert!(
            thread
                .resolve_interaction(interactions::InteractionResolution {
                    continuation: None,
                    id: pending.request.id.clone(),
                    expected_revision: terminal.revision,
                    response: interactions::InteractionResponse {
                        payload: OpaquePayload::text("late answer"),
                        context: Vec::new(),
                    },
                    mutations: Vec::new(),
                })
                .await
                .is_err()
        );
        assert!(
            thread
                .cancel_interaction(interactions::InteractionCancellation {
                    id: pending.request.id,
                    expected_revision: terminal.revision,
                })
                .await
                .is_err()
        );
        assert_eq!(snapshot.context.records.len(), 1);
        assert_eq!(
            snapshot.context.records[0].source,
            ContextSource::Runtime {
                source_id: "interaction:cancel-question".into(),
            }
        );
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.interactions, snapshot.interactions);
        assert_eq!(replayed.context, snapshot.context);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn interaction_answer_and_business_state_commit_atomically() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .request_interaction(interactions::InteractionRequest {
                id: "confirm".into(),
                turn_id: "turn".into(),
                payload: OpaquePayload::text("question"),
            })
            .await
            .unwrap();
        let response = interactions::InteractionResponse {
            payload: OpaquePayload::text("opaque answer"),
            context: vec![ContextContent::Text {
                text: Arc::from("answer context"),
            }],
        };
        let mutation = |expected| extensions::ExtensionMutation::Put {
            id: "plan".into(),
            expected_revision: expected,
            payload: OpaquePayload::new("studio.plan", 1, "approved state").unwrap(),
        };
        assert!(
            thread
                .resolve_interaction(interactions::InteractionResolution {
                    continuation: None,
                    id: "confirm".into(),
                    expected_revision: 1,
                    response: response.clone(),
                    mutations: vec![mutation(Some(9))]
                })
                .await
                .is_err()
        );
        assert!(thread.snapshot().extensions.is_empty());
        assert!(thread.snapshot().context.records.is_empty());
        assert_eq!(
            thread.snapshot().interactions["confirm"].state,
            interactions::InteractionState::Pending
        );
        thread
            .resolve_interaction(interactions::InteractionResolution {
                continuation: None,
                id: "confirm".into(),
                expected_revision: 1,
                response,
                mutations: vec![mutation(None)],
            })
            .await
            .unwrap();
        let commits = thread.journal().await.unwrap();
        let resolved = commits.last().unwrap();
        assert_eq!(resolved.interactions.len(), 1);
        assert_eq!(resolved.extensions.len(), 1);
        assert!(resolved.context.is_some());
        let replayed = journal::replay(&commits).unwrap();
        assert_eq!(
            replayed.extensions["plan"].payload.content(),
            "approved state"
        );
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct BackgroundFailure;
    impl cold::ColdStore for BackgroundFailure {
        fn pressure(&self, _: &str) -> cold::StoragePressure {
            cold::StoragePressure {
                thread_bytes: 0,
                store_bytes: 0,
                error: Some(Arc::new(cold::ColdStoreError {
                    source: Box::new(std::io::Error::other("background writer failure")),
                })),
            }
        }
        fn admit(&self, _: &str, _: u64, _: OpaquePayload) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn background_storage_failure_is_observed_before_new_model_admission() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .attach_storage(cold::ColdStoreHandle::new(BackgroundFailure))
            .await
            .unwrap();
        let error = thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap_err();
        let ThreadError::Storage(error) = error else {
            panic!("typed storage failure");
        };
        assert!(error.source.downcast_ref::<std::io::Error>().is_some());
        assert!(thread.snapshot().attempts.is_empty());
        assert!(thread.close().await.is_err());
    }
    struct PressureDuringPreparation {
        bytes: Arc<std::sync::atomic::AtomicU64>,
        executed: Arc<std::sync::atomic::AtomicBool>,
    }
    impl ModelSession for PressureDuringPreparation {
        fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> impl std::future::Future<Output = Result<PreparedModelCall, ModelError>> + Send
        {
            let executed = self.executed.clone();
            self.bytes
                .store(64 * 1024 * 1024, std::sync::atomic::Ordering::SeqCst);
            async move {
                Ok(PreparedModelCall::new(async move {
                    executed.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(ModelStepOutput {
                        attempt_id: request.attempt_id,
                        base_context_revision: request.context.revision,
                        content: Vec::new(),
                        tool_calls: Vec::new(),
                        private_context: None,
                        usage: Default::default(),
                    })
                }))
            }
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn pressure_increasing_during_prepare_prevents_execution_and_input_consumption() {
        let bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(PressureDuringPreparation {
                bytes: bytes.clone(),
                executed: executed.clone(),
            }),
        )
        .unwrap();
        thread
            .attach_storage(cold::ColdStoreHandle::new(PressureStore(bytes)))
            .await
            .unwrap();
        assert!(matches!(
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: vec![ContextContent::Text {
                        text: Arc::from("user input")
                    }],
                    cancellation: CancellationToken::new()
                })
                .await,
            Err(ThreadError::StoragePressure)
        ));
        assert!(!executed.load(std::sync::atomic::Ordering::SeqCst));
        assert!(thread.snapshot().attempts.is_empty());
        assert!(thread.snapshot().context.records.is_empty());
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn coalesced_snapshot_notifications_can_be_completed_from_paged_history() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        let mut subscription = thread.subscribe();
        assert_eq!(subscription.next().await.unwrap().commit_sequence, 0);
        for id in ["one", "two"] {
            thread
                .send_message(inbox::ThreadMessage {
                    id: id.into(),
                    source_id: "external".into(),
                    payload: OpaquePayload::text(id),
                    context: Vec::new(),
                })
                .await
                .unwrap();
        }
        let latest = subscription.next().await.unwrap();
        assert_eq!(latest.commit_sequence, 2);
        let limit = std::num::NonZeroUsize::new(1).unwrap();
        let first = thread.journal_page(0, limit).await.unwrap();
        let second = thread.journal_page(first[0].sequence, limit).await.unwrap();
        assert_eq!(first[0].sequence, 1);
        assert_eq!(second[0].sequence, 2);
        assert!(thread.journal_page(2, limit).await.unwrap().is_empty());
        assert!(thread.journal_page(3, limit).await.is_err());
        thread.close().await.unwrap();
        assert_eq!(
            subscription.next().await.unwrap().lifecycle,
            ThreadLifecycle::Closed
        );
        assert!(subscription.next().await.is_none());
        let final_history = thread.journal().await.unwrap();
        let final_snapshot = thread.snapshot();
        assert_eq!(
            final_history.last().unwrap().sequence,
            final_snapshot.commit_sequence
        );
        let replayed = journal::replay(&final_history).unwrap();
        assert_eq!(replayed.lifecycle, ThreadLifecycle::Closed);
        assert_eq!(replayed.inbox.len(), 2);
        let final_page = thread
            .journal_page(final_snapshot.commit_sequence - 1, limit)
            .await
            .unwrap();
        assert_eq!(final_page[0].sequence, final_snapshot.commit_sequence);
    }
    #[derive(Debug)]
    struct QuestionTool;
    impl crate::tool::opaque::Tool for QuestionTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            Ok(
                crate::tool::ToolOutput::new(OpaquePayload::text("question facts"), Vec::new())
                    .with_interaction(
                        OpaquePayload::new("plugin.question", 17, "opaque question").unwrap(),
                    ),
            )
        }
    }
    #[tokio::test]
    async fn interaction_tool_commits_pending_request_and_waits_for_explicit_resolution() {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    QuestionTool,
                )
                .unwrap()
                .with_interactions(),
            ])
            .await
            .unwrap();
        let result = thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert_eq!(result.outcome, TurnOutcome::WaitingInteraction);
        let input = || StepInput {
            turn_id: "next-turn".into(),
            attempt_id: "next-attempt".into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        };
        assert!(matches!(
            thread.step(input()).await,
            Err(ThreadError::PendingInteraction)
        ));
        let snapshot = thread.snapshot();
        let pending = &snapshot.interactions["tool-interaction:call"];
        assert_eq!(pending.request.payload.content(), "opaque question");
        thread
            .resolve_interaction(interactions::InteractionResolution {
                continuation: None,
                id: pending.request.id.clone(),
                expected_revision: 1,
                response: interactions::InteractionResponse {
                    payload: OpaquePayload::text("response"),
                    context: vec![ContextContent::Text {
                        text: Arc::from("user response"),
                    }],
                },
                mutations: Vec::new(),
            })
            .await
            .unwrap();
        thread.step(input()).await.unwrap();
        journal::replay(&thread.journal().await.unwrap()).unwrap();
        thread.close().await.unwrap();
    }
    struct ProjectionCalls(Arc<std::sync::Mutex<OpaquePayload>>);
    impl ModelSession for ProjectionCalls {
        fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> impl std::future::Future<Output = Result<PreparedModelCall, ModelError>> + Send
        {
            let projection = self.0.lock().unwrap().clone();
            async move {
                let mut delegate = CallsOnce;
                Ok(delegate
                    .prepare(request)
                    .await?
                    .with_request_metadata(projection.clone())
                    .with_tool_projection(projection))
            }
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct ProjectionEcho;
    impl crate::tool::opaque::Tool for ProjectionEcho {
        async fn execute(
            &self,
            _: OpaquePayload,
            context: crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::ToolOutput, crate::tool::opaque::ToolError> {
            Ok(crate::tool::ToolOutput::new(
                context.model_projection.expect("prepared projection"),
                Vec::new(),
            ))
        }
    }

    #[tokio::test]
    async fn tool_projection_is_frozen_with_admission_and_replayed_as_unknown_bytes() {
        let original = OpaquePayload::new(
            "custom.model-projection",
            91,
            "not JSON\r\nendTurn: true\n原文",
        )
        .unwrap();
        let source = Arc::new(std::sync::Mutex::new(original.clone()));
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(ProjectionCalls(source.clone())),
        )
        .unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("declaration"),
                    ProjectionEcho,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        *source.lock().unwrap() = OpaquePayload::text("new global model configuration");
        let output = thread
            .execute_tool("call".into(), CancellationToken::new())
            .await
            .unwrap();
        let ToolDispatch::Completed(output) = output else {
            panic!("projection tool completes immediately");
        };
        assert_eq!(output.payload(), &original);
        let history = thread.journal().await.unwrap();
        let saved = journal::replay(&history).unwrap();
        assert_eq!(saved.attempts[0].tool_projection.as_ref(), Some(&original));
        assert_eq!(saved.attempts[0].request_metadata.as_ref(), Some(&original));
        assert_eq!(saved.deliveries[0].output.payload(), &original);
        let mut corrupt = history.clone();
        let index = corrupt
            .iter()
            .position(|commit| {
                commit
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| matches!(attempt.outcome, AttemptOutcome::Committed(_)))
            })
            .unwrap();
        let mut commit = (*corrupt[index]).clone();
        commit.attempt.as_mut().unwrap().tool_projection = Some(OpaquePayload::text("tampered"));
        corrupt[index] = Arc::new(commit);
        assert!(journal::replay(&corrupt).is_err());
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn changed_host_authorization_prevents_an_old_pending_call_from_starting() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
        let registration = |version| {
            crate::tool::opaque::Registration::new(
                "echo".into(),
                OpaquePayload::text("same declaration"),
                CountingTool(calls.clone()),
            )
            .unwrap()
            .with_authorization(crate::tool::opaque::ToolAuthorization::new(version))
        };
        thread
            .register_tools(vec![registration("writable")])
            .await
            .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "attempt".into(),
                content: Vec::new(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        thread
            .register_tools(vec![registration("read-only")])
            .await
            .unwrap();
        assert!(
            thread
                .execute_tool("call".into(), CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(matches!(
            thread.snapshot().deliveries[0].outcome,
            ToolOutcome::Failed(_)
        ));
        thread.close().await.unwrap();
    }
    #[tokio::test]
    async fn requested_message_wakeup_uses_runtime_context_without_duplicate_invocation() {
        let thread = ThreadHandle::start("child".into(), DynModelSession::new(Echo)).unwrap();
        let message = inbox::ThreadMessage {
            id: "parent-message".into(),
            source_id: "agent:parent".into(),
            payload: OpaquePayload::text("{\"approved\":true}"),
            context: vec![ContextContent::Text {
                text: Arc::from("parent guidance"),
            }],
        };
        let options = input::InputDriverOptions {
            max_model_steps: std::num::NonZeroU32::new(3).unwrap(),
        };
        let mut subscription = thread.subscribe();
        let sequence = thread
            .send_message_and_resume(message.clone(), options)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let snapshot = subscription.next().await.unwrap();
                if snapshot.consumed_messages == sequence
                    && snapshot
                        .turns
                        .last()
                        .is_some_and(|turn| matches!(turn.state, TurnState::Finished(_)))
                {
                    break;
                }
            }
        })
        .await
        .expect("message wakes the idle owner");
        let snapshot = thread.snapshot();
        assert!(snapshot.inputs.is_empty());
        assert_eq!(snapshot.attempts.len(), 1);
        assert!(snapshot.context.records.iter().any(|record| record.source
            == ContextSource::Runtime {
                source_id: "agent:parent".into()
            }));
        assert!(
            !snapshot
                .context
                .records
                .iter()
                .any(|record| record.source == ContextSource::User)
        );
        assert_eq!(
            thread
                .send_message_and_resume(message, options)
                .await
                .unwrap(),
            sequence
        );
        let history = thread.journal().await.unwrap();
        assert_eq!(thread.snapshot().attempts.len(), 1);
        let restored = journal::replay(&history).unwrap();
        assert_eq!(restored.wake_messages_through, sequence);
        assert_eq!(restored.consumed_messages, sequence);
        thread.close().await.unwrap();
    }
    struct ReusedCall;
    impl ModelSession for ReusedCall {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            Ok(PreparedModelCall::new(async move {
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: vec![crate::model::ModelToolCall {
                        call_id: "old-call".into(),
                        tool_id: "echo".into(),
                        arguments: OpaquePayload::text("new arguments"),
                    }],
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn inherited_call_and_record_ids_cannot_be_reused_by_new_model_attempts() {
        let thread = ThreadHandle::start("child".into(), DynModelSession::new(ReusedCall)).unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("declaration"),
                    EchoTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let records = vec![
            ContextRecord {
                id: "parent-output".into(),
                turn_id: Some("parent-turn".into()),
                source: ContextSource::Assistant,
                content: Vec::new(),
                tool_calls: vec![crate::model::ModelToolCall {
                    call_id: "old-call".into(),
                    tool_id: "echo".into(),
                    arguments: OpaquePayload::text("original"),
                }],
            },
            ContextRecord {
                id: "collision:output".into(),
                turn_id: Some("parent-turn".into()),
                source: ContextSource::ToolResult {
                    call_id: "old-call".into(),
                    tool_id: "echo".into(),
                },
                content: Vec::new(),
                tool_calls: Vec::new(),
            },
        ];
        thread
            .replace_context(ReplaceContext {
                expected_revision: 0,
                reason: ContextReplacementReason::Rebuild,
                records,
            })
            .await
            .unwrap();
        let original = thread.snapshot().context;
        let step = |id: &str| StepInput {
            turn_id: "child-turn".into(),
            attempt_id: id.into(),
            content: Vec::new(),
            cancellation: CancellationToken::new(),
        };
        assert!(matches!(
            thread.step(step("collision")).await,
            Err(ThreadError::InvalidIdentity)
        ));
        assert!(thread.snapshot().attempts.is_empty());
        assert!(matches!(
            thread.step(step("fresh")).await,
            Err(ThreadError::InvalidOutput)
        ));
        assert_eq!(thread.snapshot().context, original);
        assert!(matches!(
            thread.snapshot().attempts[0].outcome,
            AttemptOutcome::Rejected(_)
        ));
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct UserPermission;
    impl crate::tool::execution_policy::ExecutionPolicy for UserPermission {
        async fn authorize(
            &self,
            _: &OpaquePayload,
            context: &crate::tool::opaque::CallContext,
        ) -> Result<crate::tool::execution_policy::ExecutionGrant, crate::tool::opaque::ToolError>
        {
            let access = context.tasks.as_ref().unwrap();
            let decision = access
                .request_execution_permission(
                    OpaquePayload::new(
                        "test.permission",
                        99,
                        r#" {"approved": true, "message": "原样保存"} "#.to_owned(),
                    )
                    .unwrap(),
                    context.cancellation.clone(),
                )
                .await
                .map_err(crate::tool::opaque::ToolError::new)?;
            match decision {
                permissions::PermissionDecision::Allow => Ok(Default::default()),
                permissions::PermissionDecision::Deny => Err(crate::tool::opaque::ToolError::new(
                    std::io::Error::other("denied"),
                )),
            }
        }
    }

    #[tokio::test]
    async fn execution_permission_is_typed_replayed_and_never_reused_after_recovery() {
        for decision in [
            permissions::PermissionDecision::Allow,
            permissions::PermissionDecision::Deny,
        ] {
            let executions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let thread =
                ThreadHandle::start("thread".into(), DynModelSession::new(CallsOnce)).unwrap();
            thread
                .register_tools(vec![
                    crate::tool::opaque::Registration::new(
                        "echo".into(),
                        OpaquePayload::text("schema"),
                        CountingTool(executions.clone()),
                    )
                    .unwrap()
                    .foreground()
                    .with_execution_policy(
                        crate::tool::execution_policy::ExecutionPolicyHandle::new(UserPermission),
                    ),
                ])
                .await
                .unwrap();
            thread
                .step(StepInput {
                    turn_id: "turn".into(),
                    attempt_id: "attempt".into(),
                    content: Vec::new(),
                    cancellation: CancellationToken::new(),
                })
                .await
                .unwrap();
            let mut subscription = thread.subscribe();
            let execution = tokio::spawn({
                let thread = thread.clone();
                async move {
                    thread
                        .execute_tool("call".into(), CancellationToken::new())
                        .await
                }
            });
            let pending = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let snapshot = subscription.next().await.unwrap();
                    if let Some(record) = snapshot.permissions.get("permission:call") {
                        break record.clone();
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 0);
            let commits = thread.journal().await.unwrap();
            let replayed = journal::replay(&commits).unwrap();
            assert_eq!(replayed.permissions[&pending.id], pending);
            let restored =
                ThreadHandle::restore("thread".into(), DynModelSession::new(CallsOnce), commits)
                    .unwrap();
            assert_eq!(
                restored.snapshot().permissions[&pending.id].state,
                permissions::PermissionState::Cancelled
            );
            assert!(
                restored
                    .resolve_execution_permission(permissions::PermissionResolution {
                        payload: None,
                        id: pending.id.clone(),
                        expected_revision: pending.revision,
                        decision: permissions::PermissionDecision::Allow
                    })
                    .await
                    .is_err()
            );
            restored.close().await.unwrap();
            thread
                .resolve_execution_permission(permissions::PermissionResolution {
                    payload: None,
                    id: pending.id.clone(),
                    expected_revision: pending.revision,
                    decision,
                })
                .await
                .unwrap();
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), execution)
                .await
                .unwrap()
                .unwrap();
            match decision {
                permissions::PermissionDecision::Allow => {
                    result.unwrap();
                    assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 1);
                }
                permissions::PermissionDecision::Deny => {
                    assert!(result.is_err());
                    assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 0);
                }
            }
            let sequence = thread.snapshot().commit_sequence;
            thread
                .resolve_execution_permission(permissions::PermissionResolution {
                    id: pending.id.clone(),
                    expected_revision: pending.revision,
                    decision,
                    payload: None,
                })
                .await
                .expect("an identical retry returns the recorded decision without executing again");
            assert_eq!(thread.snapshot().commit_sequence, sequence);
            assert_eq!(
                journal::replay(&thread.journal().await.unwrap())
                    .unwrap()
                    .permissions,
                thread.snapshot().permissions
            );
            thread.close().await.unwrap();
        }
    }
    #[tokio::test]
    async fn interaction_resolution_commits_extension_and_continuation_together_or_changes_nothing()
    {
        let thread = ThreadHandle::start("thread".into(), DynModelSession::new(Echo)).unwrap();
        thread
            .request_interaction(interactions::InteractionRequest {
                id: "question".into(),
                turn_id: "turn".into(),
                payload: OpaquePayload::text("question"),
            })
            .await
            .unwrap();
        let resolution = |input_id: &str| interactions::InteractionResolution {
            id: "question".into(),
            expected_revision: 1,
            response: interactions::InteractionResponse {
                payload: OpaquePayload::text("answer"),
                context: vec![ContextContent::Text {
                    text: "answer projection".into(),
                }],
            },
            mutations: vec![extensions::ExtensionMutation::Put {
                id: "application".into(),
                expected_revision: None,
                payload: OpaquePayload::new("unknown.state", 77, "exact state\n").unwrap(),
            }],
            continuation: Some(input::ThreadInput {
                id: input_id.into(),
                payload: OpaquePayload::text("continuation metadata"),
                context: vec![ContextContent::Text {
                    text: "continue work".into(),
                }],
            }),
        };
        let before = thread.snapshot().commit_sequence;
        assert!(thread.resolve_interaction(resolution("")).await.is_err());
        assert_eq!(thread.snapshot().commit_sequence, before);
        assert!(thread.snapshot().extensions.is_empty());
        assert!(thread.snapshot().inputs.is_empty());
        let resolved = thread
            .resolve_interaction(resolution("answer-input"))
            .await
            .unwrap();
        let sequence = thread.snapshot().commit_sequence;
        assert_eq!(
            thread
                .resolve_interaction(resolution("answer-input"))
                .await
                .unwrap(),
            resolved
        );
        assert_eq!(thread.snapshot().commit_sequence, sequence);
        let journal = thread.journal().await.unwrap();
        let commit = journal.last().unwrap();
        assert_eq!(commit.interactions.len(), 1);
        assert_eq!(commit.inputs.len(), 1);
        assert_eq!(commit.extensions.len(), 1);
        assert_eq!(
            commit.interactions[0].continuation_id.as_deref(),
            Some("answer-input")
        );
        let mut incomplete = journal.clone();
        let mut broken_commit = incomplete.last().unwrap().as_ref().clone();
        broken_commit.inputs = Vec::new().into();
        *incomplete.last_mut().unwrap() = Arc::new(broken_commit);
        assert!(
            journal::replay(&incomplete).is_err(),
            "a continuation without its input fact is an incomplete commit"
        );
        let restored =
            ThreadHandle::restore("thread".into(), DynModelSession::new(Echo), journal).unwrap();
        assert_eq!(
            restored.snapshot().inputs[0].state,
            input::InputState::Pending
        );
        assert!(
            restored.snapshot().attempts.is_empty(),
            "restoration must not execute the queued continuation"
        );
        assert_eq!(restored.snapshot().extensions, thread.snapshot().extensions);
        restored.close().await.unwrap();
        thread.close().await.unwrap();
    }
    struct SteeringModel {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        inputs: Arc<std::sync::Mutex<Vec<ContextSnapshot>>>,
    }
    impl ModelSession for SteeringModel {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let first = {
                let mut inputs = self.inputs.lock().unwrap();
                let first = inputs.is_empty();
                inputs.push(request.context.clone());
                first
            };
            let started = self.started.clone();
            let release = self.release.clone();
            Ok(PreparedModelCall::new(async move {
                if first {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![ContextContent::Text {
                        text: "done".into(),
                    }],
                    tool_calls: if first {
                        vec![crate::model::ModelToolCall {
                            call_id: "echo-call".into(),
                            tool_id: "echo".into(),
                            arguments: OpaquePayload::text("tool input"),
                        }]
                    } else {
                        Vec::new()
                    },
                    private_context: None,
                    usage: Default::default(),
                })
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn steering_joins_the_next_step_without_reordering_queued_turns_or_changing_frozen_input()
    {
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread = ThreadHandle::start(
            "thread".into(),
            DynModelSession::new(SteeringModel {
                started: started.clone(),
                release: release.clone(),
                inputs: inputs.clone(),
            }),
        )
        .unwrap();
        thread
            .register_tools(vec![
                crate::tool::opaque::Registration::new(
                    "echo".into(),
                    OpaquePayload::text("schema"),
                    EchoTool,
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let input = |id: &str| input::ThreadInput {
            id: id.into(),
            payload: OpaquePayload::text(id.to_owned()),
            context: vec![ContextContent::Text { text: id.into() }],
        };
        assert!(matches!(
            thread
                .submit_input_with_policy(input::InputSubmission {
                    input: input("invalid-steer"),
                    policy: input::InputPolicy::SteerOnly,
                    drive: None
                })
                .await,
            Err(ThreadError::InputRequiresActiveTurn)
        ));
        thread
            .submit_input_and_run(
                input("original"),
                input::InputDriverOptions {
                    max_model_steps: std::num::NonZeroU32::new(4).unwrap(),
                },
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        let original_turn = thread.snapshot().turns.last().unwrap().turn_id.clone();
        assert!(matches!(
            thread
                .submit_input_with_policy(input::InputSubmission {
                    input: input("invalid-start"),
                    policy: input::InputPolicy::StartOnly,
                    drive: None
                })
                .await,
            Err(ThreadError::InputRequiresIdle)
        ));
        let queued = thread
            .submit_input_with_policy(input::InputSubmission {
                input: input("queued"),
                policy: input::InputPolicy::StartOrQueue,
                drive: None,
            })
            .await
            .unwrap();
        let steered = thread
            .submit_input_with_policy(input::InputSubmission {
                input: input("steer"),
                policy: input::InputPolicy::SteerOnly,
                drive: None,
            })
            .await
            .unwrap();
        assert_eq!(queued.delivery, input::InputDelivery::NextTurn);
        assert_eq!(
            steered.delivery,
            input::InputDelivery::CurrentTurn {
                turn_id: original_turn.clone()
            }
        );
        release.notify_one();
        let mut subscription = thread.subscribe();
        let completed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = subscription.next().await.unwrap();
                if snapshot.inputs.len() == 3
                    && snapshot
                        .inputs
                        .iter()
                        .all(|record| matches!(record.state, input::InputState::Consumed { .. }))
                    && snapshot
                        .turns
                        .iter()
                        .all(|turn| turn.state != TurnState::Running)
                {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        let has = |snapshot: &ContextSnapshot, expected: &str| {
            snapshot.records.iter().any(|record| {
                record.source == ContextSource::User
                    && record.content
                        == vec![ContextContent::Text {
                            text: expected.into(),
                        }]
            })
        };
        let captured = inputs.lock().unwrap().clone();
        assert_eq!(captured.len(), 3);
        assert!(has(&captured[0], "original"));
        assert!(!has(&captured[0], "steer"));
        assert!(has(&captured[1], "steer"));
        assert!(!has(&captured[1], "queued"));
        assert!(has(&captured[2], "queued"));
        let steer = completed
            .inputs
            .iter()
            .find(|input| input.input.id == "steer")
            .unwrap();
        assert!(
            matches!(&steer.state, input::InputState::Consumed { turn_id, .. } if turn_id == &original_turn)
        );
        assert_eq!(
            journal::replay(&thread.journal().await.unwrap())
                .unwrap()
                .inputs,
            completed.inputs
        );
        thread.close().await.unwrap();
    }
}
