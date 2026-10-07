mod support;
#[path = "support/tool.rs"]
mod tool_support;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use pl_core::context::{ContextContent, ContextSource, OpaquePayload};
use pl_core::model::DynModelSession;
use pl_core::thread::{
    ModelStepLimit, ThreadError, ThreadHandle, TurnOutcome, TurnState,
    cold::{
        ColdStore, ColdStoreError, ColdStoreHandle, StorageFaultKind, StoragePressure, ThreadWrite,
    },
    inbox::ThreadMessage,
    input::InputDriverOptions,
};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, Tool, ToolError},
};
use tokio::sync::watch;

use support::{ScriptedModel, turn};
use tool_support::tool;

#[derive(Debug)]
struct FenceTool(Arc<Mutex<Vec<u64>>>);

impl Tool for FenceTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.0.lock().unwrap().push(context.history_fence);
        Ok(ToolOutput::new(input, vec![]))
    }
}

#[tokio::test]
async fn tool_receives_the_committed_start_fact_as_its_history_fence() {
    let (model, _) = ScriptedModel::new(&["fenced"]);
    let thread = ThreadHandle::start("fence".into(), DynModelSession::new(model)).unwrap();
    let fences = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![
            Registration::new(
                "fenced".into(),
                OpaquePayload::text("fenced tool"),
                FenceTool(fences.clone()),
            )
            .unwrap()
            .foreground_coexisting(),
        ])
        .await
        .unwrap();
    thread.run_turn(turn("fenced-turn")).await.unwrap();

    let started = thread
        .effects()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|effect| effect.tasks.to_vec())
        .find_map(|task| task.started_sequence)
        .unwrap();
    assert_eq!(*fences.lock().unwrap(), vec![started]);
    thread.close().await.unwrap();
}

#[derive(Debug, Clone)]
struct FenceStore {
    entered: watch::Sender<Option<u64>>,
    released: watch::Sender<bool>,
    admitted: Arc<Mutex<Vec<u64>>>,
    durable: Arc<AtomicU64>,
    fail_once: Arc<AtomicBool>,
}

impl ColdStore for FenceStore {
    fn admit(&self, _thread_id: &str, write: ThreadWrite) -> Result<(), ColdStoreError> {
        self.admitted.lock().unwrap().push(write.effect.sequence);
        Ok(())
    }

    fn pressure(&self, _thread_id: &str) -> StoragePressure {
        StoragePressure {
            durable_sequence: self.durable.load(Ordering::SeqCst),
            ..Default::default()
        }
    }

    async fn flush(&self, _thread_id: &str, sequence: u64) -> Result<(), ColdStoreError> {
        self.entered.send_replace(Some(sequence));
        let mut released = self.released.subscribe();
        while !*released.borrow_and_update() {
            released.changed().await.unwrap();
        }
        assert!(self.admitted.lock().unwrap().contains(&sequence));
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(ColdStoreError {
                source: Box::new(std::io::Error::other("fence write failed")),
            });
        }
        self.durable.fetch_max(sequence, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug)]
struct SideEffectTool(Arc<AtomicBool>);

impl Tool for SideEffectTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        _context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.0.store(true, Ordering::SeqCst);
        Ok(ToolOutput::new(input, vec![]))
    }
}

#[tokio::test]
async fn attached_store_must_flush_the_start_fact_before_a_tool_runs() {
    let (model, _) = ScriptedModel::new(&["fenced"]);
    let thread = ThreadHandle::start("durable-fence".into(), DynModelSession::new(model)).unwrap();
    let (entered, mut observed) = watch::channel(None);
    let (released, _) = watch::channel(false);
    let executed = Arc::new(AtomicBool::new(false));
    let store = FenceStore {
        entered,
        released: released.clone(),
        admitted: Arc::new(Mutex::new(Vec::new())),
        durable: Arc::new(AtomicU64::new(0)),
        fail_once: Arc::new(AtomicBool::new(false)),
    };
    thread
        .attach_storage(ColdStoreHandle::new(store))
        .await
        .unwrap();
    thread
        .register_tools(vec![
            Registration::new(
                "fenced".into(),
                OpaquePayload::text("fenced tool"),
                SideEffectTool(executed.clone()),
            )
            .unwrap()
            .foreground_coexisting(),
        ])
        .await
        .unwrap();

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("fenced-turn")).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while observed.borrow().is_none() {
            observed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(!executed.load(Ordering::SeqCst));
    assert!(!runner.is_finished());
    released.send_replace(true);
    tokio::time::timeout(std::time::Duration::from_secs(5), runner)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(executed.load(Ordering::SeqCst));
    thread.close().await.unwrap();
}

