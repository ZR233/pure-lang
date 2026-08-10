use super::super::provider_error::redact_secret_like_values;
use super::*;
use crate::ModelTransportProfile;
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
fn chat_completions_model_rejects_websocket_before_creating_a_client() {
    let info = ProviderInfo::openai_compatible_chat(
        "Future Chat Provider",
        "http://127.0.0.1:1/v1",
        "future-model",
    );
    let mut model = ModelInfo::fallback("future-model");
    model.transport = ModelTransportProfile {
        protocol: ProviderWireProtocol::ChatCompletions,
        supported_connection_modes: vec![ProviderConnectionMode::WebSocket],
        default_connection_mode: ProviderConnectionMode::WebSocket,
    };

    let error = OpenAiProvider::new(info, vec![model]).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("chat_completions transport does not support web_socket")
    );
}
