//! Public Thread contract: foreground ordering is independent from Solo admission.
use std::sync::{Arc, Mutex};

use pl_core::{
    context::{ContextContent, OpaquePayload},
    model::{
        DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput, ModelToolCall,
        PreparedModelCall, ToolCallMode,
    },
    thread::{ThreadError, ThreadHandle, TurnInput},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pretty_assertions::assert_eq;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

struct BatchModel {
    emitted: bool,
    control: bool,
}
impl ModelSession for BatchModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        assert_eq!(request.tool_call_mode, ToolCallMode::Parallel);
        let expected_solo = if self.control {
            vec!["second".to_owned()]
        } else {
            vec![]
        };
        assert_eq!(request.solo_tool_ids.as_ref(), expected_solo.as_slice());
        let names = if self.emitted {
            vec![]
        } else {
            vec!["first", "second"]
        };
        self.emitted = true;
        Ok(PreparedModelCall::new(async move {
            Ok(ModelStepOutput {
                attempt_id: request.attempt_id,
                base_context_revision: request.context.revision,
                content: vec![],
                tool_calls: names
                    .into_iter()
                    .map(|name| ModelToolCall {
                        call_id: format!("call-{name}"),
                        tool_id: name.into(),
                        arguments: OpaquePayload::text(name),
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

#[derive(Debug, Clone, Default)]
struct Writes {
    events: Arc<Mutex<Vec<&'static str>>>,
    started: Arc<Notify>,
    release: Arc<Notify>,
}
#[derive(Debug)]
struct Writer {
    first: bool,
    writes: Writes,
}
impl Tool for Writer {
    async fn execute(
        &self,
        _: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        if self.first {
            self.writes.events.lock().unwrap().push("first-start");
            self.writes.started.notify_one();
            tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => {
                    self.writes.events.lock().unwrap().push("first-cancelled");
                    return Err(ToolError::new(ThreadError::Cancelled));
                }
                _ = self.writes.release.notified() => {}
            }
            self.writes.events.lock().unwrap().push("first-finish");
        } else {
            self.writes.events.lock().unwrap().push("second");
        }
        Ok(ToolOutput::new(
            OpaquePayload::text("saved"),
            vec![ContextContent::Text {
                text: "saved".into(),
            }],
        ))
    }
}
async fn thread(writes: &Writes, control: bool) -> ThreadHandle {
    let thread = ThreadHandle::start(
        "foreground".into(),
        DynModelSession::new(BatchModel {
            emitted: false,
            control,
        }),
    )
    .unwrap();
    let first = Registration::new(
        "first".into(),
        OpaquePayload::text("writer"),
        Writer {
            first: true,
            writes: writes.clone(),
        },
    )
    .unwrap()
    .foreground_coexisting();
    let second = Registration::new(
        "second".into(),
        OpaquePayload::text("writer"),
        Writer {
            first: false,
            writes: writes.clone(),
        },
    )
    .unwrap()
    .foreground_coexisting();
    let second = if control {
        second.with_extension_updates()
    } else {
        second
    };
    thread.register_tools(vec![first, second]).await.unwrap();
    thread
}
fn input(cancellation: CancellationToken) -> TurnInput {
    TurnInput {
        turn_id: "turn".into(),
        attempt_prefix: "attempt".into(),
        content: vec![],
        max_model_steps: std::num::NonZeroU32::new(2).unwrap(),
        cancellation,
    }
}

#[tokio::test(start_paused = true)]
async fn coexisting_writes_wait_in_provider_order_even_after_background_ack_deadline() {
    let writes = Writes::default();
    let thread = thread(&writes, false).await;
    let owner = thread.clone();
    let running =
        tokio::spawn(async move { owner.run_turn(input(CancellationToken::new())).await });
    writes.started.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    tokio::task::yield_now().await;
    assert_eq!(*writes.events.lock().unwrap(), vec!["first-start"]);
    assert!(!running.is_finished());
    writes.release.notify_one();
    running.await.unwrap().unwrap();
    assert_eq!(
        *writes.events.lock().unwrap(),
        vec!["first-start", "first-finish", "second"]
    );
    thread.close().await.unwrap();
}

#[tokio::test]
async fn cancelling_foreground_batch_stops_first_writer_without_starting_second() {
    let writes = Writes::default();
    let thread = thread(&writes, false).await;
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let owner = thread.clone();
    let running = tokio::spawn(async move { owner.run_turn(input(token)).await });
    writes.started.notified().await;
    cancellation.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), running)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(ThreadError::Cancelled)));
    assert_eq!(
        *writes.events.lock().unwrap(),
        vec!["first-start", "first-cancelled"]
    );
    thread.close().await.unwrap();
}

#[tokio::test]
async fn coexisting_foreground_does_not_weaken_control_solo_admission() {
    let writes = Writes::default();
    let thread = thread(&writes, true).await;
    assert!(matches!(
        thread.run_turn(input(CancellationToken::new())).await,
        Err(ThreadError::InvalidOutput)
    ));
    assert!(writes.events.lock().unwrap().is_empty());
    thread.close().await.unwrap();
}
