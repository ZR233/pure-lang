//! Public-API behaviour of the explicit model execution phase.
//!
//! The phase must describe the real call boundary of one attempt: preparing the request is not the
//! provider call, admission is not a provider wait, and only the running prepared call may be read
//! as "waiting for the model implementation". Every exit path (success, failure, cancellation) must
//! clear the phase so a projection never reads a phase left over from a finished step.

use std::sync::Arc;

use pl_core::context::ContextContent;
use pl_core::model::{
    DynModelSession, ModelError, ModelFailureKind, ModelRequest, ModelSession, ModelStepOutput,
    ModelToolCall, PreparedModelCall,
};
use pl_core::thread::{
    ModelExecutionPhase, ModelStepLimit, ThreadError, ThreadHandle, TurnInput, TurnOutcome,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

fn text(value: &str) -> ContextContent {
    ContextContent::Text {
        text: Arc::from(value),
    }
}

fn turn(id: &str) -> TurnInput {
    TurnInput {
        turn_id: id.into(),
        attempt_prefix: format!("attempt-{id}"),
        content: vec![text("Please handle this request")],
        max_model_steps: ModelStepLimit::Limited(3.try_into().expect("nonzero limit")),
        cancellation: CancellationToken::new(),
    }
}

fn response(request: &ModelRequest, tool_calls: Vec<ModelToolCall>) -> ModelStepOutput {
    ModelStepOutput {
        attempt_id: request.attempt_id.clone(),
        base_context_revision: request.context.revision,
        content: vec![text(if tool_calls.is_empty() {
            "Answer completed"
        } else {
            "Calling tools"
        })],
        tool_calls,
        private_context: None,
        usage: Default::default(),
    }
}

/// A session whose `prepare` and executed call are each held at an explicit gate.
///
/// The gates are one-shot: each of them can be observed and released exactly once, so the test knows
/// which boundary the driver is waiting at instead of guessing from timing.
struct GatedModel {
    prepare_entered: Option<oneshot::Sender<()>>,
    prepare_release: Option<oneshot::Receiver<()>>,
    execute_entered: Option<oneshot::Sender<()>>,
    execute_release: Option<oneshot::Receiver<()>>,
    prepare_failure: Option<ModelFailureKind>,
    execute_failure: Option<ModelFailureKind>,
}

fn model_failure(kind: ModelFailureKind) -> ModelError {
    ModelError {
        details: None,
        kind,
        usage: Default::default(),
        source: None,
    }
}

/// One gated provider call: report entry, wait for the gate (or the Turn cancellation) and then either
/// fail with the injected kind or return the scripted response.
async fn gated_call(
    entered: Option<oneshot::Sender<()>>,
    release: Option<oneshot::Receiver<()>>,
    failure_kind: Option<ModelFailureKind>,
    request: ModelRequest,
) -> Result<ModelStepOutput, ModelError> {
    if let Some(entered) = entered {
        entered
            .send(())
            .expect("execute gate owner must still exist");
    }
    if let Some(release) = release {
        // Either the owner opens the gate or the Turn is cancelled: both end this call without
        // inventing a result.
        let cancelled = tokio::select! {
            _ = release => false,
            () = request.cancellation.cancelled() => true,
        };
        if cancelled {
            return Err(model_failure(ModelFailureKind::Cancelled));
        }
    }
    match failure_kind {
        Some(kind) => Err(model_failure(kind)),
        None => Ok(response(&request, Vec::new())),
    }
}

impl ModelSession for GatedModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        if let Some(entered) = self.prepare_entered.take() {
            entered
                .send(())
                .expect("prepare gate owner must still exist");
        }
        if let Some(release) = self.prepare_release.take() {
            release.await.expect("prepare gate must be opened");
        }
        if let Some(kind) = self.prepare_failure.take() {
            return Err(model_failure(kind));
        }
        let entered = self.execute_entered.take();
        let release = self.execute_release.take();
        let failure_kind = self.execute_failure.take();
        Ok(PreparedModelCall::new(gated_call(
            entered,
            release,
            failure_kind,
            request,
        )))
    }

    async fn close(&mut self) -> Result<(), ModelError> {
        Ok(())
    }
}

impl GatedModel {
    fn idle() -> Self {
        Self {
            prepare_entered: None,
            prepare_release: None,
            execute_entered: None,
            execute_release: None,
            prepare_failure: None,
            execute_failure: None,
        }
    }
}

