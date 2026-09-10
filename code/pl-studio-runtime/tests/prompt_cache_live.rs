use pl_model::completion::{Message, MessageContent, MessageRole, ModelContextItem};
use pl_model::runtime::{ModelTurnClient, ModelTurnOptions, ModelTurnRequest};

const DEEPSEEK_LIVE_ENV_KEY: &str = "DEEPSEEK_API_KEY";

#[path = "support/engine.rs"]
mod engine_support;

fn live_api_key() -> String {
    std::env::var(DEEPSEEK_LIVE_ENV_KEY)
        .ok()
        .filter(|key| !key.trim().is_empty())
        .expect("DEEPSEEK_API_KEY is required for explicitly requested live acceptance")
}

#[tokio::test]
#[ignore = "requires a real DeepSeek credential and incurs provider usage"]
async fn identical_deepseek_request_reports_provider_cache_read_tokens() {
    let api_key = live_api_key();
    let route = engine_support::deepseek_route(api_key);
    let client = ModelTurnClient::from_route(&route).expect("construct DeepSeek client");
    let stable_prefix = "Pure-Lang prompt cache live evidence. ".repeat(1_500);
    let input = [ModelContextItem::Message {
        message: Message {
            role: MessageRole::User,
            content: MessageContent::text(format!(
                "{stable_prefix}\n只回复 OK，不要调用工具，也不要解释。"
            )),
            tool_calls: None,
            tool_result: None,
            presentation: Default::default(),
            reasoning_content: None,
            metadata: Default::default(),
        },
    }];

    let request = || {
        ModelTurnRequest::new()
            .with_instructions("Follow the user exactly and answer with only OK.")
            .with_max_tokens(Some(16))
    };
    let first = client
        .complete(&input, request(), ModelTurnOptions::default())
        .await
        .expect("first real DeepSeek request");
    let second = client
        .complete(&input, request(), ModelTurnOptions::default())
        .await
        .expect("second identical real DeepSeek request");

    let first_usage = first.accounting().usage.totals();
    let second_usage = second.accounting().usage.totals();
    println!(
        "DeepSeek provider cache evidence: first input={} cached={}; repeated input={} cached={}",
        first_usage.prompt_tokens,
        first_usage.cached_prompt_tokens,
        second_usage.prompt_tokens,
        second_usage.cached_prompt_tokens,
    );
    assert!(first.accounting().usage.totals().prompt_tokens > 0);
    assert!(
        second.accounting().usage.totals().cached_prompt_tokens > 0,
        "provider usage must report a real cache read; local fingerprints are not evidence"
    );
}
