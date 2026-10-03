use pl_core::{
    context::{ContextContent, OpaquePayload},
    model::{ModelFactory, ModelRecoveryPhase},
    thread::{ModelStepLimit, ThreadHandle, TurnInput},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pl_model::{
    completion::{CompletionRequest, Message, MessageContent, MessageRole},
    model::{ModelInfo, ModelTransportProfile},
    provider::ProviderEndpoint,
    runtime::{ModelInvocationContext, ModelRuntime, ModelSession},
};
use pl_provider_fixture::{FixtureServer, Protocol, Reply, Step, WebSocketAction, responses_text};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn request(prompt: &str) -> CompletionRequest {
    CompletionRequest::builder()
        .messages(vec![Message {
            role: MessageRole::User,
            content: MessageContent::text(prompt),
            presentation: Default::default(),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        }])
        .build()
}

fn runtime(fixture: &FixtureServer) -> ModelRuntime {
    runtime_with_profile(fixture, ModelTransportProfile::responses_websocket())
}

fn runtime_with_profile(fixture: &FixtureServer, profile: ModelTransportProfile) -> ModelRuntime {
    let mut model = ModelInfo::compatible("recovery-model");
    model.binding.set_transport(profile);
    ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", fixture.base_url()),
        model,
    )
    .unwrap()
}

fn temporary_error() -> serde_json::Value {
    json!({"type":"error","error":{"code":"server_error","message":"temporary failure","retry_after_ms":1}})
}

fn interrupted_text(text: &str) -> Vec<serde_json::Value> {
    let mut events = responses_text(text, "interrupted", "recovery-model");
    events.pop();
    events
}

