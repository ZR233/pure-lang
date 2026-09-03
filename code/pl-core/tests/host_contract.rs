#![allow(linker_messages)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use pl_core::{
    AgentModelConfig, AgentRoleId, AgentSession, ModelRouteConfig, ModelTurnClient,
    ModelTurnOptions, ModelTurnRequest, ProviderConfig, ProviderId, TurnBudget, TurnEngineBuilder,
    TurnRequest, WebSearchAvailability, plan_web_search,
};
use pl_model::completion::WebSearchConfig;
use pl_model::model::ModelInfo;
use pl_model::provider::ProviderEndpoint;
use pl_protocol::MessageRole;
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
    let model = ModelInfo::fallback("fixture-model");
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
async fn facade_supports_route_two_turns_snapshots_engine_and_web_search() {
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
    let engine = TurnEngineBuilder::from_route(&route).unwrap().build();
    let mut session = AgentSession::new();
    session.push_user_prompt("first prompt".to_string());
    let first = client
        .complete(
            &session,
            ModelTurnRequest::new(),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    let first_text = first
        .output()
        .iter()
        .find_map(|output| output.as_message())
        .unwrap();
    assert_eq!(first.id(), None);
    assert_eq!(first.model(), "fixture-model");
    assert_eq!(first_text, "first answer");
    assert_eq!(first.usage().input_tokens(), 1);
    assert_eq!(first.usage().output_tokens(), 2);
    assert_eq!(first.usage().total_tokens(), 3);

    session.push_assistant_response(first_text.to_string(), None);
    session.push_user_prompt("second prompt".to_string());
    let second = client
        .complete(
            &session,
            ModelTurnRequest::new(),
            ModelTurnOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(second.id(), None);
    assert_eq!(
        second
            .output()
            .iter()
            .find_map(|output| output.as_message()),
        Some("second answer")
    );

    let mut engine_session = AgentSession::new();
    let turn = engine
        .run_turn(
            &mut engine_session,
            TurnRequest::new("engine prompt".to_string())
                .with_budget(TurnBudget::new(std::time::Duration::from_millis(60_000))),
        )
        .await
        .unwrap();
    assert!(turn.is_completed());
    assert_eq!(turn.content, "engine answer");

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
