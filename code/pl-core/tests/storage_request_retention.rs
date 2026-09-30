//! A long running Turn must not accumulate frozen copies of its original input.
use pl_core::{
    context::{ContextContent, OpaquePayload},
    model::{
        DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput, ModelToolCall,
        PreparedModelCall,
    },
    thread::{
        ModelStepLimit, ThreadHandle, TurnInput,
        cold::{ColdStore, ColdStoreError, ColdStoreHandle, StoragePressure, ThreadWrite},
    },
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use tokio_util::sync::CancellationToken;

const MARKER: &str = "unique-input-retained-only-by-current-context";
struct RepeatingModel(usize);
impl ModelSession for RepeatingModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        self.0 += 1;
        let calls = if self.0 <= 24 {
            vec![ModelToolCall {
                call_id: format!("call-{}", self.0),
                tool_id: "tick".into(),
                arguments: OpaquePayload::text("tick"),
            }]
        } else {
            vec![]
        };
        Ok(PreparedModelCall::new(async move {
            Ok(ModelStepOutput {
                attempt_id: request.attempt_id,
                base_context_revision: request.context.revision,
                content: vec![ContextContent::Text {
                    text: Arc::from("step completed"),
                }],
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
struct Tick;
impl Tool for Tick {
    async fn execute(&self, input: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::new(
            input,
            vec![ContextContent::Text {
                text: Arc::from("tick result"),
            }],
        ))
    }
}
#[derive(Debug, Clone, Default)]
struct ObserveCheckpoint {
    copies: Arc<AtomicUsize>,
    durable: Arc<AtomicU64>,
}
impl ColdStore for ObserveCheckpoint {
    fn admit(&self, _: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        let encoded = serde_json::to_string(&write.checkpoint.pruned()).unwrap();
        self.copies
            .fetch_max(encoded.matches(MARKER).count(), Ordering::SeqCst);
        self.durable.store(write.effect.sequence, Ordering::SeqCst);
        Ok(())
    }
    fn pressure(&self, _: &str) -> StoragePressure {
        StoragePressure {
            durable_sequence: self.durable.load(Ordering::SeqCst),
            ..Default::default()
        }
    }
    async fn flush(&self, _: &str, _: u64) -> Result<(), ColdStoreError> {
        Ok(())
    }
}
#[tokio::test]
async fn long_turn_checkpoint_keeps_one_current_input() {
    let thread = ThreadHandle::start(
        "long-retention".into(),
        DynModelSession::new(RepeatingModel(0)),
    )
    .unwrap();
    let store = ObserveCheckpoint::default();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread
        .register_tools(vec![
            Registration::new("tick".into(), OpaquePayload::text("tick"), Tick)
                .unwrap()
                .foreground_coexisting(),
        ])
        .await
        .unwrap();
    thread
        .run_turn(TurnInput {
            turn_id: "long".into(),
            attempt_prefix: "attempt".into(),
            content: vec![ContextContent::Text {
                text: Arc::from(MARKER),
            }],
            max_model_steps: ModelStepLimit::Limited(30.try_into().unwrap()),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    thread.close().await.unwrap();
    assert_eq!(
        store.copies.load(Ordering::SeqCst),
        1,
        "checkpoint duplicated historical model input during a live Turn"
    );
}

struct ReusedCallModel;
impl ModelSession for ReusedCallModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        let called = request
            .context
            .records
            .iter()
            .any(|record| !record.tool_calls.is_empty());
        Ok(PreparedModelCall::new(async move {
            Ok(ModelStepOutput {
                attempt_id: request.attempt_id,
                base_context_revision: request.context.revision,
                content: vec![],
                tool_calls: if called {
                    vec![]
                } else {
                    vec![ModelToolCall {
                        call_id: "fixed-call".into(),
                        tool_id: "tick".into(),
                        arguments: OpaquePayload::text("tick"),
                    }]
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
async fn compacted_tool_identity_survives_turn_completion_and_restart() {
    use pl_core::thread::{
        ContextReplacementReason, ModelOutputViolation, ReplaceContext, StepInput, ThreadError,
    };
    let thread =
        ThreadHandle::start("deduplicate".into(), DynModelSession::new(ReusedCallModel)).unwrap();
    thread
        .register_tools(vec![
            Registration::new("tick".into(), OpaquePayload::text("tick"), Tick)
                .unwrap()
                .foreground_coexisting(),
        ])
        .await
        .unwrap();
    thread
        .run_turn(TurnInput {
            turn_id: "original".into(),
            attempt_prefix: "original".into(),
            content: vec![],
            max_model_steps: ModelStepLimit::Limited(3.try_into().unwrap()),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    thread
        .replace_context(ReplaceContext {
            expected_revision: thread.snapshot().context.revision,
            reason: ContextReplacementReason::Compaction,
            records: vec![],
        })
        .await
        .unwrap();
    let checkpoint = thread
        .checkpoint(thread.snapshot().commit_sequence)
        .unwrap();
    assert!(checkpoint.state.live_calls.contains_key("fixed-call"));
    thread.close().await.unwrap();
    let restored = ThreadHandle::resume(
        "deduplicate".into(),
        DynModelSession::new(ReusedCallModel),
        Some(checkpoint),
    )
    .unwrap();
    restored
        .register_tools(vec![
            Registration::new("tick".into(), OpaquePayload::text("tick"), Tick)
                .unwrap()
                .foreground_coexisting(),
        ])
        .await
        .unwrap();
    let error = restored
        .step(StepInput {
            turn_id: "next".into(),
            attempt_id: "next".into(),
            content: vec![],
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ThreadError::ModelOutput(ModelOutputViolation::DuplicateCallIdentity { .. })
    ));
    restored.close().await.unwrap();
}

struct FailsOnce(bool);
impl ModelSession for FailsOnce {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        let fail = std::mem::replace(&mut self.0, false);
        Ok(PreparedModelCall::new(async move {
            if fail {
                return Err(ModelError {
                    details: None,
                    kind: pl_core::model::ModelFailureKind::Unavailable,
                    usage: Default::default(),
                    source: None,
                });
            }
            Ok(ModelStepOutput {
                attempt_id: request.attempt_id,
                base_context_revision: request.context.revision,
                content: vec![],
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

#[tokio::test]
async fn retry_uses_latest_failure_and_rejects_changed_context_or_tool_plan() {
    use pl_core::thread::{ContextReplacementReason, ReplaceContext, StepInput, ThreadError};
    for mutation in ["none", "context", "tools"] {
        let thread = ThreadHandle::start(
            format!("retry-{mutation}"),
            DynModelSession::new(FailsOnce(true)),
        )
        .unwrap();
        thread
            .step(StepInput {
                turn_id: "turn".into(),
                attempt_id: "failed".into(),
                content: vec![],
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap_err();
        let effects = thread.effects().await.unwrap();
        assert!(
            effects.iter().any(
                |effect| effect.attempt.as_ref().is_some_and(|attempt| matches!(
                    attempt.outcome,
                    pl_core::thread::AttemptOutcome::Failed(_)
                ))
            )
        );
        if mutation == "context" {
            thread
                .replace_context(ReplaceContext {
                    expected_revision: thread.snapshot().context.revision,
                    reason: ContextReplacementReason::Compaction,
                    records: vec![],
                })
                .await
                .unwrap();
        } else if mutation == "tools" {
            thread
                .register_tools(vec![
                    Registration::new("tick".into(), OpaquePayload::text("tick"), Tick).unwrap(),
                ])
                .await
                .unwrap();
        }
        let result = thread
            .retry_attempt("failed".into(), "retried".into(), CancellationToken::new())
            .await;
        if mutation == "none" {
            result.unwrap();
        } else {
            assert!(matches!(result, Err(ThreadError::InvalidContext)));
        }
        assert!(
            thread
                .retry_attempt(
                    "failed".into(),
                    "old-retry".into(),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        thread.close().await.unwrap();
    }
}

#[tokio::test]
async fn execution_receipts_are_reliable_but_only_latest_is_resident() {
    #[derive(Debug)]
    struct Receipts(Arc<std::sync::Mutex<Option<pl_core::thread::TaskAccess>>>);
    impl Tool for Receipts {
        async fn execute(
            &self,
            input: OpaquePayload,
            context: CallContext,
        ) -> Result<ToolOutput, ToolError> {
            let task = context.tasks.unwrap();
            task.record_observation(OpaquePayload::new("receipt", 1, context.call_id).unwrap())
                .await
                .map_err(ToolError::new)?;
            *self.0.lock().unwrap() = Some(task);
            Ok(ToolOutput::new(input, vec![]))
        }
    }
    let last = Arc::new(std::sync::Mutex::new(None));
    let thread =
        ThreadHandle::start("receipts".into(), DynModelSession::new(RepeatingModel(0))).unwrap();
    thread
        .register_tools(vec![
            Registration::new(
                "tick".into(),
                OpaquePayload::text("tick"),
                Receipts(last.clone()),
            )
            .unwrap()
            .foreground_coexisting(),
        ])
        .await
        .unwrap();
    thread
        .run_turn(TurnInput {
            turn_id: "receipts".into(),
            attempt_prefix: "attempt".into(),
            content: vec![],
            max_model_steps: ModelStepLimit::Limited(30.try_into().unwrap()),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(thread.snapshot().extensions.len(), 1);
    assert_eq!(
        thread.snapshot().extensions["tool-observation:receipt"]
            .payload
            .content(),
        "call-24"
    );
    let effects = thread.effects().await.unwrap();
    assert_eq!(effects.iter().flat_map(|effect| effect.extensions.iter()).filter(|change| matches!(change, pl_core::thread::extensions::ExtensionChange::Put { record, .. } if record.payload.format() == "receipt")).count(), 24);
    let stale = last.lock().unwrap().clone().unwrap();
    assert!(matches!(
        stale
            .record_observation(OpaquePayload::new("receipt", 1, "late").unwrap())
            .await,
        Err(pl_core::thread::ThreadError::TaskAccessExpired)
    ));
    thread.close().await.unwrap();
}