#[tokio::test]
async fn declared_remote_tools_with_response_events_are_not_replayed() {
    for tool in [
        pl_protocol::ToolSpec::ProgrammaticToolCalling,
        pl_protocol::ToolSpec::WebSearch {
            options: pl_protocol::HostedWebSearchOptions::OpenAi {
                external_web_access: false,
                indexed_web_access: None,
                filters: None,
                user_location: None,
                search_context_size: None,
                search_content_types: None,
            },
        },
    ] {
        let mut events = interrupted_text("remote work already started");
        events.push(temporary_error());
        let fixture = FixtureServer::start(vec![Step::prompt(
            Protocol::ResponsesWebSocket,
            "remote",
            0,
            Reply::WebSocket(events),
        )])
        .await
        .unwrap();
        let session = ModelSession::default();
        let mut input = request("remote");
        input.tools = vec![tool];
        let mut model = ModelInfo::compatible("recovery-model");
        model
            .binding
            .set_transport(ModelTransportProfile::responses_websocket());
        model.capabilities.web_search = true;
        model.capabilities.tools.programmatic_tool_calling = true;
        let runtime = ModelRuntime::new(
            ProviderEndpoint::compatible("fixture", fixture.base_url()),
            model,
        )
        .unwrap();
        let failure = runtime
            .complete(input, ModelInvocationContext::new(session.clone()))
            .await
            .unwrap_err();
        assert!(failure.is_transient_model_transport());
        assert_eq!(
            failure.accounting.usage.status(),
            pl_protocol::UsageStatus::Missing
        );
        let progress = failure.partial_progress.unwrap();
        assert_eq!(progress.observation().generation, 0);
        assert!(progress.observation().failed.is_empty());
        assert_eq!(
            progress
                .parts()
                .iter()
                .map(|part| part.text())
                .collect::<String>(),
            "remote work already started"
        );
        session.close().await.unwrap();
        assert_eq!(fixture.finish().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn cancellation_during_handshake_releases_admission_and_close_reaps_the_next_socket() {
    use futures::{SinkExt, StreamExt};
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (reached_tx, reached_rx) = tokio::sync::oneshot::channel();
    let (released_tx, released_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        // Do not finish this first handshake: cancel only after its request really arrived.
        let (mut pending, _) = listener.accept().await.unwrap();
        let mut handshake = Vec::new();
        let mut bytes = [0_u8; 1024];
        while !handshake.windows(4).any(|part| part == b"\r\n\r\n") {
            let read = pending.read(&mut bytes).await.unwrap();
            assert!(read > 0);
            handshake.extend_from_slice(&bytes[..read]);
        }
        reached_tx.send(()).unwrap();
        match pending.read(&mut bytes).await {
            Ok(read) => assert_eq!(read, 0, "cancelled handshake kept sending"),
            Err(error) => assert!(matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            )),
        }
        released_tx.send(()).unwrap();
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
        let wire = socket.next().await.unwrap().unwrap();
        assert!(wire.is_text());
        let input: serde_json::Value = serde_json::from_str(wire.to_text().unwrap()).unwrap();
        assert!(input["previous_response_id"].is_null());
        for event in responses_text("next handshake succeeded", "next", "recovery-model") {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    event.to_string().into(),
                ))
                .await
                .unwrap();
        }
        // Explicit session close must stop the idle pump and release its actual peer socket.
        assert!(!matches!(
            socket.next().await,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(_)))
        ));
    });
    let mut model = ModelInfo::compatible("recovery-model");
    model
        .binding
        .set_transport(ModelTransportProfile::responses_websocket());
    let runtime = ModelRuntime::new(
        ProviderEndpoint::compatible("fixture", format!("http://{address}/v1")),
        model,
    )
    .unwrap();
    let session = ModelSession::default();
    let cancel = CancellationToken::new();
    let context =
        ModelInvocationContext::new(session.clone()).with_cancellation(Some(cancel.clone()));
    let first = tokio::spawn({
        let runtime = runtime.clone();
        async move {
            runtime
                .complete(request("handshake cancelled"), context)
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), reached_rx)
        .await
        .unwrap()
        .unwrap();
    cancel.cancel();
    assert!(first.await.unwrap().unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), released_rx)
        .await
        .unwrap()
        .unwrap();
    let next = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.complete(
            request("next"),
            ModelInvocationContext::new(session.clone()),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(next.content.as_deref(), Some("next handshake succeeded"));
    assert_eq!(next.orchestration.transport_attempts, 1);
    tokio::time::timeout(Duration::from_secs(5), session.close())
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        runtime
            .complete(request("sealed"), ModelInvocationContext::new(session))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn two_websocket_failures_fall_back_to_responses_http_and_stay_there() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "fallback",
            0,
            Reply::WebSocket(vec![temporary_error()]),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "fallback",
            1,
            Reply::WebSocket(vec![temporary_error()]),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "fallback",
            2,
            Reply::Sse(responses_text("HTTP recovered", "http-1", "recovery-model")),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "next",
            3,
            Reply::Sse(responses_text("still HTTP", "http-2", "recovery-model")),
        ),
    ])
    .await
    .unwrap();
    let runtime = runtime(&fixture);
    let session = ModelSession::default();
    let response = runtime
        .complete(
            request("fallback"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    assert_eq!(response.orchestration.transport_attempts, 3);
    assert_eq!(response.orchestration.http_fallbacks, 1);
    assert_eq!(response.observation.failed.len(), 2);
    let next = runtime
        .complete(
            request("next"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    assert_eq!(next.content.as_deref(), Some("still HTTP"));
    assert_eq!(next.orchestration.transport_attempts, 1);
    session.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.method.as_str())
            .collect::<Vec<_>>(),
        ["WS", "WS", "POST", "POST"]
    );
    assert!(records.iter().all(|record| record.path == "/v1/responses"));
    assert_eq!(records[0].body["input"], records[2].body["input"]);
}

#[tokio::test]
async fn upgrade_required_falls_back_immediately_only_when_http_is_declared() {
    for supports_http in [true, false] {
        let mut steps = vec![Step::exact(
            Protocol::ResponsesWebSocket,
            serde_json::Value::Null,
            Reply::WebSocketHandshake { status: 426 },
        )];
        if supports_http {
            steps.push(Step::prompt(
                Protocol::ResponsesHttp,
                "upgrade",
                1,
                Reply::Sse(responses_text("HTTP", "http", "recovery-model")),
            ));
        }
        let fixture = FixtureServer::start(steps).await.unwrap();
        let mut profile = ModelTransportProfile::responses_websocket();
        if !supports_http {
            profile.supported_connection_modes =
                vec![pl_model::provider::ProviderConnectionMode::WebSocket];
        }
        let session = ModelSession::default();
        let result = runtime_with_profile(&fixture, profile)
            .complete(
                request("upgrade"),
                ModelInvocationContext::new(session.clone()),
            )
            .await;
        session.close().await.unwrap();
        if supports_http {
            let response = result.unwrap();
            assert_eq!(response.orchestration.transport_attempts, 2);
            assert_eq!(response.content.as_deref(), Some("HTTP"));
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.source.provider_failure_ref().unwrap().http_status,
                Some(426)
            );
        }
        assert_eq!(
            fixture.finish().await.unwrap().len(),
            if supports_http { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn websocket_only_budget_exhaustion_keeps_last_typed_cause_and_failed_observations() {
    let fixture = FixtureServer::start(
        (0..6)
            .map(|step| {
                let mut events = interrupted_text(&format!("failed {step}"));
                events.push(temporary_error());
                Step::prompt(
                    Protocol::ResponsesWebSocket,
                    "exhaust",
                    step,
                    Reply::WebSocket(events),
                )
            })
            .collect(),
    )
    .await
    .unwrap();
    let mut profile = ModelTransportProfile::responses_websocket();
    profile.supported_connection_modes =
        vec![pl_model::provider::ProviderConnectionMode::WebSocket];
    let session = ModelSession::default();
    let failure = runtime_with_profile(&fixture, profile)
        .complete(
            request("exhaust"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap_err();
    session.close().await.unwrap();
    let typed = failure.source.provider_failure_ref().unwrap();
    assert_eq!(typed.code.as_deref(), Some("server_error"));
    assert_eq!(failure.retry_after_ms(), Some(1));
    let progress = failure.partial_progress.unwrap();
    assert_eq!(progress.observation().generation, 5);
    assert_eq!(progress.observation().failed.len(), 5);
    assert_eq!(
        progress
            .parts()
            .iter()
            .map(|part| part.text())
            .collect::<String>(),
        "failed 5"
    );
    assert_eq!(
        progress.observation().recovery.unwrap().phase,
        pl_core::model::ModelRecoveryPhase::Exhausted
    );
    assert_eq!(fixture.finish().await.unwrap().len(), 6);
}

#[tokio::test]
async fn close_and_abrupt_eof_replay_plain_output_but_protocol_and_auth_errors_do_not() {
    for (fault, retry) in [
        (WebSocketAction::Disconnect, true),
        (
            WebSocketAction::Close {
                code: 1011,
                reason: "temporary".into(),
            },
            true,
        ),
        (
            WebSocketAction::Close {
                code: 1008,
                reason: "permission".into(),
            },
            false,
        ),
        (WebSocketAction::RawText("not-json".into()), false),
        (
            WebSocketAction::Event(
                json!({"type":"error","error":{"code":"invalid_api_key","status":401,"message":"response invalid: authentication failed"}}),
            ),
            false,
        ),
        (
            WebSocketAction::Event(
                json!({"type":"error","error":{"code":"invalid_request_error","message":"invalid response input"}}),
            ),
            false,
        ),
    ] {
        let mut actions = interrupted_text("old output")
            .into_iter()
            .map(WebSocketAction::Event)
            .collect::<Vec<_>>();
        actions.push(fault);
        let mut steps = vec![Step::prompt(
            Protocol::ResponsesWebSocket,
            "fault",
            0,
            Reply::WebSocketScript(actions),
        )];
        if retry {
            steps.push(Step::prompt(
                Protocol::ResponsesWebSocket,
                "fault",
                1,
                Reply::WebSocket(responses_text("new output", "new", "recovery-model")),
            ));
        }
        let fixture = FixtureServer::start(steps).await.unwrap();
        let session = ModelSession::default();
        let result = runtime(&fixture)
            .complete(
                request("fault"),
                ModelInvocationContext::new(session.clone()),
            )
            .await;
        session.close().await.unwrap();
        if retry {
            let response = result.unwrap();
            assert_eq!(response.content.as_deref(), Some("new output"));
            assert_eq!(
                response.observation.failed[0]
                    .parts
                    .iter()
                    .map(|part| part.text())
                    .collect::<String>(),
                "old output"
            );
        } else {
            assert!(!result.unwrap_err().is_transient_model_transport());
        }
        assert_eq!(
            fixture.finish().await.unwrap().len(),
            if retry { 2 } else { 1 }
        );
    }
}

#[tokio::test]
async fn invalid_previous_id_after_response_metadata_recovers_with_full_frozen_history() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(Protocol::ResponsesWebSocket, "first", 0, Reply::WebSocket(responses_text("one", "ws-first", "recovery-model"))),
        Step::prompt(Protocol::ResponsesWebSocket, "second", 1, Reply::WebSocket(vec![
            json!({"type":"response.created","response":{"id":"metadata-before-error","model":"recovery-model"}}),
            json!({"type":"error","error":{"code":"previous_response_not_found","message":"missing previous id"}}),
        ])),
        Step::prompt(Protocol::ResponsesWebSocket, "second", 2, Reply::WebSocket(responses_text("two", "ws-second", "recovery-model"))),
    ]).await.unwrap();
    let runtime = runtime(&fixture);
    let session = ModelSession::default();
    let first = runtime
        .complete(
            request("first"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    let mut second_request = request("first");
    second_request.append_response(&first).unwrap();
    second_request.input.extend(request("second").input);
    let response = runtime
        .complete(second_request, ModelInvocationContext::new(session.clone()))
        .await
        .unwrap();
    session.close().await.unwrap();
    assert_eq!(response.orchestration.continuation_invalid, 1);
    assert_eq!(response.content.as_deref(), Some("two"));
    let records = fixture.finish().await.unwrap();
    assert_eq!(records[1].body["previous_response_id"], "ws-first");
    assert_eq!(records[1].body["input"].as_array().unwrap().len(), 1);
    assert!(records[2].body.get("previous_response_id").is_none());
    assert_eq!(records[2].body["input"].as_array().unwrap().len(), 3);
    let pl_model::completion::AssistantReplay::Responses { output } = first.replay.unwrap() else {
        panic!("Responses request must retain native replay");
    };
    assert_eq!(records[2].body["input"][1], output[0]);
    assert_eq!(records[2].body["input"][2], records[1].body["input"][0]);
}

#[tokio::test]
async fn completed_presentation_item_does_not_prevent_safe_recovery() {
    let mut interrupted = responses_text("discarded attempt", "interrupted", "recovery-model");
    interrupted.pop();
    interrupted.push(json!({"type":"error","error":{"code":"server_error","message":"temporary failure","retry_after_ms":1}}));
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "recover",
            0,
            Reply::WebSocket(interrupted),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "recover",
            1,
            Reply::WebSocket(responses_text(
                "successful attempt",
                "recovered",
                "recovery-model",
            )),
        ),
    ])
    .await
    .unwrap();
    let session = ModelSession::default();
    let result = runtime(&fixture)
        .complete(
            request("recover"),
            ModelInvocationContext::new(session.clone()),
        )
        .await;
    session.close().await.unwrap();
    let response = result.unwrap();
    assert_eq!(response.content.as_deref(), Some("successful attempt"));
    let replay = serde_json::to_string(response.replay.as_ref().unwrap()).unwrap();
    assert!(replay.contains("successful attempt"));
    assert!(!replay.contains("discarded attempt"));
    assert_eq!(response.orchestration.transport_attempts, 2);
    assert_eq!(response.observation.generation, 1);
    assert_eq!(response.observation.failed.len(), 1);
    let discarded = &response.observation.failed[0];
    assert_eq!(discarded.generation, 0);
    assert_eq!(
        discarded
            .parts
            .iter()
            .map(|part| part.text())
            .collect::<String>(),
        "discarded attempt"
    );
    assert_eq!(
        response.observation.recovery.unwrap().phase,
        pl_core::model::ModelRecoveryPhase::Recovered
    );
    let persisted: pl_model::completion::CompletionResponse =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(
        persisted.observation.failed[0]
            .parts
            .iter()
            .map(|part| part.text())
            .collect::<String>(),
        "discarded attempt"
    );
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[1].body["previous_response_id"],
        serde_json::Value::Null
    );
    assert_eq!(records[0].body["input"], records[1].body["input"]);
}

#[tokio::test]
async fn websocket_failed_terminal_preserves_usage_and_does_not_replay() {
    let fixture = FixtureServer::start(vec![Step::prompt(
        Protocol::ResponsesWebSocket, "billed", 0, Reply::WebSocket(vec![
            json!({"type":"response.created","response":{"id":"billed","model":"recovery-model"}}),
            json!({"type":"response.failed","response":{"id":"billed","model":"reported-model","usage":{"input_tokens":11,"output_tokens":3},"error":{"code":"server_error","message":"billed failure"}}}),
        ]),
    )]).await.unwrap();
    let session = ModelSession::default();
    let result = runtime(&fixture)
        .complete(
            request("billed"),
            ModelInvocationContext::new(session.clone()),
        )
        .await;
    session.close().await.unwrap();
    let failure = result.unwrap_err();
    assert_eq!(failure.accounting.usage.input_tokens, Some(11));
    assert_eq!(failure.accounting.usage.output_tokens, Some(3));
    assert_eq!(
        failure
            .model_observation()
            .unwrap()
            .reported_model
            .as_deref(),
        Some("reported-model")
    );
    assert_eq!(fixture.finish().await.unwrap().len(), 1);
}

async fn thread(fixture: &FixtureServer) -> ThreadHandle {
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(runtime(fixture), None))
        .open_session()
        .await
        .unwrap();
    ThreadHandle::start("recovery-thread".into(), session).unwrap()
}