#[tokio::test]
async fn failed_tool_fence_blocks_the_executor_and_latches_storage_fault() {
    let (model, _) = ScriptedModel::new(&["fenced"]);
    let thread = ThreadHandle::start("failed-fence".into(), DynModelSession::new(model)).unwrap();
    let (entered, _) = watch::channel(None);
    let (released, _) = watch::channel(true);
    let executed = Arc::new(AtomicBool::new(false));
    let store = FenceStore {
        entered,
        released,
        admitted: Arc::new(Mutex::new(Vec::new())),
        durable: Arc::new(AtomicU64::new(0)),
        fail_once: Arc::new(AtomicBool::new(true)),
    };
    thread
        .attach_storage(ColdStoreHandle::new(store))
        .await
        .unwrap();
    thread
        .register_tools(vec![
            Registration::new(
                "fenced".into(),
                OpaquePayload::text("fenced tool"),
                SideEffectTool(executed.clone()),
            )
            .unwrap()
            .foreground_coexisting(),
        ])
        .await
        .unwrap();

    let mut snapshots = thread.subscribe();
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("failed-turn")).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let snapshot = snapshots.next().await.unwrap();
            if snapshot.persistence.fault == Some(StorageFaultKind::WriteFailed) {
                assert!(snapshot.persistence.resume_required);
                break;
            }
        }
    })
    .await
    .unwrap();
    assert!(!executed.load(Ordering::SeqCst));
    if !runner.is_finished() {
        let _ = thread.interrupt_turn(Some("failed-turn".into())).await;
    }
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), runner)
        .await
        .unwrap();
    thread.flush().await.unwrap();
    thread.close().await.unwrap();
}

#[tokio::test]
async fn a_turn_commits_user_model_and_tool_facts_in_call_order() {
    let (model, requests) = ScriptedModel::new(&["first", "second"]);
    let thread = ThreadHandle::start("conversation".into(), DynModelSession::new(model)).unwrap();
    let log = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![tool("first", &log), tool("second", &log)])
        .await
        .unwrap();

    let completion = thread.run_turn(turn("question")).await.unwrap();
    assert_eq!(completion.outcome, TurnOutcome::Completed);
    assert_eq!(completion.model_steps, 2);
    assert_eq!(
        *log.lock().unwrap(),
        ["question-first".to_string(), "question-second".to_string()]
    );
    {
        let seen = requests.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let results: Vec<_> = seen[1]
            .records
            .iter()
            .filter_map(|record| match &record.source {
                ContextSource::ToolResult { call_id, .. } => Some(call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(results, ["question-first", "question-second"]);
        let calls: Vec<_> = seen[1]
            .records
            .iter()
            .flat_map(|record| record.tool_calls.iter())
            .map(|call| call.arguments.content())
            .collect();
        assert_eq!(calls, ["  raw first\n", "  raw second\n"]);
    }

    let deliveries: Vec<_> = thread
        .effects()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|effect| effect.deliveries.to_vec())
        .collect();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[0].output.payload().content(), "  raw first\n");
    assert_eq!(deliveries[1].output.payload().content(), "  raw second\n");
    assert!(thread.snapshot().context.records.iter().any(|record| {
        record.source == ContextSource::Assistant
            && record.content.contains(&ContextContent::Text {
                text: Arc::from("Answer completed"),
            })
    }));
    assert!(thread.effects().await.unwrap().iter().any(|effect| {
        effect.turn.as_ref().is_some_and(|turn| {
            turn.turn_id == "question" && turn.state == TurnState::Finished(TurnOutcome::Completed)
        })
    }));
    thread.close().await.unwrap();
}

#[tokio::test]
async fn a_durable_message_receipt_wakes_only_its_unconsumed_message() {
    let (model, requests) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("parent".into(), DynModelSession::new(model)).unwrap();
    let message = ThreadMessage {
        kind: pl_core::context::AgentMessageKind::Report,
        id: "child-report".into(),
        source_id: "child".into(),
        payload: OpaquePayload::text("finished"),
        context: vec![support::text("finished")],
    };
    let sequence = thread.send_message(message).await.unwrap();
    let options = InputDriverOptions {
        max_model_steps: ModelStepLimit::Limited(2.try_into().unwrap()),
    };
    assert!(matches!(
        thread
            .wake_accepted_message("other", sequence, options)
            .await,
        Err(ThreadError::InvalidIdentity)
    ));
    assert_eq!(thread.snapshot().inbox.len(), 1);
    assert!(
        thread
            .wake_accepted_message("child-report", sequence, options)
            .await
            .unwrap()
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while thread.snapshot().consumed_messages < sequence {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !thread
            .wake_accepted_message("child-report", sequence, options)
            .await
            .unwrap()
    );
    assert_eq!(thread.snapshot().inbox_sequence, sequence);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].records.iter().any(|record| {
            record.source
                == ContextSource::AgentMessage {
                    source_id: "child".into(),
                    purpose: pl_core::context::AgentMessageKind::Report,
                }
        }));
    }
    thread.close().await.unwrap();
}
