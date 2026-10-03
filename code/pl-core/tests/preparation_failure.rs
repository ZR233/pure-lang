mod support;

use pl_core::{
    context::OpaquePayload,
    model::{DynModelSession, ModelError, ModelFailureKind},
    thread::{
        ThreadHandle, TurnState,
        context_preparation::{
            ContextPreparation, ContextPreparationHook, ContextPreparationRequest, ContextPreparer,
        },
    },
};
use std::sync::atomic::{AtomicBool, Ordering};
use support::{ScriptedModel, turn};

#[derive(Debug)]
struct FailOnce(AtomicBool);

impl ContextPreparationHook for FailOnce {
    async fn before_step(&self, _: ContextPreparationRequest) -> ContextPreparation {
        if self.0.swap(false, Ordering::Relaxed) {
            ContextPreparation::Failed {
                error: ModelError {
                    kind: ModelFailureKind::Unavailable,
                    details: Some(Box::new(
                        OpaquePayload::new("fixture.failure", 1, "retryable provider timeout")
                            .unwrap(),
                    )),
                    usage: Box::new(pl_core::model::ModelUsage {
                        input_tokens: Some(37),
                        ..Default::default()
                    }),
                    source: None,
                },
                mutations: vec![],
            }
        } else {
            ContextPreparation::Unchanged
        }
    }
}

#[tokio::test]
async fn preadmission_failure_retains_model_facts_and_continues_without_replaying_history() {
    let (model, observed) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("preparation".into(), DynModelSession::new(model)).unwrap();
    thread
        .set_context_preparation(Some(ContextPreparer::new(FailOnce(AtomicBool::new(true)))))
        .await
        .unwrap();
    let original_context = thread.snapshot().context;
    assert!(thread.run_turn(turn("failed")).await.is_err());
    let snapshot = thread.snapshot();
    assert!(snapshot.attempts.is_empty());
    assert!(observed.lock().unwrap().is_empty());
    assert_eq!(snapshot.context, original_context);
    let effects = thread.effects().await.unwrap();
    let failed_turn = effects
        .iter()
        .rev()
        .filter_map(|effect| effect.turn.as_ref())
        .find(|turn| matches!(turn.state, TurnState::Failed { .. }))
        .unwrap();
    let TurnState::Failed {
        model_failure: Some(facts),
        ..
    } = &failed_turn.state
    else {
        panic!("preparation discarded model failure facts")
    };
    assert_eq!(facts.kind, ModelFailureKind::Unavailable);
    assert_eq!(facts.usage.input_tokens, Some(37));
    assert_eq!(
        facts.details.as_ref().unwrap().content(),
        "retryable provider timeout"
    );
    let encoded = serde_json::to_value(failed_turn).unwrap();
    let restored: pl_core::thread::TurnRecord = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(&restored, failed_turn);
    // Older v2 Turn records retain their description without guessing producer facts.
    let mut old = encoded;
    old["state"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("modelFailure");
    let old: pl_core::thread::TurnRecord = serde_json::from_value(old).unwrap();
    assert!(matches!(
        &old.state,
        TurnState::Failed {
            model_failure: None,
            ..
        }
    ));
    thread.run_turn(turn("continue")).await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 1);
    let checkpoint = thread
        .checkpoint(thread.snapshot().commit_sequence)
        .unwrap();
    let context = thread.snapshot().context;
    thread.close().await.unwrap();
    let (model, observed) = ScriptedModel::new(&[]);
    let resumed = ThreadHandle::resume(
        "preparation".into(),
        DynModelSession::new(model),
        Some(checkpoint),
    )
    .unwrap();
    assert_eq!(resumed.snapshot().context, context);
    assert!(observed.lock().unwrap().is_empty());
    resumed.run_turn(turn("after-reopen")).await.unwrap();
    assert_eq!(observed.lock().unwrap().len(), 1);
    resumed.close().await.unwrap();
}