fn turn(prompt: &str, id: &str, steps: u32) -> TurnInput {
    TurnInput {
        turn_id: id.into(),
        attempt_prefix: id.into(),
        content: vec![ContextContent::Text {
            text: Arc::from(prompt),
        }],
        max_model_steps: ModelStepLimit::Limited(steps.try_into().unwrap()),
        cancellation: CancellationToken::new(),
    }
}

#[tokio::test]
async fn thread_slow_observer_and_final_receipt_keep_failed_generation_without_polluting_input() {
    let mut events = interrupted_text("discarded secret fragment");
    events.push(temporary_error());
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "thread recover",
            0,
            Reply::WebSocket(events),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "thread recover",
            1,
            Reply::WebSocket(responses_text("canonical answer", "ok", "recovery-model")),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "followup",
            2,
            Reply::WebSocket(responses_text("next answer", "next", "recovery-model")),
        ),
    ])
    .await
    .unwrap();
    let thread = thread(&fixture).await;
    // Deliberately do not consume progress frames: final receipt is authoritative even when watch
    // has coalesced the complete failure/retry/success sequence away from a slow subscriber.
    let result = thread
        .run_turn(turn("thread recover", "first-turn", 1))
        .await
        .unwrap();
    assert_eq!(result.model_steps, 1);
    let effects = thread.effects().await.unwrap();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(
                effect.attempt.as_ref().map(|attempt| &attempt.outcome),
                Some(pl_core::thread::AttemptOutcome::Running)
            ))
            .count(),
        1
    );
    let output = effects
        .iter()
        .find_map(
            |effect| match effect.attempt.as_ref().map(|attempt| &attempt.outcome) {
                Some(pl_core::thread::AttemptOutcome::Committed(output)) => Some(output),
                _ => None,
            },
        )
        .unwrap();
    let receipt = pl_model::runtime::model_response_receipt(output)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.response.observation.generation, 1);
    assert_eq!(
        receipt.response.observation.failed[0]
            .parts
            .iter()
            .map(|part| part.text())
            .collect::<String>(),
        "discarded secret fragment"
    );
    assert_eq!(
        receipt.response.observation.recovery.unwrap().phase,
        ModelRecoveryPhase::Recovered
    );
    thread
        .run_turn(turn("followup", "second-turn", 1))
        .await
        .unwrap();
    thread.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 3);
    assert!(
        !records[2]
            .body
            .to_string()
            .contains("discarded secret fragment")
    );
    // The failed observation is not in history; healthy continuation may send only the new input.
    assert_eq!(records[2].body["previous_response_id"], "ok");
}