#[tokio::test]
async fn preparing_the_request_is_not_reported_as_the_running_call() {
    let (prepare_entered, prepare_wait) = oneshot::channel();
    let (prepare_gate, prepare_release) = oneshot::channel();
    let (execute_entered, execute_wait) = oneshot::channel();
    let (execute_gate, execute_release) = oneshot::channel();
    let thread = ThreadHandle::start(
        "phase-thread".into(),
        DynModelSession::new(GatedModel {
            prepare_entered: Some(prepare_entered),
            prepare_release: Some(prepare_release),
            execute_entered: Some(execute_entered),
            execute_release: Some(execute_release),
            ..GatedModel::idle()
        }),
    )
    .unwrap();
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("phase-turn")).await }
    });

    // The request is being prepared inside the model implementation; nothing has been dispatched yet.
    prepare_wait.await.expect("prepare must be entered");
    assert_eq!(
        thread.snapshot().model_execution,
        Some(ModelExecutionPhase::PreparingRequest)
    );
    // A mailbox commit republishes the snapshot while prepare owns the session.
    // Borrowing it for preparation must not look like a missing model binding.
    thread
        .send_message(pl_core::thread::inbox::ThreadMessage {
            id: "during-prepare".into(),
            source_id: "observer".into(),
            payload: pl_core::context::OpaquePayload::text("status update"),
            context: Vec::new(),
        })
        .await
        .unwrap();
    assert!(thread.snapshot().model_available);

    prepare_gate.send(()).expect("prepare gate must be open");
    execute_wait.await.expect("execute must be entered");
    assert_eq!(
        thread.snapshot().model_execution,
        Some(ModelExecutionPhase::Running)
    );
    assert!(thread.snapshot().model_available);

    execute_gate.send(()).expect("execute gate must be open");
    let completion = runner.await.unwrap().unwrap();
    assert_eq!(completion.outcome, TurnOutcome::Completed);
    assert_eq!(thread.snapshot().model_execution, None);
    thread.close().await.unwrap();
}

#[tokio::test]
async fn a_failed_step_clears_the_execution_phase_and_leaves_the_thread_usable() {
    let thread = ThreadHandle::start(
        "failed-thread".into(),
        DynModelSession::new(GatedModel {
            execute_failure: Some(ModelFailureKind::InvalidResponse),
            ..GatedModel::idle()
        }),
    )
    .unwrap();

    let error = thread.run_turn(turn("failed-turn")).await.unwrap_err();
    assert!(matches!(error, ThreadError::Model(_)));
    assert_eq!(thread.snapshot().model_execution, None);

    let next = thread.run_turn(turn("after-failure")).await.unwrap();
    assert_eq!(next.outcome, TurnOutcome::Completed);
    assert_eq!(thread.snapshot().model_execution, None);
    thread.close().await.unwrap();
}

/// Request preparation failing is another exit path that must not leave a phase behind: the phase
/// claimed `PreparingRequest`, and a projection reading it after the failure would report a driver
/// that is no longer waiting for anything.
#[tokio::test]
async fn a_failed_request_preparation_clears_the_execution_phase() {
    let thread = ThreadHandle::start(
        "prepare-failed-thread".into(),
        DynModelSession::new(GatedModel {
            prepare_failure: Some(ModelFailureKind::Unavailable),
            ..GatedModel::idle()
        }),
    )
    .unwrap();

    let error = thread
        .run_turn(turn("prepare-failed-turn"))
        .await
        .unwrap_err();
    assert!(matches!(error, ThreadError::Model(_)));
    assert_eq!(thread.snapshot().model_execution, None);

    let next = thread
        .run_turn(turn("after-prepare-failure"))
        .await
        .unwrap();
    assert_eq!(next.outcome, TurnOutcome::Completed);
    assert_eq!(thread.snapshot().model_execution, None);
    thread.close().await.unwrap();
}

#[tokio::test]
async fn a_cancelled_execution_clears_the_execution_phase() {
    let (execute_entered, execute_wait) = oneshot::channel();
    let (execute_gate, execute_release) = oneshot::channel();
    let thread = ThreadHandle::start(
        "cancelled-phase-thread".into(),
        DynModelSession::new(GatedModel {
            execute_entered: Some(execute_entered),
            execute_release: Some(execute_release),
            ..GatedModel::idle()
        }),
    )
    .unwrap();
    let input = turn("cancel-phase-turn");
    let cancellation = input.cancellation.clone();
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(input).await }
    });

    execute_wait.await.expect("execute must be entered");
    assert_eq!(
        thread.snapshot().model_execution,
        Some(ModelExecutionPhase::Running)
    );
    cancellation.cancel();

    assert!(matches!(runner.await.unwrap(), Err(ThreadError::Cancelled)));
    assert_eq!(thread.snapshot().model_execution, None);
    // The gate is still owned by the test, so the cancelled call can never be mistaken for a result.
    drop(execute_gate);

    let next = thread.run_turn(turn("after-cancel")).await.unwrap();
    assert_eq!(next.outcome, TurnOutcome::Completed);
    assert_eq!(thread.snapshot().model_execution, None);
    thread.close().await.unwrap();
}
