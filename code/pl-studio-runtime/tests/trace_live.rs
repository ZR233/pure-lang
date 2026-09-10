use pl_core::{
    context::ContextContent,
    model::Model,
    thread::{StepInput, ThreadHandle},
};
use pl_model::runtime::{ModelRuntime, ThreadModel, model_response_receipt};
use pretty_assertions::{assert_eq, assert_ne};

#[path = "support/engine.rs"]
mod engine_support;

#[tokio::test]
async fn cross_turn_receipts_and_history_are_isolated_live() {
    let route = engine_support::deepseek_route(
        std::env::var("DEEPSEEK_API_KEY")
            .expect("explicit live acceptance requires DEEPSEEK_API_KEY"),
    );
    let model = ThreadModel::new(
        ModelRuntime::from_route(&route).unwrap(),
        route.reasoning_config(),
    );
    let thread =
        ThreadHandle::start("trace-live".into(), model.open_session().await.unwrap()).unwrap();
    let mut outputs = Vec::new();
    for (turn, prompt) in [
        ("first", "请只输出：你好，第一轮。"),
        ("second", "请只输出：你好，第二轮。"),
    ] {
        let response = thread
            .step(StepInput {
                turn_id: turn.into(),
                attempt_id: format!("{turn}-call"),
                content: vec![ContextContent::Text {
                    text: prompt.into(),
                }],
                cancellation: Default::default(),
            })
            .await
            .unwrap();
        let receipt = model_response_receipt(&response)
            .unwrap()
            .expect("model provenance is retained");
        assert!(
            receipt
                .response
                .content
                .as_ref()
                .is_some_and(|text| !text.is_empty())
        );
        assert_eq!(receipt.binding.requested_model, route.model.slug);
        outputs.push(response);
    }
    assert_ne!(outputs[0].attempt_id, outputs[1].attempt_id);
    let snapshot = thread.snapshot();
    assert_eq!(snapshot.attempts.len(), 2);
    assert_eq!(snapshot.attempts[0].turn_id, "first");
    assert_eq!(snapshot.attempts[1].turn_id, "second");
    assert!(
        snapshot.attempts[1]
            .input
            .records
            .iter()
            .any(|record| record.turn_id.as_deref() == Some("first"))
    );
    let replay = pl_core::thread::journal::replay(&thread.journal().await.unwrap()).unwrap();
    assert_eq!(replay.context, snapshot.context);
    assert_eq!(replay.attempts.len(), 2);
    thread.close().await.unwrap();
}