#[tokio::test]
async fn thread_http_fallback_survives_physical_reset_after_failed_turn() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "failed turn",
            0,
            Reply::WebSocket(vec![temporary_error()]),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "failed turn",
            1,
            Reply::WebSocket(vec![temporary_error()]),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "failed turn",
            2,
            Reply::HttpError {
                status: 400,
                code: "invalid_request_error".into(),
                message: "cannot recover this request".into(),
            },
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "next turn",
            3,
            Reply::Sse(responses_text("resumed", "resumed", "recovery-model")),
        ),
    ])
    .await
    .unwrap();
    let thread = thread(&fixture).await;
    assert!(
        thread
            .run_turn(turn("failed turn", "failed-turn", 1))
            .await
            .is_err()
    );
    thread
        .run_turn(turn("next turn", "next-turn", 1))
        .await
        .unwrap();
    thread.close().await.unwrap();
    assert_eq!(
        fixture
            .finish()
            .await
            .unwrap()
            .iter()
            .map(|record| record.method.as_str())
            .collect::<Vec<_>>(),
        ["WS", "WS", "POST", "POST"]
    );
}

#[derive(Debug)]
struct RecordingLookup(Arc<Mutex<Vec<(String, String)>>>);

impl Tool for RecordingLookup {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.0
            .lock()
            .unwrap()
            .push((context.call_id, input.content().to_owned()));
        Ok(ToolOutput::new(
            input.clone(),
            vec![ContextContent::Text {
                text: Arc::from(input.content()),
            }],
        ))
    }
}

