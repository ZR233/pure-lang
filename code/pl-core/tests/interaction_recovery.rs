//! Pending interactions survive resource close and cold recovery as logical facts.
//!
//! These tests pin the close/recovery contract for submitted interactions: closing the runtime
//! resources of a Thread stops models, tools, background tasks and pending permissions, but it is
//! not an answer, a cancellation or a re-ask of an already submitted interaction. Only an explicit
//! resolve or cancel command settles it, and recovery replays neither the question nor execution.

mod support;

use pl_core::context::OpaquePayload;
use pl_core::model::DynModelSession;
use pl_core::thread::{
    StepInput, ThreadError, ThreadHandle, ToolDispatch,
    input::ThreadInput,
    interactions::{
        InteractionCancellation, InteractionRequest, InteractionResolution, InteractionResponse,
        InteractionState,
    },
};
use tokio_util::sync::CancellationToken;

use support::{ScriptedModel, text, turn};

fn question(id: &str, turn_id: &str) -> InteractionRequest {
    InteractionRequest {
        id: id.into(),
        turn_id: turn_id.into(),
        payload: OpaquePayload::text(format!("please answer {id}")),
    }
}

#[tokio::test]
async fn closing_the_thread_keeps_a_pending_interaction_recoverable() {
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("plan-pending".into(), DynModelSession::new(model)).unwrap();
    thread.run_turn(turn("turn-1")).await.unwrap();
    let asked = question("plan-1", "turn-1");
    thread.request_interaction(asked.clone()).await.unwrap();
    let pending = thread
        .snapshot()
        .interactions
        .get("plan-1")
        .cloned()
        .expect("submitted interaction is pending");
    assert!(matches!(pending.state, InteractionState::Pending));
    assert_eq!(pending.revision, 1);

    thread.close().await.unwrap();

    // Resource close is not an interaction answer: no committed effect cancelled the question.
    for effect in thread.effects().await.unwrap() {
        for record in effect.interactions.iter() {
            if record.request.id == "plan-1" {
                assert_eq!(record.state, InteractionState::Pending);
                assert_eq!(record.revision, 1);
            }
        }
    }

    // The restart DTO keeps the pending question, so the reopened session sees the same fact.
    let checkpoint = serde_json::to_vec(
        &thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap(),
    )
    .unwrap();
    let restored = serde_json::from_slice(&checkpoint).unwrap();
    let resumed =
        ThreadHandle::resume_without_model("plan-pending".into(), Some(restored)).unwrap();
    let record = resumed
        .snapshot()
        .interactions
        .get("plan-1")
        .cloned()
        .expect("recovery preserves the pending interaction");
    assert_eq!(record.request, asked);
    assert_eq!(record.revision, 1);
    assert_eq!(record.state, InteractionState::Pending);

    // Recovery re-asks nothing and executes nothing. The owner is restored without any model
    // session, so the only facts recovery could commit are its own settlements; asserting that it
    // committed no interaction fact proves the question was neither re-submitted nor answered.
    // Model-backed execution of the resolved continuation is covered by the test below.
    for effect in resumed.effects().await.unwrap() {
        assert!(effect.interactions.is_empty());
    }

    // A replayed submission of the same identity returns the same pending record instead of
    // minting a second question or a new revision.
    let replay = resumed.request_interaction(asked).await.unwrap();
    assert_eq!(replay.revision, 1);
    assert_eq!(replay.state, InteractionState::Pending);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn a_recovered_interaction_resolves_once_and_repeated_answers_stay_idempotent() {
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("plan-answer".into(), DynModelSession::new(model)).unwrap();
    thread
        .request_interaction(question("plan-1", "turn-1"))
        .await
        .unwrap();
    thread.close().await.unwrap();
    let checkpoint = serde_json::to_vec(
        &thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap(),
    )
    .unwrap();
    let restored = serde_json::from_slice(&checkpoint).unwrap();
    let (model, seen) = ScriptedModel::new(&[]);
    let resumed = ThreadHandle::resume(
        "plan-answer".into(),
        DynModelSession::new(model),
        Some(restored),
    )
    .unwrap();

    let resolution = InteractionResolution {
        continuation: Some(ThreadInput {
            id: "interaction:plan-1:continuation".into(),
            payload: OpaquePayload::text("continue with the approved plan"),
            context: vec![text("continue")],
        }),
        id: "plan-1".into(),
        expected_revision: 1,
        response: InteractionResponse {
            payload: OpaquePayload::text("approved"),
            context: vec![text("user approved the plan")],
        },
        mutations: Vec::new(),
    };
    let resolved = resumed
        .resolve_interaction(resolution.clone())
        .await
        .unwrap();
    assert_eq!(resolved.revision, 2);
    let continuation_count = |thread: &ThreadHandle| {
        thread
            .snapshot()
            .inputs
            .iter()
            .filter(|record| record.input.id == "interaction:plan-1:continuation")
            .count()
    };
    assert_eq!(continuation_count(&resumed), 1);

    // The same answer again returns the original receipt without enqueuing a second continuation.
    let repeated = resumed
        .resolve_interaction(resolution.clone())
        .await
        .unwrap();
    assert_eq!(repeated.revision, 2);
    assert_eq!(continuation_count(&resumed), 1);

    // A different answer for the same identity is a conflict, not a second resolution.
    let conflicting = InteractionResolution {
        response: InteractionResponse {
            payload: OpaquePayload::text("revised"),
            context: vec![text("user revised the plan")],
        },
        ..resolution
    };
    assert!(matches!(
        resumed.resolve_interaction(conflicting).await,
        Err(ThreadError::InvalidIdentity)
    ));
    assert_eq!(continuation_count(&resumed), 1);

    // Answering alone starts no model work; the continuation waits for explicit execution.
    assert!(seen.lock().unwrap().is_empty());

    // Explicitly continuing executes the single staged continuation exactly once; the repeated
    // answer above did not enqueue a second input, so exactly one model request follows.
    resumed
        .resume_inputs(pl_core::thread::input::InputDriverOptions {
            max_model_steps: pl_core::thread::ModelStepLimit::Limited(
                3.try_into().expect("nonzero limit"),
            ),
        })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while resumed
            .snapshot()
            .inputs
            .iter()
            .any(|record| record.input.id == "interaction:plan-1:continuation")
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("continuation turn finishes");
    assert_eq!(seen.lock().unwrap().len(), 1);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn an_explicitly_cancelled_interaction_is_not_resurrected_by_recovery() {
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("plan-cancelled".into(), DynModelSession::new(model)).unwrap();
    thread
        .request_interaction(question("plan-1", "turn-1"))
        .await
        .unwrap();
    let cancelled = thread
        .cancel_interaction(InteractionCancellation {
            id: "plan-1".into(),
            expected_revision: 1,
        })
        .await
        .unwrap();
    assert_eq!(cancelled.revision, 2);
    assert!(matches!(cancelled.state, InteractionState::Cancelled));
    // The settled record is committed history, so the resident pending map no longer shows it.
    assert!(!thread.snapshot().interactions.contains_key("plan-1"));

    thread.close().await.unwrap();
    let checkpoint = serde_json::to_vec(
        &thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap(),
    )
    .unwrap();
    let restored = serde_json::from_slice(&checkpoint).unwrap();
    let resumed =
        ThreadHandle::resume_without_model("plan-cancelled".into(), Some(restored)).unwrap();
    // Recovery must not rebuild the cancelled interaction from history or accept new answers.
    assert!(!resumed.snapshot().interactions.contains_key("plan-1"));
    assert!(matches!(
        resumed
            .resolve_interaction(InteractionResolution {
                continuation: None,
                id: "plan-1".into(),
                expected_revision: 1,
                response: InteractionResponse {
                    payload: OpaquePayload::text("approved"),
                    context: Vec::new(),
                },
                mutations: Vec::new(),
            })
            .await,
        Err(ThreadError::InvalidIdentity)
    ));
    assert!(matches!(
        resumed
            .cancel_interaction(InteractionCancellation {
                id: "plan-1".into(),
                expected_revision: 2,
            })
            .await,
        Err(ThreadError::InvalidIdentity)
    ));

    // The owner is open again and can ask a genuinely new question.
    let fresh = resumed
        .request_interaction(question("plan-2", "turn-2"))
        .await
        .unwrap();
    assert_eq!(fresh.revision, 1);
    assert!(matches!(fresh.state, InteractionState::Pending));
    resumed.close().await.unwrap();
}

/// Stops inside a pending execution-permission wait exactly like a host approval prompt.
#[derive(Debug)]
struct AwaitingPermission;

impl pl_core::tool::opaque::Tool for AwaitingPermission {
    async fn execute(
        &self,
        _: OpaquePayload,
        context: pl_core::tool::opaque::CallContext,
    ) -> Result<pl_core::tool::ToolOutput, pl_core::tool::opaque::ToolError> {
        let tasks = context.tasks.clone().expect("task access is granted");
        let decision = tasks
            .request_execution_permission(
                OpaquePayload::text("allow this command?"),
                context.cancellation.clone(),
            )
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)?;
        Ok(pl_core::tool::ToolOutput::new(
            OpaquePayload::text(format!("{decision:?}")),
            vec![text("decision recorded")],
        ))
    }
}

#[tokio::test]
async fn close_stops_runtime_resources_while_preserving_the_pending_interaction() {
    let (model, _) = ScriptedModel::new(&["await"]);
    let thread = ThreadHandle::start("plan-resources".into(), DynModelSession::new(model)).unwrap();
    thread
        .register_tools(vec![
            pl_core::tool::opaque::Registration::new(
                "await".into(),
                OpaquePayload::text("await approval"),
                AwaitingPermission,
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    thread
        .step(StepInput {
            turn_id: "approval-turn".into(),
            attempt_id: "approval-attempt".into(),
            content: vec![text("run the awaiting tool")],
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    let ToolDispatch::Running(task) = thread
        .execute_tool("approval-turn-await".into(), CancellationToken::new())
        .await
        .unwrap()
    else {
        panic!("the awaiting tool must run as a task")
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !thread
            .snapshot()
            .permissions
            .values()
            .any(|record| record.state == pl_core::thread::permissions::PermissionState::Pending)
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("execution permission becomes pending");
    thread
        .request_interaction(question("plan-1", "turn-1"))
        .await
        .unwrap();

    // A crash-safe checkpoint taken while the permission is still pending.
    let running_checkpoint = serde_json::to_vec(
        &thread
            .checkpoint(thread.snapshot().commit_sequence)
            .unwrap(),
    )
    .unwrap();

    thread.close().await.unwrap();

    // Close stopped the background task and settled its permission without granting it, while the
    // submitted interaction survived as a pending logical fact.
    let settled = thread.snapshot();
    assert!(
        !settled.permissions.values().any(|record| {
            record.state == pl_core::thread::permissions::PermissionState::Pending
        })
    );
    assert!(
        !settled
            .tasks
            .values()
            .any(|record| { record.status == pl_core::thread::task::TaskStatus::Running })
    );
    let preserved = settled
        .interactions
        .get("plan-1")
        .cloned()
        .expect("close preserves the pending interaction");
    assert_eq!(preserved.state, InteractionState::Pending);
    assert!(
        thread.effects().await.unwrap().iter().any(|effect| {
            effect.permissions.iter().any(|record| {
                record.call_id == "approval-turn-await"
                    && record.state == pl_core::thread::permissions::PermissionState::Cancelled
            })
        }),
        "close settles the pending permission as cancelled"
    );
    drop(task);

    // Recovering the mid-flight checkpoint keeps the same boundary: the permission does not
    // survive as authorization, the task is interrupted, and the interaction stays pending.
    let restored = serde_json::from_slice(&running_checkpoint).unwrap();
    let (model, seen) = ScriptedModel::new(&[]);
    let resumed = ThreadHandle::resume(
        "plan-resources".into(),
        DynModelSession::new(model),
        Some(restored),
    )
    .unwrap();
    let recovered = resumed.snapshot();
    assert!(
        !recovered.permissions.values().any(|record| {
            record.state == pl_core::thread::permissions::PermissionState::Pending
        })
    );
    assert!(
        !recovered
            .tasks
            .values()
            .any(|record| { record.status == pl_core::thread::task::TaskStatus::Running })
    );
    let preserved = recovered
        .interactions
        .get("plan-1")
        .cloned()
        .expect("recovery preserves the pending interaction");
    assert_eq!(preserved.state, InteractionState::Pending);
    assert!(seen.lock().unwrap().is_empty());
    resumed.close().await.unwrap();
}
