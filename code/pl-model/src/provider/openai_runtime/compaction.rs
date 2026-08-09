use std::collections::HashMap;
use std::time::Duration;

use async_openai::Client;
use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{ModelContextItem, PureError, Result};
use serde_json::{Map, Value};

use super::provider_error::openai_error_to_pure;
use super::{OpenAiProvider, PureOpenAiConfig, get_auth_token};
use crate::protocol::openai::sse;
use crate::protocol::openai::{OpenAiProtocol, OpenAiRequestBody};
use crate::provider_info::ProviderWireProtocol;
use crate::request::{
    CompletionRequest, ModelCompactionRequest, ModelCompactionResponse, OpenAiCompactionMode,
    TokenUsage,
};

const REMOTE_COMPACTION_V2_FEATURE: &str = "remote_compaction_v2";
const MAX_REMOTE_V2_RETRIES: u32 = 2;

pub(super) async fn compact_context(
    provider: &OpenAiProvider,
    request: ModelCompactionRequest,
) -> Result<ModelCompactionResponse> {
    if provider.info.protocol != ProviderWireProtocol::Responses {
        return Err(PureError::ConfigError(
            "remote context compaction requires the Responses protocol".to_string(),
        ));
    }
    match request.mode {
        OpenAiCompactionMode::RemoteV2 => compact_v2(provider, request).await,
        OpenAiCompactionMode::Local => Err(PureError::ConfigError(
            "local context compaction must be orchestrated by pl-core".to_string(),
        )),
    }
}