fn tool_events(call_id: &str, argument: &str) -> Vec<serde_json::Value> {
    pl_provider_fixture::responses_tool_calls(
        "tool-response",
        "recovery-model",
        &[pl_provider_fixture::RealtimeToolCall {
            item_id: "same-tool-item",
            call_id,
            name: "lookup",
            arguments: json!({"term":argument}).to_string(),
        }],
    )
}

#[tokio::test]
async fn retry_never_executes_discarded_tool_call_or_reexecutes_preceding_result() {
    let mut interrupted = tool_events("second-call", "discarded argument");
    interrupted.pop();
    let mut actions = interrupted
        .into_iter()
        .map(WebSocketAction::Event)
        .collect::<Vec<_>>();
    actions.push(WebSocketAction::Disconnect);
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "tools",
            0,
            Reply::WebSocket(tool_events("first-call", "preceding argument")),
        ),
        Step::tool_output(
            Protocol::ResponsesWebSocket,
            "first-call",
            "preceding argument",
            1,
            Reply::WebSocketScript(actions),
        ),
        Step::tool_output(
            Protocol::ResponsesWebSocket,
            "first-call",
            "preceding argument",
            2,
            Reply::WebSocket(tool_events("second-call", "successful argument")),
        ),
        Step::tool_output(
            Protocol::ResponsesWebSocket,
            "second-call",
            "successful argument",
            3,
            Reply::WebSocket(responses_text("tools finished", "final", "recovery-model")),
        ),
    ])
    .await
    .unwrap();
    let thread = thread(&fixture).await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    thread
        .register_tools(vec![
            Registration::new(
                "lookup".into(),
                pl_model::runtime::thread_tool_declaration(
                    &pl_model::completion::ToolSpec::function(
                        "lookup",
                        "Lookup",
                        json!({"type":"object","properties":{"term":{"type":"string"}}}),
                    ),
                )
                .unwrap(),
                RecordingLookup(calls.clone()),
            )
            .unwrap(),
        ])
        .await
        .unwrap();
    let completed = thread
        .run_turn(turn("tools", "tools-turn", 3))
        .await
        .unwrap();
    assert_eq!(completed.model_steps, 3);
    let calls = calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "first-call");
    assert_eq!(calls[1].0, "second-call");
    assert!(calls[1].1.contains("successful argument"));
    assert!(
        !calls
            .iter()
            .any(|(_, input)| input.contains("discarded argument"))
    );
    thread.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(records[1].body["previous_response_id"], "tool-response");
    assert!(records[2].body.get("previous_response_id").is_none());
    // Healthy continuation sends only the new tool result. Recovery must instead
    // send its full frozen prefix, including the preceding native call unchanged.
    let prior_call = tool_events("first-call", "preceding argument")
        .into_iter()
        .find(|event| event["type"] == "response.output_item.done")
        .unwrap()["item"]
        .clone();
    let mut frozen = records[0].body["input"].as_array().unwrap().clone();
    frozen.push(prior_call);
    frozen.extend(records[1].body["input"].as_array().unwrap().iter().cloned());
    assert_eq!(records[2].body["input"], json!(frozen));
    assert!(!records[3].body.to_string().contains("discarded argument"));
}

