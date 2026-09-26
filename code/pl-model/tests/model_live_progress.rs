//! Public-API coverage of the live observation produced while a provider stream is still open.
//!
//! The cases drive real fixture SSE through the Thread so they exercise the decoder, the observation
//! producer and the typed port together, and they read the failure receipt through its public reader.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pl_core::chat::PresentationPart;
use pl_core::context::{ContextContent, OpaquePayload};
use pl_core::model::{
    AggregateChannel, ModelError, ModelFactory, ModelFailureKind, ModelProgress,
    ObservedPartIdentity,
};
use pl_core::thread::{ModelStepLimit, ThreadHandle, TurnInput};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, Tool, ToolError},
};
use pl_model::completion::{
    CompletionPresentationItem, CompletionPresentationItemKind, CompletionPresentationPart,
    CompletionPresentationPartKind, CompletionRequest, Message, MessageContent, MessageRole,
    ToolSpec,
};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::ProviderEndpoint;
use pl_model::runtime::{
    ModelInvocationContext, ModelRuntime, ModelSession, thread_tool_declaration,
};
use pl_protocol::PureError;
use pl_protocol::trace::TraceTextChannel;
use pl_provider_fixture::{FixtureServer, Protocol, Reply, Step};
use serde_json::{Value, json};

fn model(slug: &str, profile: ModelTransportProfile) -> ModelInfo {
    let mut model = ModelInfo::compatible(slug);
    model.binding.set_transport(profile);
    model
}

fn user_turn(prompt: &str) -> TurnInput {
    TurnInput {
        turn_id: "live-turn".into(),
        attempt_prefix: "live-attempt".into(),
        content: vec![ContextContent::Text {
            text: Arc::from(prompt),
        }],
        max_model_steps: ModelStepLimit::Limited(1.try_into().expect("one step is a valid limit")),
        cancellation: Default::default(),
    }
}

fn user_message(text: &str) -> Message {
    Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(text),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }
}

/// Runs one hanging fixture stream through a Thread and returns the first live snapshot the predicate
/// accepts, so the observation is read while the provider stream is still open.
async fn observed_progress(
    prompt: &str,
    protocol: Protocol,
    profile: ModelTransportProfile,
    events: Vec<Value>,
    accept: impl Fn(&ModelProgress) -> bool,
) -> ModelProgress {
    let fixture = FixtureServer::start(vec![Step::prompt(
        protocol,
        prompt,
        0,
        Reply::HangingSse(events),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", profile),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("live-thread".into(), session).expect("thread starts");
    let progress = {
        let mut subscription = thread.subscribe();
        let turn = thread.run_turn(user_turn(prompt));
        tokio::pin!(turn);
        let observed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = tokio::select! {
                    _ = &mut turn => panic!("the turn ended before the live observation arrived"),
                    snapshot = subscription.next() => snapshot.expect("thread stays open"),
                };
                if let Some(active) = snapshot.model_progress.as_ref()
                    && accept(&active.progress)
                {
                    break active.progress.clone();
                }
            }
        })
        .await
        .expect("live observation arrives before the stream ends");
        // The provider stream never completes, so it ends without a terminal event once the fixture
        // stops; bound the wait instead of depending on retry timing.
        let _ = fixture.shutdown().await;
        let _ = tokio::time::timeout(Duration::from_secs(10), turn).await;
        observed
    };
    // The pinned turn future was dropped with its scope, so its borrow of the Thread is gone and the
    // handle can be released here.
    drop(thread);
    progress
}

/// One reasoning item that closes, then two text blocks in the same channel that keep streaming.
fn reasoning_then_same_channel_text_blocks() -> Vec<Value> {
    vec![
        json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"id":"reasoning-1","type":"reasoning","summary":[]}}),
        json!({"type":"response.reasoning_summary_text.delta","item_id":"reasoning-1",
            "output_index":0,"summary_index":0,"delta":"ponder"}),
        json!({"type":"response.output_item.done","output_index":0,
            "item":{"id":"reasoning-1","type":"reasoning",
                "summary":[{"type":"summary_text","text":"ponder"}]}}),
        json!({"type":"response.output_item.added","output_index":1,
            "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-1",
            "output_index":1,"content_index":0,"delta":"answer"}),
        json!({"type":"response.output_item.added","output_index":2,
            "item":{"id":"message-2","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-2",
            "output_index":2,"content_index":0,"delta":"again"}),
    ]
}

