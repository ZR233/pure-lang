mod support;

use std::sync::Mutex;

use pl_core::context::OpaquePayload;
use pl_core::model::{
    DynModelSession, ModelError, ModelFailureKind, ModelRequest, ModelSession, PreparedModelCall,
};
use pl_core::thread::{ThreadError, ThreadHandle, ToolOutcome, TurnOutcome, TurnState};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, Tool, ToolError},
};
use tokio::sync::oneshot;

use support::{response, turn};

#[derive(Debug)]
struct CancellationAwareTool {
    entered: Mutex<Option<oneshot::Sender<()>>>,
}

impl Tool for CancellationAwareTool {
    async fn execute(
        &self,
        _input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).expect("receiver must still exist");
        }
        context.cancellation.cancelled().await;
        Ok(ToolOutput::new(
            OpaquePayload::text("observed after cancellation"),
            vec![support::text(
                "tool result must not be delivered as success",
            )],
        ))
    }
}

struct BlockingFirstCall {
    entered: Option<oneshot::Sender<()>>,
}

impl ModelSession for BlockingFirstCall {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        if let Some(entered) = self.entered.take() {
            Ok(PreparedModelCall::new(async move {
                entered.send(()).expect("receiver must still exist");
                request.cancellation.cancelled().await;
                Err(ModelError {
                    details: None,
                    kind: ModelFailureKind::Cancelled,
                    usage: Default::default(),
                    source: None,
                })
            }))
        } else {
            Ok(PreparedModelCall::new(async move {
                Ok(response(&request, Vec::new()))
            }))
        }
    }

    async fn close(&mut self) -> Result<(), ModelError> {
        Ok(())
    }
}

#[tokio::test]
async fn cancelling_an_active_turn_commits_its_termination_and_leaves_thread_usable() {
    let (entered, waiting) = oneshot::channel();
    let thread = ThreadHandle::start(
        "cancelled-thread".into(),
        DynModelSession::new(BlockingFirstCall {
            entered: Some(entered),
        }),
    )
    .unwrap();
    let input = turn("cancel-me");
    let cancellation = input.cancellation.clone();
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(input).await }
    });
    waiting.await.expect("model invocation started");
    cancellation.cancel();
    assert!(matches!(runner.await.unwrap(), Err(ThreadError::Cancelled)));
    assert!(thread.effects().await.unwrap().iter().any(|effect| {
        effect
            .turn
            .as_ref()
            .is_some_and(|turn| turn.turn_id == "cancel-me" && turn.state == TurnState::Cancelled)
    }));

    let next = thread.run_turn(turn("after-cancel")).await.unwrap();
    assert_eq!(next.outcome, TurnOutcome::Completed);
    assert_eq!(next.model_steps, 1);
    assert!(thread.snapshot().model_available);
    thread.close().await.unwrap();
}

#[tokio::test]
async fn cancelling_a_running_tool_retains_its_result_as_a_cancelled_delivery() {
    let (model, _) = support::ScriptedModel::new(&["slow"]);
    let thread = ThreadHandle::start("tool-thread".into(), DynModelSession::new(model)).unwrap();
    let (entered, waiting) = oneshot::channel();
    thread
        .register_tools(vec![
            Registration::new(
                "slow".into(),
                OpaquePayload::text("Slow tool"),
                CancellationAwareTool {
                    entered: Mutex::new(Some(entered)),
                },
            )
            .unwrap()
            .foreground_coexisting(),
        ])
        .await
        .unwrap();
    let input = turn("interrupt-tool");
    let cancellation = input.cancellation.clone();
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(input).await }
    });
    waiting.await.expect("tool invocation started");
    cancellation.cancel();
    assert!(matches!(runner.await.unwrap(), Err(ThreadError::Cancelled)));

    let deliveries: Vec<_> = thread
        .effects()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|effect| effect.deliveries.to_vec())
        .collect();
    assert_eq!(deliveries.len(), 1);
    assert!(matches!(deliveries[0].outcome, ToolOutcome::Cancelled));
    assert_eq!(
        deliveries[0].output.payload().content(),
        "observed after cancellation"
    );
    assert_ne!(
        deliveries[0].delivered_context.as_slice(),
        deliveries[0].output.context()
    );
    thread.close().await.unwrap();
}