#[tokio::test]
async fn controlled_websocket_idle_is_300_seconds_and_ping_does_not_extend_it() {
    let reached = CancellationToken::new();
    let release = CancellationToken::new();
    let mut actions = interrupted_text("idle fragment")
        .into_iter()
        .map(WebSocketAction::Event)
        .collect::<Vec<_>>();
    actions.push(WebSocketAction::Barrier {
        reached: reached.clone(),
        release: release.clone(),
    });
    actions.push(WebSocketAction::Ping);
    actions.push(WebSocketAction::Barrier {
        reached: CancellationToken::new(),
        release: CancellationToken::new(),
    });
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "idle",
            0,
            Reply::WebSocketScript(actions),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "idle",
            1,
            Reply::WebSocket(responses_text(
                "recovered after idle",
                "recovered",
                "recovery-model",
            )),
        ),
    ])
    .await
    .unwrap();
    let thread = thread(&fixture).await;
    let mut subscription = thread.subscribe();
    let invocation = thread.run_turn(turn("idle", "idle-turn", 1));
    tokio::pin!(invocation);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                _ = &mut invocation => panic!("idle request ended too early"),
                snapshot = subscription.next() => {
                    if snapshot.unwrap().model_progress.as_ref().is_some_and(|active| !active.progress.parts().is_empty()) { break; }
                }
            }
        }
    }).await.unwrap();
    reached.cancelled().await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(180)).await;
    assert_eq!(
        fixture.recorded().len(),
        1,
        "HTTP's shorter collector deadline must not terminate WS"
    );
    release.cancel();
    tokio::time::advance(Duration::from_secs(119)).await;
    assert_eq!(fixture.recorded().len(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    // Resume real time for loopback I/O. The response deadline itself was exercised virtually.
    tokio::time::resume();
    let completed = tokio::time::timeout(Duration::from_secs(5), invocation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.model_steps, 1);
    thread.close().await.unwrap();
    assert_eq!(fixture.finish().await.unwrap().len(), 2);
}

#[tokio::test]
async fn completed_or_partial_reasoning_is_display_only_when_a_safe_retry_succeeds() {
    for completed in [false, true] {
        let mut events = vec![
            json!({"type":"response.created","response":{"id":"reasoning-failed","model":"recovery-model"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"reasoning-item","type":"reasoning","summary":[]}}),
            json!({"type":"response.reasoning_summary_text.delta","item_id":"reasoning-item","output_index":0,"summary_index":0,"delta":"abandoned reasoning"}),
        ];
        if completed {
            events.push(json!({"type":"response.output_item.done","output_index":0,"item":{"id":"reasoning-item","type":"reasoning","summary":[{"type":"summary_text","text":"abandoned reasoning"}]}}));
        }
        let mut actions = events
            .into_iter()
            .map(WebSocketAction::Event)
            .collect::<Vec<_>>();
        actions.push(WebSocketAction::Disconnect);
        let fixture = FixtureServer::start(vec![
            Step::prompt(
                Protocol::ResponsesWebSocket,
                "reasoning",
                0,
                Reply::WebSocketScript(actions),
            ),
            Step::prompt(
                Protocol::ResponsesWebSocket,
                "reasoning",
                1,
                Reply::WebSocket(responses_text(
                    "successful answer",
                    "successful",
                    "recovery-model",
                )),
            ),
        ])
        .await
        .unwrap();
        let session = ModelSession::default();
        let response = runtime(&fixture)
            .complete(
                request("reasoning"),
                ModelInvocationContext::new(session.clone()),
            )
            .await
            .unwrap();
        assert_eq!(response.content.as_deref(), Some("successful answer"));
        assert!(
            response
                .reasoning_content
                .as_ref()
                .is_none_or(String::is_empty)
        );
        assert_eq!(response.observation.failed.len(), 1);
        assert_eq!(
            response.observation.failed[0]
                .parts
                .iter()
                .map(|part| part.text())
                .collect::<String>(),
            "abandoned reasoning"
        );
        session.close().await.unwrap();
        let records = fixture.finish().await.unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].body["input"], records[1].body["input"]);
    }
}

