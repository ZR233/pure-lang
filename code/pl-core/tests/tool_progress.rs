//! Public-API coverage of the typed tool live-output port.
//!
//! These cases exercise only what a tool producer and a live projection actually call: reporting an
//! increment under a stable producer-chosen identity, sharing one content block so a consumer
//! recovers only the bytes it has not delivered yet, a bounded window that arrives as an explicit
//! whole replacement, the live bound, and a late report against a call that already finished.

mod support;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use pl_core::{
    context::OpaquePayload,
    model::{ContentBlock, ContentIncrement, DynModelSession, ToolProgress, ToolProgressUpdate},
    thread::{TaskAccess, ThreadError, ThreadHandle},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use tokio::sync::{Notify, mpsc};

use support::{ScriptedModel, text, turn};

/// The producer-chosen identity of the preview; core treats it as opaque.
const PART: &str = "command-output";

/// One simulated preview step: tell the test the report was accepted, then wait to be released.
#[derive(Debug)]
struct Step {
    reached: mpsc::UnboundedSender<usize>,
    resume: Arc<Notify>,
}

impl Step {
    async fn hand_off(&self, index: usize) {
        self.reached
            .send(index)
            .expect("the test observes the step");
        self.resume.notified().await;
    }
}

/// A tool that reports a scripted sequence of live-output increments.
#[derive(Debug)]
struct ProgressTool {
    updates: Vec<ToolProgressUpdate>,
    step: Step,
    /// Whether the owner accepted every report, so a test can assert which increment it took.
    outcomes: Arc<Mutex<Vec<bool>>>,
    /// The call's task access, kept so a test can report after the call itself returned.
    retained: Arc<Mutex<Option<TaskAccess>>>,
}

impl Tool for ProgressTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its task access");
        *self.retained.lock().unwrap() = Some(tasks.clone());
        for (index, update) in self.updates.iter().enumerate() {
            let accepted = tasks.report_output(update.clone()).await.is_ok();
            self.outcomes.lock().unwrap().push(accepted);
            self.step.hand_off(index).await;
        }
        Ok(ToolOutput::new(input.clone(), vec![text(input.content())]))
    }
}

struct Harness {
    thread: ThreadHandle,
    runner: tokio::task::JoinHandle<Result<pl_core::thread::TurnCompletion, ThreadError>>,
    steps: mpsc::UnboundedReceiver<usize>,
    resume: Arc<Notify>,
    outcomes: Arc<Mutex<Vec<bool>>>,
    retained: Arc<Mutex<Option<TaskAccess>>>,
}

impl Harness {
    async fn start(updates: Vec<ToolProgressUpdate>) -> Self {
        let (model, _requests) = ScriptedModel::new(&["stream"]);
        let thread =
            ThreadHandle::start("tool-progress".into(), DynModelSession::new(model)).unwrap();
        let (reached, steps) = mpsc::unbounded_channel();
        let resume = Arc::new(Notify::new());
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let retained = Arc::new(Mutex::new(None));
        thread
            .register_tools(vec![
                Registration::new(
                    "stream".into(),
                    OpaquePayload::text("Tool stream"),
                    ProgressTool {
                        updates,
                        step: Step {
                            reached,
                            resume: resume.clone(),
                        },
                        outcomes: outcomes.clone(),
                        retained: retained.clone(),
                    },
                )
                .unwrap(),
            ])
            .await
            .unwrap();
        let runner = tokio::spawn({
            let thread = thread.clone();
            async move { thread.run_turn(turn("live")).await }
        });
        Self {
            thread,
            runner,
            steps,
            resume,
            outcomes,
            retained,
        }
    }

    /// Waits for one accepted report, then releases the tool for the next one.
    async fn next_step(&mut self) -> usize {
        within(self.steps.recv())
            .await
            .expect("the tool reports its scripted step")
    }

    fn release(&self) {
        self.resume.notify_one();
    }

    /// The live output of this Thread's only running call.
    fn progress(&self) -> ToolProgress {
        let snapshot = self.thread.snapshot();
        let running = snapshot
            .tasks
            .values()
            .find(|task| task.status == pl_core::thread::task::TaskStatus::Running)
            .expect("the running call is observable");
        snapshot
            .tool_progress
            .get(&running.id)
            .cloned()
            .expect("the accepted output is published with its running call")
    }
}

async fn within<F: std::future::Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("the live-output port must not block the Thread")
}

