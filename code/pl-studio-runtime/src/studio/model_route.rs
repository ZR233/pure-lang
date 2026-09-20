//! Durable model selectors are separate from the physical provider binding resolved at runtime.

use anyhow::{Context, Result, bail};
use pl_core::{
    context::OpaquePayload,
    thread::{ThreadSnapshot, extensions::ExtensionRecord},
};
use pl_model::config::{ModelRouteConfig, ProviderId, ReasoningEffort};

pub(crate) const MODEL_ROUTE_EXTENSION: &str = "studio.model-route";
pub(crate) const MODEL_ROUTE_FORMAT: &str = "pl.studio.model-route";
pub(crate) const AGENT_PROFILE_EXTENSION: &str = "studio.agent-profile";
pub(crate) const AGENT_PROFILE_FORMAT: &str = "pl.studio.agent-profile";
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(crate) struct SavedModelRoute {
    pub route: ModelRouteConfig,
}

pub(crate) fn encode(route: &ModelRouteConfig) -> Result<OpaquePayload> {
    Ok(OpaquePayload::new(
        MODEL_ROUTE_FORMAT,
        FORMAT_VERSION,
        serde_json::to_string(route)?,
    )?)
}

pub(crate) fn decode(payload: &OpaquePayload) -> Result<ModelRouteConfig> {
    decode_payload(payload, MODEL_ROUTE_FORMAT, "model route")
}

pub(crate) fn saved(state: &ThreadSnapshot) -> Result<Option<SavedModelRoute>> {
    state
        .extensions
        .get(MODEL_ROUTE_EXTENSION)
        .map(|record| {
            Ok(SavedModelRoute {
                route: decode(&record.payload)?,
            })
        })
        .transpose()
}

pub(crate) fn encode_profile(profile: &pl_protocol::AgentProfileSnapshot) -> Result<OpaquePayload> {
    Ok(OpaquePayload::new(
        AGENT_PROFILE_FORMAT,
        FORMAT_VERSION,
        serde_json::to_string(profile)?,
    )?)
}

pub(crate) fn saved_profile(
    state: &ThreadSnapshot,
) -> Result<Option<(pl_protocol::AgentProfileSnapshot, u64)>> {
    state
        .extensions
        .get(AGENT_PROFILE_EXTENSION)
        .map(|record| {
            Ok((
                decode_payload(&record.payload, AGENT_PROFILE_FORMAT, "Agent Profile")?,
                record.revision,
            ))
        })
        .transpose()
}

pub(crate) fn profile_route(
    profile: &pl_protocol::AgentProfileSnapshot,
) -> Result<ModelRouteConfig> {
    Ok(ModelRouteConfig {
        provider: ProviderId::new(profile.provider_id.clone())?,
        model: profile.model.clone(),
        effort: profile.effort.clone().map(ReasoningEffort::new),
    })
}

/// Reads the newest decodable admission receipt without consulting current provider settings.
pub(crate) fn latest_request_route(state: &ThreadSnapshot) -> Option<ModelRouteConfig> {
    state.attempts.iter().rev().find_map(|attempt| {
        let receipt =
            pl_model::runtime::model_request_receipt(attempt.request_metadata.as_ref()?).ok()?;
        Some(ModelRouteConfig {
            provider: ProviderId::new(receipt.binding.provider_instance_id).ok()?,
            model: receipt.binding.requested_model,
            effort: receipt
                .reasoning
                .and_then(|reasoning| reasoning.effort)
                .map(ReasoningEffort::new),
        })
    })
}

pub(crate) fn route_record(
    state: &ThreadSnapshot,
    child: bool,
) -> Result<Option<(&ExtensionRecord, ModelRouteConfig)>> {
    if child {
        let Some(record) = state.extensions.get(AGENT_PROFILE_EXTENSION) else {
            return Ok(None);
        };
        let profile: pl_protocol::AgentProfileSnapshot =
            decode_payload(&record.payload, AGENT_PROFILE_FORMAT, "Agent Profile")?;
        Ok(Some((record, profile_route(&profile)?)))
    } else {
        let Some(record) = state.extensions.get(MODEL_ROUTE_EXTENSION) else {
            return Ok(None);
        };
        Ok(Some((record, decode(&record.payload)?)))
    }
}

fn decode_payload<T: serde::de::DeserializeOwned>(
    payload: &OpaquePayload,
    expected_format: &str,
    label: &str,
) -> Result<T> {
    if payload.format() != expected_format || payload.version() != FORMAT_VERSION {
        bail!(
            "unsupported saved {label} codec {} version {}",
            payload.format(),
            payload.version()
        );
    }
    serde_json::from_str(payload.content()).with_context(|| format!("decode saved {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::thread::{AttemptOutcome, RequestAttempt};

    fn prepared_request(provider: &str, model: &str, effort: Option<&str>) -> OpaquePayload {
        OpaquePayload::new(
            "pl.model.prepared-request",
            1,
            serde_json::json!({
                "binding": {
                    "providerInstanceId": provider,
                    "requestedModel": model,
                    "adapter": "deepSeek",
                    "protocol": "responses",
                    "isolation": "fixture",
                    "purpose": "turn",
                    "contextWindow": 1000000
                },
                "tools": [],
                "toolChoice": "auto",
                "parallelToolCalls": false,
                "reasoning": effort.map(|effort| serde_json::json!({
                    "effort": effort,
                    "summary": "enabled"
                })),
                "temperature": null,
                "maxTokens": null
            })
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn saved_model_route_rejects_unknown_codec_and_corrupt_json() {
        let unsupported = OpaquePayload::new(MODEL_ROUTE_FORMAT, 2, "{}").unwrap();
        assert!(
            decode(&unsupported)
                .unwrap_err()
                .to_string()
                .contains("unsupported saved model route codec")
        );
        let corrupt = OpaquePayload::new(MODEL_ROUTE_FORMAT, 1, "{").unwrap();
        assert!(
            decode(&corrupt)
                .unwrap_err()
                .to_string()
                .contains("decode saved model route")
        );
    }

    #[test]
    fn latest_request_route_uses_the_newest_decodable_receipt() {
        let attempt = |attempt_id: &str, metadata: Option<OpaquePayload>| RequestAttempt {
            request_metadata: metadata,
            tool_projection: None,
            turn_id: "turn".into(),
            attempt_id: attempt_id.into(),
            retry_of: None,
            input: Default::default(),
            tools: Vec::new().into(),
            outcome: AttemptOutcome::Interrupted,
            input_estimate: None,
        };
        let state = ThreadSnapshot {
            attempts: vec![
                attempt(
                    "old",
                    Some(prepared_request("deepseek", "deepseek-flash", Some("high"))),
                ),
                attempt("corrupt", Some(OpaquePayload::text("not a receipt"))),
                attempt(
                    "new",
                    Some(prepared_request("deepseek", "deepseek-v4-pro", Some("max"))),
                ),
            ]
            .into(),
            ..Default::default()
        };

        let route = latest_request_route(&state).unwrap();
        assert_eq!(route.provider.as_str(), "deepseek");
        assert_eq!(route.model, "deepseek-v4-pro");
        assert_eq!(route.effort.unwrap().as_str(), "max");
    }
}