#[tokio::test]
async fn invalid_continuation_replays_native_phase_reasoning_and_output_fields_without_candidates()
{
    let output = vec![
        json!({"id":"original-reason","type":"reasoning","summary":[],"encrypted_content":"frozen-reasoning"}),
        json!({"id":"original-commentary","type":"message","role":"assistant","phase":"commentary","status":"completed","content":[{"type":"output_text","text":"original commentary","annotations":[]}]}),
        json!({"id":"original-final","type":"message","role":"assistant","phase":"final_answer","status":"completed","content":[{"type":"output_text","text":"original final","annotations":[]}]}),
    ];
    let first_events = output.iter().enumerate().map(|(index, item)| json!({"type":"response.output_item.done","output_index":index,"item":item}))
        .chain([json!({"type":"response.completed","response":{"id":"native-first","model":"recovery-model","output":output,"usage":{"input_tokens":11,"output_tokens":3}}})]).collect();
    let mut candidate = interrupted_text("discarded native candidate");
    candidate.push(json!({"type":"error","error":{"code":"invalid_previous_response_id","message":"expired continuation"}}));
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "native first",
            0,
            Reply::WebSocket(first_events),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "native second",
            1,
            Reply::WebSocket(candidate),
        ),
        Step::prompt(
            Protocol::ResponsesWebSocket,
            "native second",
            2,
            Reply::WebSocket(responses_text(
                "native success",
                "native-second",
                "recovery-model",
            )),
        ),
    ])
    .await
    .unwrap();
    let runtime = runtime(&fixture);
    let session = ModelSession::default();
    let first = runtime
        .complete(
            request("native first"),
            ModelInvocationContext::new(session.clone()),
        )
        .await
        .unwrap();
    let mut next = request("native first");
    next.append_response(&first).unwrap();
    next.input.extend(request("native second").input);
    let response = runtime
        .complete(next, ModelInvocationContext::new(session.clone()))
        .await
        .unwrap();
    assert_eq!(response.orchestration.continuation_invalid, 1);
    assert_eq!(
        response.observation.failed[0]
            .parts
            .iter()
            .map(|part| part.text())
            .collect::<String>(),
        "discarded native candidate"
    );
    let success_replay = serde_json::to_string(response.replay.as_ref().unwrap()).unwrap();
    assert!(success_replay.contains("native success"));
    assert!(!success_replay.contains("discarded native candidate"));
    session.close().await.unwrap();
    let records = fixture.finish().await.unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[1].body["previous_response_id"], "native-first");
    let mut frozen = records[0].body["input"].as_array().unwrap().clone();
    frozen.extend(output);
    frozen.extend(records[1].body["input"].as_array().unwrap().iter().cloned());
    assert_eq!(records[2].body["input"], json!(frozen));
    assert!(records[2].body.get("previous_response_id").is_none());
    assert!(
        !records[2]
            .body
            .to_string()
            .contains("discarded native candidate")
    );
}