async fn compact_v2(
    provider: &OpenAiProvider,
    request: ModelCompactionRequest,
) -> Result<ModelCompactionResponse> {
    let (mut body, mut model_headers) = build_compaction_body(provider, &request)?;
    let input = body
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| PureError::ConfigError("OpenAI Responses input must be an array".into()))?;
    input.push(serde_json::json!({ "type": "compaction_trigger" }));
    body.insert("stream".to_string(), Value::Bool(true));
    append_beta_feature(
        &mut model_headers,
        provider.info.http_headers.as_ref(),
        REMOTE_COMPACTION_V2_FEATURE,
    );

    let mut retry = 0;
    loop {
        let result = compact_v2_attempt(provider, body.clone(), &model_headers).await;
        match result {
            Ok(response) => return Ok(response),
            Err(error) if retry < MAX_REMOTE_V2_RETRIES && is_retryable(&error) => {
                retry += 1;
                tokio::time::sleep(Duration::from_millis(100 * u64::from(retry))).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn compact_v2_attempt(
    provider: &OpenAiProvider,
    body: Map<String, Value>,
    model_headers: &HashMap<String, String>,
) -> Result<ModelCompactionResponse> {
    let token = get_auth_token(provider.info.bearer_token.clone()).await?;
    let config = PureOpenAiConfig::new(
        provider.resolve_base_url(),
        token,
        provider.info.http_headers.as_ref(),
        model_headers,
    )?;
    let client = Client::build(provider.http_client.clone(), config);
    let mut stream: StreamResponse<sse::SseStreamEvent> = client
        .responses()
        .create_stream_byot(body)
        .await
        .map_err(openai_error_to_pure)?;
    let mut compaction = None;
    let mut compaction_count = 0;
    let mut usage = None;
    let mut completed = false;
    while let Some(event) = stream.next().await {
        let event = event.map_err(openai_error_to_pure)?;
        match event.kind.as_str() {
            "response.output_item.done" => {
                if let Some(item) = event.item
                    && item.get("type").and_then(Value::as_str) == Some("compaction")
                {
                    compaction_count += 1;
                    if compaction.is_none() {
                        compaction = parse_output_item(item)?;
                    }
                }
            }
            "response.completed" => {
                completed = true;
                usage = event
                    .response
                    .as_ref()
                    .and_then(|response| response.get("usage"))
                    .map(token_usage)
                    .transpose()?;
                break;
            }
            _ => {}
        }
    }
    if !completed {
        return Err(PureError::HttpError(
            "OpenAI remote compaction v2 stream closed before response.completed".to_string(),
        ));
    }
    if compaction_count != 1 {
        return Err(PureError::ConfigError(format!(
            "OpenAI remote compaction v2 expected exactly one compaction item, got {compaction_count}"
        )));
    }
    let Some(compaction) = compaction else {
        return Err(PureError::ConfigError(
            "OpenAI remote compaction v2 returned an invalid compaction item".to_string(),
        ));
    };
    Ok(ModelCompactionResponse {
        input: vec![compaction],
        usage,
    })
}

fn build_compaction_body(
    provider: &OpenAiProvider,
    request: &ModelCompactionRequest,
) -> Result<(Map<String, Value>, HashMap<String, String>)> {
    let model_info = provider.model_info(&request.model);
    let effective_capabilities = model_info.capabilities.clone().with_provider_capabilities(
        provider.capabilities,
        provider.info.uses_native_custom_tools(),
    );
    let supports_custom_tools = provider.info.uses_native_custom_tools()
        && effective_capabilities.supports_custom_tools()
        && effective_capabilities.supports_freeform_tools();
    let mut completion = CompletionRequest::builder(request.model.clone())
        .instructions(request.instructions.clone())
        .input(request.input.clone())
        .tools(request.tools.clone())
        .parallel_tool_calls(request.parallel_tool_calls)
        .prompt_cache_key(request.prompt_cache_key.clone())
        .reasoning(request.reasoning.clone())
        .store(Some(false))
        .build()
        .provider_compatible(supports_custom_tools);
    completion.validate_against(&effective_capabilities)?;
    if let Some(api_model) = &model_info.request_profile.api_model {
        completion.model = api_model.clone();
    }
    let OpenAiRequestBody::Responses(mut body) =
        OpenAiProtocol::responses().build_request(&completion, &model_info)?
    else {
        return Err(PureError::ConfigError(
            "OpenAI remote compaction requires the Responses protocol".to_string(),
        ));
    };
    for key in ["tool_choice", "store", "previous_response_id"] {
        body.remove(key);
    }
    Ok((body, model_info.request_profile.headers.clone()))
}

fn append_beta_feature(
    model_headers: &mut HashMap<String, String>,
    provider_headers: Option<&HashMap<String, String>>,
    feature: &str,
) {
    const HEADER: &str = "x-codex-beta-features";
    let mut features = provider_headers
        .into_iter()
        .flat_map(HashMap::iter)
        .chain(model_headers.iter())
        .filter(|(key, _)| key.eq_ignore_ascii_case(HEADER))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if !features.iter().any(|value| value == feature) {
        features.push(feature.to_string());
    }
    features.dedup();
    model_headers.retain(|key, _| !key.eq_ignore_ascii_case(HEADER));
    model_headers.insert(HEADER.to_string(), features.join(","));
}

fn parse_output_item(item: Value) -> Result<Option<ModelContextItem>> {
    match item.get("type").and_then(Value::as_str) {
        Some("compaction") => {
            let encrypted_content = item
                .get("encrypted_content")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    PureError::HttpError(
                        "OpenAI compaction item is missing encrypted_content".to_string(),
                    )
                })?;
            Ok(Some(ModelContextItem::Compaction {
                encrypted_content: encrypted_content.to_string(),
            }))
        }
        _ => Ok(None),
    }
}

fn token_usage(value: &Value) -> Result<TokenUsage> {
    let input = value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let output = value
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cached = value
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    Ok(TokenUsage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(input + output),
        cached_prompt_tokens: cached,
        cache_write_tokens: value
            .get("input_tokens_details")
            .and_then(|details| details.get("cache_write_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        reasoning_tokens: 0,
    })
}

fn is_retryable(error: &PureError) -> bool {
    error.is_transient_model_transport()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compaction_output_item() {
        let parsed = parse_output_item(serde_json::json!({
            "type": "compaction",
            "encrypted_content": "encrypted"
        }))
        .unwrap();

        assert_eq!(
            parsed,
            Some(ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string()
            })
        );
    }

    #[test]
    fn retry_policy_uses_typed_transport_failures_only() {
        assert!(is_retryable(&PureError::transient_model_failure(
            "temporarily unavailable",
            None,
            Some("server_is_overloaded".to_string()),
            Some(503),
        )));
        assert!(!is_retryable(&PureError::LlmError(
            "timeout 429 503 is display text only".to_string(),
        )));
        assert!(!is_retryable(&PureError::HttpError(
            "unclassified HTTP error".to_string(),
        )));
    }
}
