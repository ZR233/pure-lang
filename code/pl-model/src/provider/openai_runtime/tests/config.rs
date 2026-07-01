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