#[tokio::test]
async fn a_closed_item_does_not_freeze_later_blocks_of_the_same_channel() {
    let progress = observed_progress(
        "live prompt",
        Protocol::ResponsesHttp,
        ModelTransportProfile::responses_http(),
        reasoning_then_same_channel_text_blocks(),
        |progress| progress.parts().iter().any(|part| part.text() == "again"),
    )
    .await;

    assert_eq!(
        progress
            .observed_part("reasoning-1", PresentationPart::SummaryText(0))
            .expect("the reasoning item is observed")
            .text(),
        "ponder"
    );
    assert_eq!(
        progress
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the text block that opened after the reasoning item closed")
            .text(),
        "answer"
    );
    assert_eq!(
        progress
            .observed_part("message-2", PresentationPart::OutputText(0))
            .expect("a second block of the same channel stays its own identity")
            .text(),
        "again"
    );
    // Real provider item boundaries replaced the aggregate, so no channel body is kept alongside it.
    assert!(progress.channel(AggregateChannel::Text).is_none());
    assert!(progress.channel(AggregateChannel::Reasoning).is_none());
}

/// One stable text part paced so its growth is observable, plus a later part of its own identity.
fn paced_text_growth_then_a_new_part() -> Vec<Value> {
    vec![
        json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-1",
            "output_index":0,"content_index":0,"delta":"a"}),
        json!({"type":"response.output_text.delta","item_id":"message-1",
            "output_index":0,"content_index":0,"delta":"b"}),
        json!({"type":"response.output_item.added","output_index":1,
            "item":{"id":"message-2","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-2",
            "output_index":1,"content_index":0,"delta":"c"}),
    ]
}

/// One part that grows, then enough further parts that the first part stops being the growing tail
/// before it grows again.
fn paced_text_growth_across_a_chunk_boundary() -> Vec<Value> {
    let mut events = vec![
        json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-1",
            "output_index":0,"content_index":0,"delta":"a"}),
    ];
    // More parts than one shared chunk, so `message-1` leaves the growing tail.
    for index in 0..66_u32 {
        let item = format!("filler-{index}");
        events.push(json!({"type":"response.output_item.added","output_index": index + 1,
            "item":{"id":item.clone(),"type":"message","role":"assistant","phase":"final_answer","content":[]}}));
        events.push(json!({"type":"response.output_text.delta","item_id":item,
            "output_index": index + 1,"content_index":0,"delta":"x"}));
    }
    events.push(
        json!({"type":"response.output_text.delta","item_id":"message-1",
        "output_index":0,"content_index":0,"delta":"b"}),
    );
    events.push(json!({"type":"response.output_item.added","output_index":67,
        "item":{"id":"message-2","type":"message","role":"assistant","phase":"final_answer","content":[]}}));
    events.push(
        json!({"type":"response.output_text.delta","item_id":"message-2",
        "output_index":67,"content_index":0,"delta":"c"}),
    );
    events
}

