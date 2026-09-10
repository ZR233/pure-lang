#![allow(linker_messages)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pl_core::{
    context::ContextContent,
    model::Model,
    thread::{ThreadHandle, TurnInput, TurnOutcome},
};
use pl_model::runtime::ThreadModel;
use pl_model::runtime::{ModelTurnClient, ModelTurnOptions, ModelTurnRequest};
use pl_studio_runtime::search::{WebSearchAvailability, plan_web_search};

use pl_model::config::{
    AgentModelConfig, AgentRoleId, ModelRouteConfig, ProviderConfig, ProviderId,
};
use pl_model::model::ModelInfo;
use pl_model::provider::ProviderEndpoint;
use pl_protocol::MessageRole;
use pl_protocol::search::WebSearchConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn chat_sse(text: &str, response_id: &str) -> String {
    format!(
        "data: {{\"id\":\"{response_id}\",\"model\":\"fixture-model\",\"choices\":[{{\"delta\":{{\"content\":\"<final>{text}</final>\"}},\"finish_reason\":null}}]}}\n\ndata: {{\"id\":\"{response_id}\",\"model\":\"fixture-model\",\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}}}\n\ndata: [DONE]\n\n"
    )
}

async fn serve_chat_sequence(
    responses: Vec<String>,
) -> (
    String,
    Arc<Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&bodies);
    let handle = tokio::spawn(async move {
        for body in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            let (header_end, content_length) = loop {
                let read = socket.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "fixture request ended before headers completed");
                request.extend_from_slice(&chunk[..read]);
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    break (header_end, content_length);
                }
            };
            while request.len() < header_end + 4 + content_length {
                let read = socket.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "fixture request ended before body completed");
                request.extend_from_slice(&chunk[..read]);
            }
            captured.lock().unwrap().push(
                serde_json::from_slice(&request[header_end + 4..header_end + 4 + content_length])
                    .unwrap(),
            );

            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
    });
    (format!("http://{address}"), bodies, handle)
}

fn host_config(base_url: String) -> (AgentModelConfig, AgentRoleId) {
    let provider_id = ProviderId::new("fixture").unwrap();
    let role = AgentRoleId::new("executor").unwrap();
    let mut endpoint = ProviderEndpoint::compatible("Fixture", base_url);
    endpoint.bearer_token = Some("fixture-token".to_string());
    let model = ModelInfo::compatible("fixture-model");
    let provider = ProviderConfig::from_explicit_models(endpoint, vec![model]);
    let config = AgentModelConfig {
        providers: BTreeMap::from([(provider_id.clone(), provider)]),
        routes: BTreeMap::from([(
            role.clone(),
            ModelRouteConfig {
                provider: provider_id,
                model: "fixture-model".to_string(),
                effort: None,
            },
        )]),
    };
    (config, role)
}

#[tokio::test]
async fn facade_supports_route_two_turns_snapshots_thread_and_web_search() {
    let (base_url, bodies, server) = serve_chat_sequence(vec![
        chat_sse("first answer", "response-1"),
        chat_sse("second answer", "response-2"),
        chat_sse("engine answer", "response-3"),
    ])
    .await;
    let (config, role) = host_config(base_url);
    config.validate().unwrap();
    let route = config.resolve(&role).unwrap();

    let client = ModelTurnClient::from_route(&route).unwrap();
    let model = ThreadModel::new(
        pl_model::runtime::ModelRuntime::from_route(&route).unwrap(),
        route.reasoning_config(),
    );
    let mut input = vec![message(MessageRole::User, "first prompt")];
    let first = client
        .complete(&input, ModelTurnRequest::new(), ModelTurnOptions::default())
        .await
        .unwrap();
    let first_text = first
        .output()
        .iter()
        .find_map(|output| output.as_message())
        .unwrap();
    assert_eq!(first.id(), Some("response-1"));
    assert_eq!(first.model(), "fixture-model");
    assert_eq!(first_text, "first answer");
    assert_eq!(first.accounting().usage.totals().prompt_tokens, 1);
    assert_eq!(first.accounting().usage.totals().completion_tokens, 2);
    assert_eq!(first.accounting().usage.totals().total_tokens, 3);

    input.push(message(MessageRole::Assistant, first_text));
    input.push(message(MessageRole::User, "second prompt"));
    let second = client
        .complete(&input, ModelTurnRequest::new(), ModelTurnOptions::default())
        .await
        .unwrap();
    assert_eq!(second.id(), Some("response-2"));
    assert_eq!(
        second
            .output()
            .iter()
            .find_map(|output| output.as_message()),
        Some("second answer")
    );

    let thread =
        ThreadHandle::start("host-contract".into(), model.open_session().await.unwrap()).unwrap();
    let turn = thread
        .run_turn(TurnInput {
            turn_id: "turn".into(),
            attempt_prefix: "request".into(),
            content: vec![ContextContent::Text {
                text: "engine prompt".into(),
            }],
            max_model_steps: std::num::NonZeroU32::new(4).unwrap(),
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(turn.outcome, TurnOutcome::Completed);
    assert_eq!(
        turn.last_output.content[0],
        ContextContent::Text {
            text: "engine answer".into()
        }
    );
    thread.close().await.unwrap();

    let search = plan_web_search(&config, &route, &WebSearchConfig::default()).unwrap();
    assert_eq!(
        search.resolution.availability,
        WebSearchAvailability::ProviderUnsupported
    );

    server.await.unwrap();
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 3);
    let second_messages = bodies[1]["messages"].as_array().unwrap();
    assert_eq!(
        second_messages[0]["role"],
        serde_json::to_value(MessageRole::User).unwrap()
    );
    let second_body = bodies[1].to_string();
    assert!(second_body.contains("first prompt"));
    assert!(second_body.contains("first answer"));
    assert!(second_body.contains("second prompt"));
}

fn message(role: MessageRole, text: &str) -> pl_model::completion::ModelContextItem {
    pl_model::completion::Message {
        role,
        content: pl_model::completion::MessageContent::text(text),
        presentation: Default::default(),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }
    .into()
}
