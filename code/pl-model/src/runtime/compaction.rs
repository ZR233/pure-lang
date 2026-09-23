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
    let model_observation = response.model_observation.clone();
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
        source: Box::new(source),
        accounting: Box::new(accounting.clone()),
        model_observation: model_observation.clone().map(Box::new),
        cancelled: false,
    })?;
    Ok(ModelCompactionResponse {
        input: vec![replacement],
        accounting,
        model_observation,
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
