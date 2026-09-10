//! Stable provider cache hints, independent of execution and diagnostic identities.
use pl_protocol::ThreadPromptSnapshot;
use sha2::{Digest, Sha256};

/// Derives a deterministic cache hint from stable model input identity and host isolation.
/// The isolation value must represent an account/deployment boundary and contain no credentials.
/// Appended dynamic context, timestamps, scope labels and diagnostic generations do not rotate it.
pub fn derive_prompt_cache_key(isolation: &str, prompt: &ThreadPromptSnapshot) -> String {
    let mut hash = Sha256::new();
    for part in [
        "pl-model/cache-key/v1",
        isolation,
        &prompt.provider_hash,
        &prompt.model,
        &prompt.fixed_prefix_hash,
        &prompt.request_properties_hash,
        &prompt.tool_schema_hash,
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("pl:{:x}", hash.finalize())
}

/// Creates an account/deployment isolation namespace without exposing endpoint credentials.
/// Only configured connection material participates; execution IDs and generated transport headers do not.
pub fn binding_cache_namespace(
    provider_id: &str,
    endpoint: &crate::provider::ProviderEndpoint,
) -> String {
    let mut hash = Sha256::new();
    let mut field = |value: &[u8]| {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    };
    field(b"pl-model/cache-isolation/v1");
    field(provider_id.as_bytes());
    field(endpoint.base_url.as_bytes());
    field(
        endpoint
            .bearer_token
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    );
    let mut headers = endpoint
        .http_headers
        .iter()
        .flat_map(|headers| headers.iter())
        .map(|(name, value)| (name.to_ascii_lowercase(), value.as_str()))
        .collect::<Vec<_>>();
    headers.sort();
    for (name, value) in headers {
        field(name.as_bytes());
        field(value.as_bytes());
    }
    format!("binding:{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prompt() -> ThreadPromptSnapshot {
        ThreadPromptSnapshot {
            scope: "role".into(),
            generation: 1,
            provider: "provider".into(),
            provider_hash: "deployment".into(),
            model: "model".into(),
            fixed_prefix_hash: "instructions".into(),
            fixed_prefix_section_hashes: Default::default(),
            request_properties_hash: "options".into(),
            tool_schema_hash: "tools".into(),
            context_hash: "context".into(),
            prompt_cache_policy: "enabled".into(),
            prefix_changed_reason: pl_protocol::PromptPrefixChangedReason::Initial,
            updated_at: 1,
        }
    }
    #[test]
    fn appended_context_and_diagnostic_revision_do_not_rotate_cache_identity() {
        let original = prompt();
        let mut next = original.clone();
        next.generation += 1;
        next.updated_at += 60;
        next.context_hash = "appended facts".into();
        next.scope = "another thread".into();
        assert_eq!(
            derive_prompt_cache_key("account", &original),
            derive_prompt_cache_key("account", &next)
        );
        assert_ne!(
            derive_prompt_cache_key("account", &original),
            derive_prompt_cache_key("another-account", &next)
        );
        next.tool_schema_hash = "changed declaration".into();
        assert_ne!(
            derive_prompt_cache_key("account", &original),
            derive_prompt_cache_key("account", &next)
        );
    }
    #[test]
    fn cache_isolation_changes_with_account_or_deployment_but_not_header_order() {
        let mut endpoint = crate::provider::ProviderEndpoint::openai(None);
        endpoint.bearer_token = Some("fixture-account-secret".into());
        endpoint.http_headers = Some(std::collections::HashMap::from([
            ("OpenAI-Project".into(), "project-a".into()),
            ("X-Account".into(), "account-a".into()),
        ]));
        let first = binding_cache_namespace("configured-provider", &endpoint);
        assert!(!first.contains("fixture-account-secret"));
        endpoint.http_headers = Some(std::collections::HashMap::from([
            ("x-account".into(), "account-a".into()),
            ("openai-project".into(), "project-a".into()),
        ]));
        endpoint.name = "renamed display label".into();
        assert_eq!(
            first,
            binding_cache_namespace("configured-provider", &endpoint)
        );
        endpoint.bearer_token = Some("different-account-secret".into());
        let second = binding_cache_namespace("configured-provider", &endpoint);
        assert_ne!(first, second);
        endpoint.base_url = "https://another-deployment.example/v1".into();
        assert_ne!(
            second,
            binding_cache_namespace("configured-provider", &endpoint)
        );
    }
}
