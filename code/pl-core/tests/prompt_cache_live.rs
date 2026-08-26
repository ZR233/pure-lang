use pl_core::{AgentSession, ModelTurnClient, ModelTurnOptions, ModelTurnRequest};

const DEEPSEEK_LIVE_ENV_KEY: &str = "API_KEY_DEEPSEEK";

mod support;

fn live_api_key() -> Option<String> {
    match std::env::var(DEEPSEEK_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{DEEPSEEK_LIVE_ENV_KEY} is not set; skipping live prompt-cache test");
            None
        }
    }
}

#[tokio::test]
#[ignore = "requires a real DeepSeek credential and incurs provider usage"]
async fn identical_deepseek_request_reports_provider_cache_read_tokens() {
    let Some(api_key) = live_api_key() else {
        return;
    };
    let route = support::deepseek_route(api_key);
    let client = ModelTurnClient::from_route(&route).expect("construct DeepSeek client");
    let mut session = AgentSession::new();
    let stable_prefix = "Pure-Lang prompt cache live evidence. ".repeat(1_500);
    session.push_user_prompt(format!(
        "{stable_prefix}\n只回复 OK，不要调用工具，也不要解释。"
    ));

    let request = || {
        ModelTurnRequest::new()
            .with_instructions("Follow the user exactly and answer with only OK.")
            .with_max_tokens(Some(16))
    };
    let first = client
        .complete(&session, request(), ModelTurnOptions::default())
        .await
        .expect("first real DeepSeek request");
    let second = client
        .complete(&session, request(), ModelTurnOptions::default())
        .await
        .expect("second identical real DeepSeek request");

    assert!(first.usage().input_tokens() > 0);
    assert!(
        second.usage().cached_input_tokens() > 0,
        "provider usage must report a real cache read; local fingerprints are not evidence"
    );
}
