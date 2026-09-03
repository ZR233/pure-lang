use super::super::provider_error::redact_secret_like_values;
use super::*;
use crate::model::ModelTransportProfile;
use pretty_assertions::assert_eq;

#[test]
fn runtime_binds_the_configured_model() {
    let mut model = ModelInfo::fallback("deepseek-v4-flash");
    model.display_name = "Custom DeepSeek".to_string();
    let provider = ModelRuntime::new(ProviderEndpoint::deepseek(None), model).unwrap();

    assert_eq!(provider.model().display_name, "Custom DeepSeek");
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
    let info = ProviderEndpoint::compatible("Future Chat Provider", "http://127.0.0.1:1/v1");
    let mut model = ModelInfo::fallback("future-model");
    model.transport = ModelTransportProfile {
        protocol: ProviderWireProtocol::ChatCompletions,
        supported_connection_modes: vec![ProviderConnectionMode::WebSocket],
        default_connection_mode: ProviderConnectionMode::WebSocket,
    };

    let error = ModelRuntime::new(info, model).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("chat_completions transport does not support web_socket")
    );
}
