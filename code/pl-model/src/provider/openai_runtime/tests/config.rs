use super::*;
use pretty_assertions::assert_eq;

#[test]
fn configured_models_override_bundled_models() {
    let mut model = ModelInfo::fallback("deepseek-v4-flash");
    model.display_name = "Custom DeepSeek".to_string();
    let provider = OpenAiProvider::new(ProviderInfo::deepseek(None), vec![model]).unwrap();

    assert_eq!(
        provider.model_info("deepseek-v4-flash").display_name,
        "Custom DeepSeek"
    );
}

#[test]
fn redacts_openai_api_keys_from_error_text() {
    let input = "Incorrect API key provided: sk-abc123*******************************************************xyz.";

    let redacted = redact_secret_like_values(input);

    assert_eq!(redacted, "Incorrect API key provided: [REDACTED_API_KEY].");
    assert!(!redacted.contains("sk-abc123"));
}

#[test]
fn chat_completions_rejects_websocket_before_creating_a_client() {
    let mut info = ProviderInfo::openai_compatible_chat(
        "Future Chat Provider",
        "http://127.0.0.1:1/v1",
        "future-model",
    );
    info.connection_mode = ProviderConnectionMode::WebSocket;

    let error = OpenAiProvider::new(info, vec![ModelInfo::fallback("future-model")]).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("chat_completions protocol does not support web_socket")
    );
}