#[tokio::test]
async fn a_held_snapshot_stays_immutable_while_the_same_part_keeps_growing() {
    // A consumer that cloned one frame must keep exactly that frame even though the producer keeps
    // appending to the same identity and then opens a new one. The producer edits the shared part
    // list in place only while the watch owns its only reference; holding this clone is what forces
    // the copy-on-write that keeps the delivered frame intact.
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "live prompt",
        0,
        Reply::PacedEvents {
            initial_delay_ms: 0,
            step_millis: 300,
            events: paced_text_growth_then_a_new_part(),
        },
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("live-thread".into(), session).expect("thread starts");
    let mut subscription = thread.subscribe();
    let turn = thread.run_turn(user_turn("live prompt"));
    tokio::pin!(turn);
    let (held, grown) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut held: Option<ModelProgress> = None;
        loop {
            let snapshot = tokio::select! {
                _ = &mut turn => panic!("the turn ended before both observations arrived"),
                snapshot = subscription.next() => snapshot.expect("thread stays open"),
            };
            let Some(active) = snapshot.model_progress.as_ref() else {
                continue;
            };
            let progress = &active.progress;
            let part = |item: &str| {
                progress
                    .observed_part(item, PresentationPart::OutputText(0))
                    .map(|part| part.text())
            };
            if held.is_none() && part("message-1").as_deref() == Some("a") {
                // Hold this mid-stream frame; the producer keeps appending after it.
                held = Some(progress.clone());
            }
            if let Some(frame) = held.as_ref()
                && part("message-1").as_deref() == Some("ab")
                && part("message-2").as_deref() == Some("c")
            {
                break (frame.clone(), progress.clone());
            }
        }
    })
    .await
    .expect("both observations arrive before the stream ends");
    let _ = fixture.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(10), turn).await;

    // The frame the consumer kept is byte-for-byte the one it saw: the same identity kept streaming
    // after it, but appending to a shared list never mutates a snapshot a consumer already holds.
    assert_eq!(
        held.observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the held frame kept the part it delivered")
            .text(),
        "a"
    );
    assert!(
        held.observed_part("message-2", PresentationPart::OutputText(0))
            .is_none(),
        "a part opened after the held frame is not grafted onto it"
    );
    // The newest frame carries the appended bytes and the new part under their stable identities.
    assert_eq!(
        grown
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the same identity keeps growing")
            .text(),
        "ab"
    );
    assert_eq!(
        grown
            .observed_part("message-2", PresentationPart::OutputText(0))
            .expect("the part opened later has its own identity")
            .text(),
        "c"
    );
}

#[tokio::test]
async fn a_held_snapshot_survives_the_growth_of_a_chunk_it_shared() {
    // The part list is shared in chunks, so the frame a consumer holds keeps exactly what it saw
    // even after the chunk it shared has filled, become an immutable chunk, and been edited again by
    // a later delta on the same identity. The producer only ever reproduces the chunk it edits.
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "live prompt",
        0,
        Reply::PacedEvents {
            initial_delay_ms: 0,
            step_millis: 30,
            events: paced_text_growth_across_a_chunk_boundary(),
        },
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("live-thread".into(), session).expect("thread starts");
    let mut subscription = thread.subscribe();
    let turn = thread.run_turn(user_turn("live prompt"));
    tokio::pin!(turn);
    let (held, grown) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut held: Option<ModelProgress> = None;
        loop {
            let snapshot = tokio::select! {
                _ = &mut turn => panic!("the turn ended before both observations arrived"),
                snapshot = subscription.next() => snapshot.expect("thread stays open"),
            };
            let Some(active) = snapshot.model_progress.as_ref() else {
                continue;
            };
            let progress = &active.progress;
            let part = |item: &str| {
                progress
                    .observed_part(item, PresentationPart::OutputText(0))
                    .map(|part| part.text())
            };
            if held.is_none() && part("message-1").as_deref() == Some("a") {
                // Hold this frame while it still shares the chunk the producer keeps editing.
                held = Some(progress.clone());
            }
            if let Some(frame) = held.as_ref()
                && part("message-1").as_deref() == Some("ab")
                && part("message-2").as_deref() == Some("c")
            {
                break (frame.clone(), progress.clone());
            }
        }
    })
    .await
    .expect("both observations arrive before the stream ends");
    let _ = fixture.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(10), turn).await;

    // The frame the consumer kept is exactly the one it saw, even though a later delta edited the
    // chunk it shared with the newest snapshot.
    assert_eq!(
        held.observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the held frame kept the part it delivered")
            .text(),
        "a"
    );
    assert!(
        held.observed_part("message-2", PresentationPart::OutputText(0))
            .is_none(),
        "a part opened after the held frame is not grafted onto it"
    );
    assert!(
        held.len() < grown.len(),
        "the frame was held before the list finished growing ({} vs {})",
        held.len(),
        grown.len()
    );
    // The newest frame carries the appended bytes, every part in between, and the new part.
    assert!(
        grown.len() >= 68,
        "one observation per provider item is still observed ({})",
        grown.len()
    );
    assert_eq!(
        grown
            .observed_part("filler-65", PresentationPart::OutputText(0))
            .expect("the part opened after the held frame is still observed")
            .text(),
        "x"
    );
    assert_eq!(
        grown
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the same identity keeps growing")
            .text(),
        "ab"
    );
    assert_eq!(
        grown
            .observed_part("message-2", PresentationPart::OutputText(0))
            .expect("the part opened later has its own identity")
            .text(),
        "c"
    );
}

