//! Native remote compaction reuses the complete inference lifecycle and accounting.
use super::{InvocationRunner, ModelInvocationContext};
use crate::completion::tool_schema::CustomToolProjection;
use crate::completion::{
    CompletionFailure, CompletionRequest, ModelCompactionRequest, ModelCompactionResponse,
    OpenAiCompactionMode,
};
use crate::provider::ProviderWireProtocol;
use crate::runtime::openai::{OpenAiProtocol, OpenAiRequestBody};
use pl_protocol::{ModelContextItem, PureError, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;

const REMOTE_COMPACTION_V2_FEATURE: &str = "remote_compaction_v2";

pub(super) async fn compact_context(
    provider: &InvocationRunner,
    request: ModelCompactionRequest,
    context: ModelInvocationContext,
) -> std::result::Result<ModelCompactionResponse, CompletionFailure> {
    if !provider.endpoint().service_capabilities.remote_compaction
        || provider.model().binding.transport.protocol != ProviderWireProtocol::Responses
    {
        return Err(PureError::ConfigError(
            "endpoint does not support native remote compaction".into(),
        )
        .into());
    }
    match request.mode {
        OpenAiCompactionMode::Local => {
            return Err(PureError::ConfigError(
                "local compaction belongs to core orchestration".into(),
            )
            .into());
        }
        OpenAiCompactionMode::RemoteV2 => {}
    }
    let (mut body, mut headers) = build_compaction_body(provider, &request)?;
    let input = body
        .get_mut("input")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            PureError::ConfigError("Responses compaction input must be an array".into())
        })?;
    #[derive(serde::Serialize)]
    struct CompactionTrigger {
        r#type: &'static str,
    }
    input.push(
        serde_json::to_value(CompactionTrigger {
            r#type: "compaction_trigger",
        })
        .map_err(PureError::from)?,
    );
    append_beta_feature(
        &mut headers,
        provider.endpoint().http_headers.as_ref(),
        REMOTE_COMPACTION_V2_FEATURE,
    );
    let completion = CompletionRequest::builder()
        .instructions(request.instructions)
        .input(request.input)
        .tools(request.tools)
        .parallel_tool_calls(request.parallel_tool_calls)
        .reasoning(request.reasoning)
        .build();
    let response = provider
        .for_compaction(headers, body)
        .complete(completion, context)
        .await?;
    let accounting = response.accounting;
    let mut checkpoints = response
        .responses_context_items
        .into_iter()
        .filter(|item| item.value.get("type").and_then(Value::as_str) == Some("compaction"));
    let replacement = (|| -> Result<_> {
        let checkpoint = checkpoints.next().ok_or_else(|| {
            PureError::Protocol("remote compaction returned no checkpoint".into())
        })?;
        if checkpoints.next().is_some() {
            return Err(PureError::Protocol(
                "remote compaction returned conflicting checkpoints".into(),
            ));
        }
        parse_output_item(checkpoint.value)?.ok_or_else(|| {
            PureError::Protocol("remote compaction returned an invalid checkpoint".into())
        })
    })()
    .map_err(|source| CompletionFailure {
        source,
        accounting: Box::new(accounting.clone()),
    })?;
    Ok(ModelCompactionResponse {
        input: vec![replacement],
        accounting,
    })
}