#[tokio::test]
async fn appends_share_one_stable_identity_and_deliver_only_the_missing_bytes() {
    let mut harness = Harness::start(vec![
        ToolProgressUpdate::append(PART, "hel"),
        ToolProgressUpdate::append(PART, "lo"),
        // An increment that changes nothing must not advance the observation.
        ToolProgressUpdate::append(PART, ""),
    ])
    .await;

    assert_eq!(harness.next_step().await, 0);
    let first = harness.progress();
    assert_eq!(
        first.len(),
        1,
        "one producer identity stays one observation"
    );
    let first_part = first.part(PART).expect("the producer identity is kept");
    assert_eq!(first_part.text(), "hel");
    assert_eq!(first.version(), 1);
    assert_eq!(first_part.version(), 1);
    let baseline = first_part.baseline();

    harness.release();
    assert_eq!(harness.next_step().await, 1);
    let second = harness.progress();
    assert_eq!(
        second.len(),
        1,
        "the same identity is never split into two parts"
    );
    let second_part = second.part(PART).expect("the same producer identity");
    assert_eq!(second_part.text(), "hello");
    assert_eq!(second.version(), 2);
    // The second observation still extends the first one's shared chain, so a consumer that
    // delivered "hel" recovers exactly the missing bytes and copies no full text to find out.
    assert_eq!(
        ContentBlock::appended_since(second_part.content(), first_part.content()).as_deref(),
        Some("lo")
    );
    assert_eq!(
        second_part.increment_since(&baseline),
        ContentIncrement::Append("lo".to_owned())
    );
    assert_eq!(
        second_part.increment_since(&second_part.baseline()),
        ContentIncrement::Current
    );

    harness.release();
    assert_eq!(harness.next_step().await, 2);
    let unchanged = harness.progress();
    assert_eq!(
        unchanged.version(),
        2,
        "an empty increment is not a new content version"
    );
    assert_eq!(unchanged.content().text(), "hello");

    harness.release();
    let completion = within(harness.runner)
        .await
        .expect("the turn completes")
        .unwrap();
    assert_eq!(completion.model_steps, 2);
    // The call is terminal, so its live window leaves with it instead of staying as a second copy
    // of the canonical result.
    assert!(harness.thread.snapshot().tool_progress.is_empty());
}

#[tokio::test]
async fn a_bounded_window_replaces_the_whole_body_instead_of_appending_onto_a_dropped_head() {
    let replacement = "[omitted]\nworld";
    let mut harness = Harness::start(vec![
        ToolProgressUpdate::append(PART, "hello"),
        ToolProgressUpdate::replace(PART, replacement),
    ])
    .await;

    assert_eq!(harness.next_step().await, 0);
    let first = harness.progress();
    let baseline = first.part(PART).expect("the observed part").baseline();

    harness.release();
    assert_eq!(harness.next_step().await, 1);
    let second = harness.progress();
    let part = second.part(PART).expect("the same producer identity");
    assert_eq!(part.text(), replacement);
    assert_eq!(second.version(), 2);
    // The observed body no longer extends the delivered one, so a consumer must replace its whole
    // copy: appending the new bytes to the old prefix would silently mix two bodies.
    assert_eq!(second.content().len(), replacement.len());
    assert_eq!(
        ContentBlock::appended_since(part.content(), first.part(PART).unwrap().content()),
        None
    );
    assert_eq!(
        part.increment_since(&baseline),
        ContentIncrement::Replace(replacement.to_owned())
    );
    assert_eq!(second.version(), 2, "a replacement is one observation");

    harness.release();
    within(harness.runner)
        .await
        .expect("the turn completes")
        .unwrap();
}

#[tokio::test]
async fn an_increment_over_the_live_bound_is_rejected_and_keeps_the_accepted_output() {
    let oversized = "y".repeat(pl_core::model::MAX_TOOL_PROGRESS_BYTES as usize);
    let mut harness = Harness::start(vec![
        ToolProgressUpdate::append(PART, "keep"),
        ToolProgressUpdate::append(PART, oversized),
    ])
    .await;

    assert_eq!(harness.next_step().await, 0);

    harness.release();
    assert_eq!(harness.next_step().await, 1);
    assert_eq!(
        *harness.outcomes.lock().unwrap(),
        vec![true, false],
        "the live window is bounded instead of growing with the operation"
    );
    // The refused increment never becomes resident: the bytes that were already accepted stay the
    // running call's authoritative output.
    let progress = harness.progress();
    assert_eq!(progress.content().text(), "keep");
    assert_eq!(progress.version(), 1);

    // A producer that would exceed the observed-window ceiling is not rolling its window over, so
    // it is cancelled with the bytes already accepted and the Thread fails closed with the same
    // typed pressure fault the reliable-budget refusal latches — instead of quietly dropping the
    // excess while the tool keeps producing. The call is cancelled while it is still in flight, so
    // the fault must already be observable here; the Turn itself then parks at its next storage
    // safety point until an explicit resume, exactly like a reliable-budget truncation.
    let snapshot = harness.thread.snapshot();
    assert_eq!(
        snapshot.persistence.fault,
        Some(pl_core::thread::cold::StorageFaultKind::QueueFull),
        "an exhausted live window travels as a typed pressure fault, not error text"
    );
    assert!(
        snapshot.persistence.resume_required,
        "the Thread must fail closed and wait for an explicit resume"
    );
    // The tool stays suspended on its scripted step, so the Turn is left waiting for it rather than
    // completing; the harness is dropped at the end of the test, so nothing else is awaited here.
}