#[tokio::test]
async fn multiple_summary_indices_of_one_item_stay_distinct_parts() {
    // Two summaries of one reasoning item arrive before it closes, so the `summary_index` the
    // provider reported must already be the part identity: a regression that folds every summary
    // into `SummaryText(0)` would hide the second part until the terminal item rewrote it.
    let events = vec![
        json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"id":"reasoning-1","type":"reasoning","summary":[]}}),
        json!({"type":"response.reasoning_summary_text.delta","item_id":"reasoning-1",
            "output_index":0,"summary_index":0,"delta":"first"}),
        json!({"type":"response.reasoning_summary_text.delta","item_id":"reasoning-1",
            "output_index":0,"summary_index":1,"delta":"second"}),
        json!({"type":"response.output_item.added","output_index":1,
            "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-1",
            "output_index":1,"content_index":0,"delta":"answer"}),
    ];
    let progress = observed_progress(
        "multi summary",
        Protocol::ResponsesHttp,
        ModelTransportProfile::responses_http(),
        events,
        |progress| {
            progress
                .observed_part("reasoning-1", PresentationPart::SummaryText(1))
                .is_some()
        },
    )
    .await;

    assert_eq!(
        progress
            .observed_part("reasoning-1", PresentationPart::SummaryText(0))
            .expect("the first summary keeps its own index")
            .text(),
        "first"
    );
    assert_eq!(
        progress
            .observed_part("reasoning-1", PresentationPart::SummaryText(1))
            .expect("the second summary is its own part, not folded into index zero")
            .text(),
        "second"
    );
}

#[tokio::test]
async fn reasoning_delta_without_item_id_resolves_the_announced_output_index() {
    let events = vec![
        json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
        json!({"type":"response.output_item.added","output_index":1,
            "item":{"id":"reasoning-1","type":"reasoning","summary":[]}}),
        // No `item_id`: the identity is the item the `output_index` was announced for.
        json!({"type":"response.reasoning_text.delta","output_index":1,
            "content_index":0,"delta":"ponder"}),
        json!({"type":"response.output_item.added","output_index":2,
            "item":{"id":"message-2","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"message-2",
            "output_index":2,"content_index":0,"delta":"answer"}),
    ];
    let progress = observed_progress(
        "raw reasoning",
        Protocol::ResponsesHttp,
        ModelTransportProfile::responses_http(),
        events,
        |progress| {
            progress
                .observed_part("message-2", PresentationPart::OutputText(0))
                .is_some()
        },
    )
    .await;

    assert_eq!(
        progress
            .observed_part("reasoning-1", PresentationPart::ReasoningText(0))
            .expect("the announced item owns the reasoning delta that omitted its id")
            .text(),
        "ponder"
    );
    assert_eq!(
        progress
            .observed_part("message-2", PresentationPart::OutputText(0))
            .expect("the answer keeps its own identity")
            .text(),
        "answer"
    );
    // Raw reasoning is a provider part of its own; it is neither a summary nor an aggregate.
    assert!(
        progress
            .observed_part("reasoning-1", PresentationPart::SummaryText(0))
            .is_none()
    );
    assert!(progress.channel(AggregateChannel::Reasoning).is_none());
}

#[tokio::test]
async fn chat_reasoning_content_streams_before_the_answer_as_its_own_channel() {
    let events = vec![
        json!({"choices":[{"delta":{"reasoning_content":"ponder"}}]}),
        json!({"choices":[{"delta":{"content":"answer"}}]}),
    ];
    let progress = observed_progress(
        "chat thinking",
        Protocol::Chat,
        ModelTransportProfile::chat_completions_http(),
        events,
        |progress| progress.channel(AggregateChannel::Text).is_some(),
    )
    .await;

    assert_eq!(
        progress
            .channel(AggregateChannel::Reasoning)
            .expect("chat reasoning content is observed as the raw reasoning channel")
            .text(),
        "ponder"
    );
    assert_eq!(
        progress
            .channel(AggregateChannel::Text)
            .expect("the answer is observed on its own channel")
            .text(),
        "answer"
    );
    // Chat reports no provider item identity, so no fabricated provider item stands in for it.
    assert!(
        progress
            .parts()
            .iter()
            .all(|part| matches!(part.identity(), ObservedPartIdentity::Aggregate { .. }))
    );
}

#[tokio::test]
async fn reasoning_delta_without_any_identity_fails_instead_of_inventing_an_item() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "unidentified reasoning",
        0,
        Reply::Sse(vec![
            json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
            // Neither an item id nor an announced output index: the delta cannot be attributed.
            json!({"type":"response.reasoning_text.delta","content_index":0,"delta":"ponder"}),
        ]),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let failure = runtime
        .complete(
            CompletionRequest::builder()
                .messages(vec![user_message("unidentified reasoning")])
                .build(),
            ModelInvocationContext::new(ModelSession::default()),
        )
        .await
        .expect_err("an unassociable reasoning delta fails instead of fabricating an item");
    assert!(
        failure.to_string().contains("protocol error"),
        "unexpected failure: {failure}"
    );
    let _ = fixture.shutdown().await;
}