fn build_compaction_body(
    provider: &InvocationRunner,
    request: &ModelCompactionRequest,
) -> Result<(Map<String, Value>, HashMap<String, String>)> {
    let model_info = provider.model().clone();
    let effective_capabilities = model_info
        .capabilities
        .clone()
        .with_native_custom_tools(provider.endpoint().uses_native_custom_tools());
    let custom_tools_native = provider.endpoint().uses_native_custom_tools()
        && effective_capabilities.supports_custom_tools()
        && effective_capabilities.supports_freeform_tools();
    let custom_tool_projection = if custom_tools_native {
        CustomToolProjection::Native
    } else {
        CustomToolProjection::ToFunction
    };
    let completion = CompletionRequest::builder()
        .instructions(request.instructions.clone())
        .input(request.input.clone())
        .tools(request.tools.clone())
        .parallel_tool_calls(request.parallel_tool_calls)
        .reasoning(request.reasoning.clone())
        .build()
        .provider_compatible(custom_tool_projection);
    completion.validate_against(&model_info.slug, &effective_capabilities)?;
    let OpenAiRequestBody::Responses(mut body) = OpenAiProtocol::responses().build_request(
        &completion,
        &model_info,
        request.prompt_cache_key.as_deref(),
    )?
    else {
        return Err(PureError::ConfigError(
            "OpenAI remote compaction requires the Responses protocol".to_string(),
        ));
    };
    for key in ["tool_choice", "store", "previous_response_id"] {
        body.remove(key);
    }
    Ok((body, model_info.binding.request.headers.clone()))
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

    fn compaction_request(mode: OpenAiCompactionMode) -> ModelCompactionRequest {
        ModelCompactionRequest {
            mode,
            instructions: "canonical instructions".to_string(),
            input: vec![pl_protocol::ModelContextItem::from(pl_protocol::Message {
                presentation: Default::default(),
                role: pl_protocol::MessageRole::User,
                content: pl_protocol::MessageContent::text("hello".to_string()),
                reasoning_content: None,
                tool_calls: None,
                tool_result: None,
                metadata: std::collections::HashMap::new(),
            })],
            tools: vec![pl_protocol::ToolSpec::function(
                "read_file",
                "Read a file",
                serde_json::json!({"type": "object", "properties": {}}),
            )],
            parallel_tool_calls: true,
            reasoning: Some(crate::completion::ReasoningConfig {
                effort: Some("medium".to_string()),
                summary: Some(crate::completion::ReasoningSummary::Enabled),
            }),
            prompt_cache_key: Some("cache-key".to_string()),
        }
    }

    #[tokio::test]
    async fn v2_compaction_uses_responses_trigger_feature_and_completed_usage() {
        use pretty_assertions::assert_eq;

        use crate::provider::ProviderConnectionMode;
        use crate::runtime::test_support::{openai_provider, serve_sse_once};

        let sse_body = concat!(
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ignored\"}]}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"encrypted-v2\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let (base_url, handle) = serve_sse_once(sse_body).await;
        let fixture = openai_provider(base_url, ProviderConnectionMode::Http);
        let mut endpoint = fixture.endpoint().clone();
        endpoint.service_capabilities.remote_compaction = true;
        let provider =
            crate::runtime::ModelRuntime::new(endpoint, fixture.model().clone()).unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let context = ModelInvocationContext::new(Default::default()).with_events(tx);

        let response = provider
            .compaction()
            .expect("declared native compaction")
            .complete(compaction_request(OpenAiCompactionMode::RemoteV2), context)
            .await
            .unwrap();
        let captured = handle.await.unwrap();

        assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
        assert_eq!(
            captured.headers["x-codex-beta-features"],
            "existing_feature,remote_compaction_v2"
        );
        assert_eq!(
            captured.body["input"].as_array().unwrap().last().unwrap(),
            &serde_json::json!({"type": "compaction_trigger"})
        );
        assert_eq!(response.input.len(), 1);
        assert_eq!(response.accounting.usage.totals().total_tokens, 15);
    }

    #[tokio::test]
    async fn v2_compaction_does_not_replay_after_stream_is_established() {
        use pretty_assertions::assert_eq;

        use crate::provider::ProviderConnectionMode;
        use crate::runtime::test_support::{openai_provider, serve_sse_once};

        let (base_url, handle) = serve_sse_once("data: [DONE]\n\n".to_string()).await;
        let fixture = openai_provider(base_url, ProviderConnectionMode::Http);
        let mut endpoint = fixture.endpoint().clone();
        endpoint.service_capabilities.remote_compaction = true;
        let provider =
            crate::runtime::ModelRuntime::new(endpoint, fixture.model().clone()).unwrap();
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let context = ModelInvocationContext::new(Default::default()).with_events(tx);

        let error = provider
            .compaction()
            .expect("declared native compaction")
            .complete(compaction_request(OpenAiCompactionMode::RemoteV2), context)
            .await
            .unwrap_err();
        let captured = handle.await.unwrap();

        assert!(error.is_transient_model_transport());
        assert_eq!(captured.request_line, "POST /responses HTTP/1.1");
    }
}