#[tokio::test]
async fn a_finished_call_rejects_a_late_increment_instead_of_reviving_its_preview() {
    let mut harness = Harness::start(vec![ToolProgressUpdate::append(PART, "final")]).await;

    assert_eq!(harness.next_step().await, 0);
    harness.release();
    within(harness.runner)
        .await
        .expect("the turn completes")
        .unwrap();
    assert!(
        harness.thread.snapshot().tool_progress.is_empty(),
        "a terminal call keeps no live window"
    );

    let access = harness
        .retained
        .lock()
        .unwrap()
        .clone()
        .expect("the call kept its access");
    let error = within(access.report_output(ToolProgressUpdate::append(PART, "late")))
        .await
        .expect_err("a finished call must reject a late live-output report");
    assert!(matches!(error, ThreadError::TaskAccessExpired));
    assert!(
        harness.thread.snapshot().tool_progress.is_empty(),
        "a rejected late increment must not revive the preview"
    );
}

/// A tool whose durable capture could not be stored, reported through the typed boundary.
///
/// It reports one accepted increment first, so the test can prove the already-accepted output is
/// kept, then fails with the exact storage category a real backend would name instead of a string.
#[derive(Debug)]
struct StorageFaultTool;

impl Tool for StorageFaultTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = context
            .tasks
            .clone()
            .expect("a running call owns its access");
        within(tasks.report_output(ToolProgressUpdate::append(PART, "partial")))
            .await
            .expect("the accepted increment is charged before the fault");
        let output = ToolOutput::new(input.clone(), vec![text("partial")]);
        let source = pl_core::thread::cold::ColdStoreError {
            source: Box::new(std::io::Error::other("capture write failed")),
        };
        Err(
            ToolError::new(pl_core::thread::cold::OutputStorageFault::new(
                pl_core::thread::cold::StorageFaultKind::WriteFailed,
                Arc::new(source),
            ))
            .with_output(output),
        )
    }
}

/// A tool that cannot store its reliable output latches the typed fault and keeps what it accepted.
///
/// The failure is a real storage fact, not a recoverable tool error: the Thread must fail closed and
/// pause further admission, while the output already streamed stays delivered with the failed result
/// instead of being dropped.
#[tokio::test]
async fn a_tool_reliable_output_storage_fault_latches_and_keeps_accepted_output() {
    let (model, _requests) = ScriptedModel::new(&["store"]);
    let thread =
        ThreadHandle::start("tool-storage-fault".into(), DynModelSession::new(model)).unwrap();
    thread
        .register_tools(vec![
            Registration::new(
                "store".into(),
                OpaquePayload::text("Tool store"),
                StorageFaultTool,
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    // The tool returns a typed storage fault, so the failed result is committed and the Turn then
    // parks at its next storage safety point. Observe the fault through the published snapshot and
    // stop driving the parked runner instead of awaiting a completion that never arrives.
    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(turn("storage")).await }
    });
    let mut subscription = thread.subscribe();
    let faulted = within(async {
        loop {
            let snapshot = subscription.next().await.expect("thread stays open");
            if snapshot.persistence.resume_required {
                return snapshot;
            }
        }
    })
    .await;
    assert_eq!(
        faulted.persistence.fault,
        Some(pl_core::thread::cold::StorageFaultKind::WriteFailed),
        "the producer's typed storage category is latched, not guessed from error text"
    );
    assert!(
        faulted.persistence.resume_required,
        "a hard storage fault pauses new admission instead of letting the Thread continue"
    );
    drop(runner);
    // The failed result is enrolled by a commit that follows the latch, so wait for the effect that
    // carries it. A settled result is committed history, read the same way a cancelled call's
    // retained output is proven; yielding lets the detached runner reach that commit.
    let deliveries = within(async {
        loop {
            let deliveries: Vec<_> = thread
                .effects()
                .await
                .unwrap()
                .into_iter()
                .flat_map(|effect| effect.deliveries.to_vec())
                .collect();
            if deliveries
                .iter()
                .any(|delivery| delivery.tool_id == "store")
            {
                return deliveries;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let delivery = deliveries
        .iter()
        .find(|delivery| delivery.tool_id == "store")
        .expect("the failed call's accepted output stays delivered");
    assert!(
        delivery.output.context().iter().any(|content| matches!(
            content,
            pl_core::context::ContextContent::Text { text } if text.as_ref() == "partial"
        )),
        "the bytes already accepted are kept with the failed result"
    );
    // The runner is parked at its storage safety point, so the detached Turn is left for the test
    // runtime to abort instead of awaiting a close that never settles.
}