fn legacy_binding() -> Value {
    json!({
        "providerInstanceId": "fixture",
        "requestedModel": "fixture-model",
        "adapter": "openAiCompatible",
        "protocol": "responses",
        "isolation": "fixture",
        "purpose": "turn",
    })
}

fn legacy_receipt(partial_progress: Value) -> ModelError {
    let payload = json!({
        "message": "provider failed mid stream",
        "binding": legacy_binding(),
        "accounting": serde_json::to_value(pl_protocol::InferenceAccounting::default())
            .expect("default accounting encodes"),
        "partialProgress": partial_progress,
    });
    ModelError {
        details: Some(Box::new(
            OpaquePayload::new(
                "pl.model.failure",
                1,
                serde_json::to_string(&payload).expect("legacy receipt encodes"),
            )
            .expect("static format and version are valid"),
        )),
        kind: ModelFailureKind::Unavailable,
        usage: Default::default(),
        source: None,
    }
}

#[test]
fn legacy_failure_receipt_keeps_the_partial_text_it_had_already_received() {
    let item = CompletionPresentationItem {
        provider_item_id: "message-1".into(),
        output_index: Some(0),
        kind: CompletionPresentationItemKind::Text(TraceTextChannel::Final),
        parts: vec![CompletionPresentationPart {
            content_index: 0,
            provider_part_id: None,
            kind: CompletionPresentationPartKind::OutputText,
            text: "received answer".into(),
        }],
    };
    let error = legacy_receipt(json!({
        "content": [],
        "reasoning": null,
        "presentation": [{
            "format": "pl.model.presentation-item",
            "version": 1,
            "content": serde_json::to_string(&item).expect("item encodes"),
        }],
    }));

    let receipt = pl_model::runtime::model_failure_receipt(&error)
        .expect("a version 1 receipt is migrated, not rejected")
        .expect("the receipt is present");
    assert_eq!(receipt.message, "provider failed mid stream");
    let progress = receipt
        .partial_progress
        .expect("the migrated receipt keeps its partial observation");
    assert_eq!(
        progress
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the encoder's item identity is preserved")
            .text(),
        "received answer"
    );
}

#[test]
fn legacy_failure_receipt_recovers_the_channel_aggregate_body() {
    let error = legacy_receipt(json!({
        "content": [{"kind": "text", "text": "half an answer"}],
        "reasoning": {"format": "text/plain", "version": 1, "content": "half a thought"},
        "presentation": [],
    }));

    let receipt = pl_model::runtime::model_failure_receipt(&error)
        .expect("a version 1 receipt is migrated, not rejected")
        .expect("the receipt is present");
    let progress = receipt
        .partial_progress
        .expect("the migrated receipt keeps its partial observation");
    assert_eq!(
        progress
            .channel(AggregateChannel::Text)
            .expect("the received answer text is recovered")
            .text(),
        "half an answer"
    );
    assert_eq!(
        progress
            .channel(AggregateChannel::Reasoning)
            .expect("the received reasoning text is recovered")
            .text(),
        "half a thought"
    );
}