#[tokio::test]
async fn cancellation_during_read_or_backoff_keeps_receipt_and_next_turn_uses_fresh_connection() {
    for backoff in [false, true] {
        let mut actions = interrupted_text("cancelled fragment")
            .into_iter()
            .map(WebSocketAction::Event)
            .collect::<Vec<_>>();
        if backoff {
            actions.push(WebSocketAction::Event(json!({"type":"error","error":{"code":"server_error","message":"wait before retry","retry_after_ms":30000}})));
        } else {
            actions.push(WebSocketAction::Barrier {
                reached: CancellationToken::new(),
                release: CancellationToken::new(),
            });
        }
        let fixture = FixtureServer::start(vec![
            Step::prompt(
                Protocol::ResponsesWebSocket,
                "cancel",
                0,
                Reply::WebSocketScript(actions),
            ),
            Step::prompt(
                Protocol::ResponsesWebSocket,
                "after cancel",
                1,
                Reply::WebSocket(responses_text("next turn works", "next", "recovery-model")),
            ),
        ])
        .await
        .unwrap();
        let thread = thread(&fixture).await;
        let mut subscription = thread.subscribe();
        let input = turn("cancel", "cancel-turn", 1);
        let token = input.cancellation.clone();
        let runner = tokio::spawn({
            let thread = thread.clone();
            async move { thread.run_turn(input).await }
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = subscription.next().await.unwrap();
                if snapshot.model_progress.as_ref().is_some_and(|active| {
                    if backoff {
                        active.progress.observation().generation == 1
                    } else {
                        !active.progress.parts().is_empty()
                    }
                }) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        token.cancel();
        let result = tokio::time::timeout(Duration::from_secs(5), runner)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Err(pl_core::thread::ThreadError::Cancelled)
        ));
        assert_eq!(
            fixture.recorded().len(),
            1,
            "cancellation must not send a retry"
        );
        let effects = thread.effects().await.unwrap();
        let error = effects
            .iter()
            .find_map(
                |effect| match effect.attempt.as_ref().map(|attempt| &attempt.outcome) {
                    Some(pl_core::thread::AttemptOutcome::Cancelled { result: Err(error) }) => {
                        Some(error)
                    }
                    _ => None,
                },
            )
            .unwrap();
        let receipt = pl_model::runtime::model_failure_receipt(error)
            .unwrap()
            .unwrap();
        let progress = receipt.partial_progress.unwrap();
        if backoff {
            assert!(
                receipt.presentation_items.is_empty(),
                "retired provider parts must exist only in the failed generation, not also in the unsent current generation"
            );
            assert!(progress.parts().is_empty());
            assert_eq!(
                progress.observation().recovery.unwrap().phase,
                ModelRecoveryPhase::Cancelled
            );
            assert_eq!(
                progress.observation().failed[0]
                    .parts
                    .iter()
                    .map(|part| part.text())
                    .collect::<String>(),
                "cancelled fragment"
            );
        } else {
            assert_eq!(
                progress
                    .parts()
                    .iter()
                    .map(|part| part.text())
                    .collect::<String>(),
                "cancelled fragment"
            );
        }
        thread
            .run_turn(turn("after cancel", "next-turn", 1))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), thread.close())
            .await
            .unwrap()
            .unwrap();
        let records = fixture.finish().await.unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[1].body.get("previous_response_id").is_none());
        assert!(!records[1].body.to_string().contains("cancelled fragment"));
    }
}

#[tokio::test]
async fn controlled_http_idle_remains_180_seconds() {
    let fixture = FixtureServer::start(vec![
        Step::prompt(
            Protocol::ResponsesHttp,
            "http idle",
            0,
            Reply::HangingSse(interrupted_text("HTTP partial")),
        ),
        Step::prompt(
            Protocol::ResponsesHttp,
            "http idle",
            1,
            Reply::Sse(responses_text("HTTP success", "http-ok", "recovery-model")),
        ),
    ])
    .await
    .unwrap();
    let session = ModelFactory::new(pl_model::runtime::ThreadModel::new(
        runtime_with_profile(&fixture, ModelTransportProfile::responses_http()),
        None,
    ))
    .open_session()
    .await
    .unwrap();
    let thread = ThreadHandle::start("http-idle-thread".into(), session).unwrap();
    let mut subscription = thread.subscribe();
    let invocation = thread.run_turn(turn("http idle", "http-idle-turn", 1));
    tokio::pin!(invocation);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop { tokio::select! {
            _ = &mut invocation => panic!("HTTP idle ended too early"),
            snapshot = subscription.next() => if snapshot.unwrap().model_progress.as_ref().is_some_and(|active| !active.progress.parts().is_empty()) { break; },
        } }
    }).await.unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(179)).await;
    assert_eq!(fixture.recorded().len(), 1);
    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::time::resume();
    let result = tokio::time::timeout(Duration::from_secs(5), invocation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.model_steps, 1);
    thread.close().await.unwrap();
    assert_eq!(fixture.finish().await.unwrap().len(), 2);
}
