//! Product-owner integration: real model sessions, resource registration and cold reconstruction.
#[path = "support/actor.rs"]
mod actor;
#[path = "support/engine.rs"]
mod fixture;
use pl_core::{
    context::{ContextContent, ContextRecord, ContextSource},
    thread::{StepInput, ThreadLifecycle},
};
use pl_studio_runtime::thread_assembler::StudioThreadAssembler;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn restored_studio_owner_keeps_exact_model_history_and_opens_an_independent_session() {
    let (url, server) = fixture::serve_checked_sse_sequence(
        vec![answer("first"), answer("second")],
        |index, body| {
            index == 0
                || (body.to_string().contains("original instruction")
                    && body.to_string().contains("first")
                    && body.to_string().contains("followup"))
        },
    )
    .await;
    let route = fixture::route(
        "fixture",
        pl_model::provider::ProviderEndpoint::deepseek(Some(url)),
        pl_model::model::ModelInfo::compatible("host-test"),
        None,
    );
    let root = tempfile::tempdir().unwrap();
    let owner = StudioThreadAssembler::default();
    let mut spec = actor::specification("thread", route.clone(), root.path());
    spec.initial_context = vec![ContextRecord {
        id: "instruction".into(),
        turn_id: None,
        source: ContextSource::Instruction,
        content: vec![ContextContent::Text {
            text: "original instruction".into(),
        }],
        tool_calls: vec![],
    }];
    let first = owner.assemble(spec).await.unwrap();
    first.step(step("first", "hello")).await.unwrap();
    let history = first.journal().await.unwrap();
    let original = first.snapshot().context;
    owner.close("thread").await.unwrap();
    assert!(owner.thread("thread").is_none());
    let mut spec = actor::specification("thread", route, root.path());
    spec.history = history;
    let restored = owner.assemble(spec).await.unwrap();
    assert_eq!(restored.snapshot().context, original);
    restored.step(step("second", "followup")).await.unwrap();
    let replay = pl_core::thread::journal::replay(&restored.journal().await.unwrap()).unwrap();
    assert_eq!(replay.context, restored.snapshot().context);
    let close_failures = owner.close_all().await;
    assert!(
        close_failures.is_empty(),
        "unexpected close failures: {close_failures:?}"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn closing_a_tree_releases_descendants_and_leaves_other_roots_available() {
    let root = tempfile::tempdir().unwrap();
    let owner = StudioThreadAssembler::default();
    let route = fixture::route(
        "fixture",
        pl_model::provider::ProviderEndpoint::deepseek(None),
        pl_model::model::ModelInfo::compatible("host-test"),
        None,
    );
    let parent = owner
        .assemble(actor::specification("root", route.clone(), root.path()))
        .await
        .unwrap();
    let mut child = actor::specification("child", route.clone(), root.path());
    child.parent_id = Some("root".into());
    let child = owner.assemble(child).await.unwrap();
    let sibling = owner
        .assemble(actor::specification("other", route, root.path()))
        .await
        .unwrap();
    owner.close_tree("root").await.unwrap();
    assert_eq!(parent.snapshot().lifecycle, ThreadLifecycle::Closed);
    assert_eq!(child.snapshot().lifecycle, ThreadLifecycle::Closed);
    assert_eq!(sibling.snapshot().lifecycle, ThreadLifecycle::Open);
    assert!(owner.thread("root").is_none());
    assert!(owner.thread("child").is_none());
    assert!(owner.thread("other").is_some());
    assert!(owner.close_all().await.is_empty());
}

fn step(id: &str, text: &str) -> StepInput {
    StepInput {
        turn_id: id.into(),
        attempt_id: format!("{id}-call"),
        content: vec![ContextContent::Text { text: text.into() }],
        cancellation: Default::default(),
    }
}
fn answer(text: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}]})
    )
}