#[test]
fn unknown_failure_receipt_versions_are_still_rejected() {
    let mut error = legacy_receipt(json!({"content": [], "reasoning": null, "presentation": []}));
    error.details = Some(Box::new(
        OpaquePayload::new("pl.model.failure", 99, "{}").expect("valid payload"),
    ));
    let rejected = pl_model::runtime::model_failure_receipt(&error);
    assert!(matches!(
        rejected,
        Err(ModelError {
            kind: ModelFailureKind::UnsupportedContent,
            ..
        })
    ));
}

/// Minimal reliable backend that funds only `limit` bytes of one in-flight call's live output.
///
/// Admission and durability always succeed: the only thing under test is the reservation the real
/// adapter charges against, so the stream has to stop where the quota does and nowhere else.
#[derive(Debug)]
struct OutputQuotaStore {
    limit: u64,
}

impl pl_core::thread::cold::ColdStore for OutputQuotaStore {
    fn reserve_operation_output(
        &self,
        _thread_id: &str,
        _operation_id: &str,
        max_bytes: u64,
    ) -> Result<u64, pl_core::thread::cold::ColdStoreError> {
        Ok(self.limit.min(max_bytes))
    }

    fn admit(
        &self,
        _thread_id: &str,
        _write: pl_core::thread::cold::ThreadWrite,
    ) -> Result<(), pl_core::thread::cold::ColdStoreError> {
        Ok(())
    }

    async fn flush(
        &self,
        _thread_id: &str,
        _sequence: u64,
    ) -> Result<(), pl_core::thread::cold::ColdStoreError> {
        Ok(())
    }
}

/// A provider stream that outgrows the call's reliable output quota is cancelled with what it had.
///
/// The real adapter charges each observed increment through the Thread's own reservation *before* it
/// becomes resident, so this drives the actual decoder, observation producer and reservation end to
/// end instead of a scripted sender calling `charge_output` directly. The stream has to stop at the
/// first chunk the quota cannot hold, keep the bytes it already received readable through the
/// failure receipt, and latch the typed storage fault that holds further model/tool work.
#[tokio::test]
async fn fixture_stream_past_the_reliable_output_quota_cancels_with_a_partial_receipt() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "budget prompt",
        0,
        Reply::Sse(vec![
            json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,
                "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
            json!({"type":"response.output_text.delta","item_id":"message-1",
                "output_index":0,"content_index":0,"delta":"answer"}),
            json!({"type":"response.output_text.delta","item_id":"message-1",
                "output_index":0,"content_index":0,"delta":"again"}),
        ]),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("quota-thread".into(), session).expect("thread starts");
    thread
        .attach_storage(pl_core::thread::cold::ColdStoreHandle::new(
            OutputQuotaStore { limit: 8 },
        ))
        .await
        .expect("storage attaches");

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(user_turn("budget prompt")).await }
    });
    let snapshot = {
        let mut subscription = thread.subscribe();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = subscription.next().await.expect("thread stays open");
                if snapshot.persistence.resume_required {
                    return snapshot;
                }
            }
        })
        .await
        .expect("a stream past the reliable quota must latch the typed fault")
    };
    assert_eq!(
        snapshot.persistence.fault,
        Some(pl_core::thread::cold::StorageFaultKind::QueueFull),
        "the truncation must travel as a typed storage fault, not error text"
    );
    let _ = tokio::time::timeout(Duration::from_secs(10), runner)
        .await
        .expect("the cancelled call ends the Turn");
    let _ = fixture.shutdown().await;

    let snapshot = thread.snapshot();
    let error = snapshot
        .attempts
        .iter()
        .find_map(|attempt| match &attempt.outcome {
            pl_core::thread::AttemptOutcome::Failed(error) => Some(error.clone()),
            _ => None,
        })
        .expect("the truncated call failed instead of committing");
    let receipt = pl_model::runtime::model_failure_receipt(error.as_ref())
        .expect("the receipt decodes")
        .expect("the failure carries a receipt");
    assert_eq!(
        receipt
            .partial_progress
            .expect("the receipt keeps what was already received")
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the accepted text keeps its identity")
            .text(),
        "answer",
        "the bytes the quota accepted before the refusal stay readable"
    );
    let _ = tokio::time::timeout(Duration::from_secs(10), thread.close()).await;
}

/// Tool-call arguments over the reliable quota are refused before they are copied in.
///
/// The arguments of a function call are output this call may have to retain just like text, so the
/// same reservation bounds them: the stream stops at the first argument chunk the quota cannot hold,
/// the text already accepted stays readable, and no partial (invalid-JSON) call is ever handed to an
/// executor.
#[tokio::test]
async fn fixture_stream_past_the_reliable_output_quota_on_tool_arguments_is_refused() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "tool budget prompt",
        0,
        Reply::Sse(vec![
            json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,
                "item":{"id":"message-1","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
            json!({"type":"response.output_text.delta","item_id":"message-1",
                "output_index":0,"content_index":0,"delta":"answer"}),
            json!({"type":"response.output_item.added","output_index":1,
                "item":{"id":"item-7","type":"function_call","name":"lookup"}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"item-7",
                "call_id":"call-7","delta":"{\"term\":\"rust\"}"}),
        ]),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("tool-quota-thread".into(), session).expect("thread starts");
    // The text delta ("answer", six bytes) fits; the fifteen-byte argument payload does not.
    thread
        .attach_storage(pl_core::thread::cold::ColdStoreHandle::new(
            OutputQuotaStore { limit: 12 },
        ))
        .await
        .expect("storage attaches");

    let runner = tokio::spawn({
        let thread = thread.clone();
        async move { thread.run_turn(user_turn("tool budget prompt")).await }
    });
    let snapshot = {
        let mut subscription = thread.subscribe();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = subscription.next().await.expect("thread stays open");
                if snapshot.persistence.resume_required {
                    return snapshot;
                }
            }
        })
        .await
        .expect("tool arguments past the reliable quota must latch the typed fault")
    };
    assert_eq!(
        snapshot.persistence.fault,
        Some(pl_core::thread::cold::StorageFaultKind::QueueFull),
        "the refusal travels as the same typed storage fault text does"
    );
    let _ = tokio::time::timeout(Duration::from_secs(10), runner)
        .await
        .expect("the refused call ends the Turn");
    let _ = fixture.shutdown().await;

    let snapshot = thread.snapshot();
    let error = snapshot
        .attempts
        .iter()
        .find_map(|attempt| match &attempt.outcome {
            pl_core::thread::AttemptOutcome::Failed(error) => Some(error.clone()),
            _ => None,
        })
        .expect("the refused call failed instead of committing a tool call");
    let receipt = pl_model::runtime::model_failure_receipt(error.as_ref())
        .expect("the receipt decodes")
        .expect("the failure carries a receipt");
    assert_eq!(
        receipt
            .partial_progress
            .expect("the receipt keeps what was already received")
            .observed_part("message-1", PresentationPart::OutputText(0))
            .expect("the accepted text keeps its identity")
            .text(),
        "answer",
        "text accepted before the argument refusal stays readable"
    );
    assert!(
        receipt.presentation_items.is_empty(),
        "no incomplete tool call was materialized from the refused arguments"
    );
    let _ = tokio::time::timeout(Duration::from_secs(10), thread.close()).await;
}

/// A provider tool call whose identity is exposed in stages is charged to the quota once.
///
/// The start announces the item, the argument delta omits the call id, and the closing item finally
/// carries *only* a distinct call id. Keying each event by its own raw identity would charge one call
/// twice — the delta under the item id and the close under the late call id — and refuse a stream
/// that really fits. Resolving identity the way the canonical accumulator does keeps all three
/// events on one key, so the reliable quota sees the call exactly once.
#[derive(Debug)]
struct RecordingLookup {
    calls: Arc<Mutex<Vec<String>>>,
}

impl Tool for RecordingLookup {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.calls.lock().expect("tool log").push(context.call_id);
        Ok(ToolOutput::new(
            input.clone(),
            vec![ContextContent::Text {
                text: Arc::from("lookup result"),
            }],
        ))
    }
}

#[tokio::test]
async fn a_staged_tool_identity_is_charged_once_against_the_reliable_quota() {
    // Fifteen argument bytes: they fit the 25-byte quota once, but two independent charges of the
    // same call would overrun it and falsely refuse a well-formed tool call.
    let arguments = "{\"term\":\"rust\"}";
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "staged identity",
        0,
        Reply::Sse(vec![
            json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,
                "item":{"id":"item-7","type":"function_call","name":"lookup"}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"item-7",
                "delta":arguments}),
            json!({"type":"response.output_item.done","output_index":0,
                "item":{"call_id":"call-7","type":"function_call","name":"lookup",
                    "arguments":arguments}}),
            // The stream is only terminal once the provider completes the response: without it the
            // accumulator reports an incomplete transport, the runtime retries, and the second
            // request no longer matches this step's *single* scripted reply.
            json!({"type":"response.completed","response":{"id":"live-stream","model":"live-fixture",
                "usage":{"input_tokens":8,"output_tokens":4}}}),
        ]),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime, None))
        .open_session()
        .await
        .expect("model session opens");
    let thread = ThreadHandle::start("staged-identity".into(), session).expect("thread starts");
    thread
        .attach_storage(pl_core::thread::cold::ColdStoreHandle::new(
            OutputQuotaStore { limit: 25 },
        ))
        .await
        .expect("storage attaches");
    let calls = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![
            Registration::new(
                "lookup".into(),
                // A real tool declaration, encoded exactly as the runtime does, so the request is
                // projected as a normal function tool and the stream reaches the identity resolver.
                thread_tool_declaration(&ToolSpec::function(
                    "lookup",
                    "Look up a term",
                    json!({"type": "object", "properties": {"term": {"type": "string"}}}),
                ))
                .expect("a valid function declaration"),
                RecordingLookup {
                    calls: calls.clone(),
                },
            )
            .expect("valid tool identity"),
        ])
        .await
        .expect("tool registers");

    let completed = tokio::time::timeout(
        Duration::from_secs(10),
        thread.run_turn(user_turn("staged identity")),
    )
    .await
    .expect("the staged identity stream must not stall")
    .expect("the stream fits the quota once and completes");
    assert_eq!(completed.model_steps, 1, "one model step produced the call");
    let snapshot = thread.snapshot();
    assert_eq!(
        snapshot.persistence.fault, None,
        "one call exposed in stages must not be charged twice"
    );
    assert!(!snapshot.persistence.resume_required);
    assert_eq!(
        calls.lock().expect("tool log").as_slice(),
        ["call-7"],
        "the resolved call is materialized and dispatched exactly once"
    );
    let _ = fixture.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(10), thread.close()).await;
}

/// Driving the collector without a live progress sender still enforces its own output ceiling.
///
/// A host call outside a Thread carries no `ModelProgressSender`, so the per-call reservation never
/// runs. The completion collector must still bound the text/reasoning/tool-argument domain it
/// retains: a single over-limit delta is refused by the collector itself instead of being copied in
/// and growing the retained body without bound.
#[tokio::test]
async fn a_stream_without_a_progress_sender_is_still_bounded_by_the_collector() {
    let oversized = "x".repeat(16 * 1024 * 1024 + 1);
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesHttp,
        "unbounded",
        0,
        Reply::Sse(vec![
            json!({"type":"response.created","response":{"id":"live-stream","model":"live-fixture"}}),
            json!({"type":"response.output_item.added","output_index":0,
                "item":{"id":"message-1","type":"message","role":"assistant",
                    "phase":"final_answer","content":[]}}),
            json!({"type":"response.output_text.delta","item_id":"message-1",
                "output_index":0,"content_index":0,"delta":oversized}),
        ]),
    )])
    .await
    .expect("fixture starts");
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model("live-fixture", ModelTransportProfile::responses_http()),
    )
    .expect("fixture endpoint is valid");

    // No progress sender: this drives the decoder and the collector directly, with no Thread-owned
    // reservation in front of them.
    let failure = tokio::time::timeout(
        Duration::from_secs(30),
        runtime.complete(
            CompletionRequest::builder()
                .messages(vec![user_message("unbounded")])
                .build(),
            ModelInvocationContext::new(ModelSession::default()),
        ),
    )
    .await
    .expect("the collector bound must not stall on an over-limit stream")
    .expect_err("an over-limit stream is refused by the collector itself");
    assert!(
        matches!(&*failure.source, PureError::MemoryError(message) if message.contains("16 MiB")),
        "the collector's own ceiling refuses the stream, not a progress sender: {:?}",
        failure.source
    );
    let _ = fixture.shutdown().await;
}
