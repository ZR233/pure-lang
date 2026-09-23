use crate::completion::stable_tool_schemas;
use std::collections::BTreeMap;

use pl_protocol::{
    Message, PromptPrefixChangedReason, PureError, ThreadPromptMetadata, ThreadPromptSnapshot,
};

use crate::completion::ReasoningConfig;
use crate::provider::{EffectivePromptCachePolicy, ProviderEndpoint};
use pl_protocol::ToolSpec;
use sha2::{Digest, Sha256};

/// 计算 prompt generation 所需的固定请求属性。
#[derive(Debug)]
pub struct PromptCacheInput<'a> {
    pub scope: &'a str,
    pub provider: &'a ProviderEndpoint,
    pub model: &'a str,
    pub instructions: &'a str,
    pub prelude_messages: &'a [Message],
    pub context_hash: &'a str,
    pub fixed_prefix_section_hashes: BTreeMap<String, String>,
    /// 实际发送的 eager 工具 schema（`WirePrefixFingerprint` 语义）。
    pub tools: &'a [ToolSpec],
    pub tool_choice: &'a str,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<&'a ReasoningConfig>,
    pub output_schema: Option<&'a serde_json::Value>,
    pub service_tier: Option<&'a str>,
    pub compacted: bool,
    pub prompt_cache_policy: EffectivePromptCachePolicy,
    pub updated_at: i64,
}

struct PromptHashes<'a> {
    context: &'a str,
    provider: &'a str,
    fixed_prefix: &'a str,
    request_properties: &'a str,
    tool_schema: &'a str,
}

/// Computes a diagnostic update from frozen model input without modifying caller state.
///
/// # Errors
/// Returns an encoding failure before producing any replacement metadata.
pub fn prompt_diagnostics(
    previous_metadata: &ThreadPromptMetadata,
    input: PromptCacheInput<'_>,
) -> Result<Option<ThreadPromptSnapshot>, PureError> {
    let context_hash = input.context_hash.to_owned();
    let stable_tools = stable_tool_schemas(input.tools.to_vec());
    let tool_schema_hash = canonical_json_hash(&serde_json::to_value(&stable_tools)?);
    let provider_hash = provider_hash(input.provider)?;
    let request_properties_hash = request_properties_hash(&input, &provider_hash)?;
    let fixed_prefix_hash = fixed_prefix_hash(&input, &provider_hash)?;
    let previous_prompt = previous_metadata.slots.get(input.scope);
    let previous_active_scope = (!previous_metadata.active_scope.is_empty())
        .then_some(previous_metadata.active_scope.as_str());
    let previous_generation = previous_metadata
        .slots
        .values()
        .map(|snapshot| snapshot.generation)
        .max()
        .unwrap_or_default();

    let reason = changed_reason(
        previous_prompt,
        previous_active_scope,
        PromptHashes {
            context: &context_hash,
            provider: &provider_hash,
            fixed_prefix: &fixed_prefix_hash,
            request_properties: &request_properties_hash,
            tool_schema: &tool_schema_hash,
        },
        &input,
    );
    let Some(reason) = reason else {
        return Ok(None);
    };
    let generation = if matches!(
        reason,
        PromptPrefixChangedReason::ContextAppended | PromptPrefixChangedReason::ContextRecovered
    ) {
        previous_prompt.map_or(previous_generation.max(1), |previous| previous.generation)
    } else {
        previous_generation.saturating_add(1).max(1)
    };
    let prompt = ThreadPromptSnapshot {
        scope: input.scope.to_string(),
        generation,
        provider: input.provider.name.clone(),
        provider_hash,
        model: input.model.to_string(),
        fixed_prefix_hash,
        fixed_prefix_section_hashes: input.fixed_prefix_section_hashes.clone(),
        request_properties_hash,
        tool_schema_hash,
        context_hash,
        prompt_cache_policy: input.prompt_cache_policy.label().to_string(),
        prefix_changed_reason: reason,
        updated_at: input.updated_at,
    };
    Ok(Some(prompt))
}

/// Classifies observed changes for diagnostics; this does not choose a provider cache key.
fn changed_reason(
    previous: Option<&ThreadPromptSnapshot>,
    previous_active_scope: Option<&str>,
    hashes: PromptHashes<'_>,
    input: &PromptCacheInput<'_>,
) -> Option<PromptPrefixChangedReason> {
    if previous_active_scope.is_none() {
        return Some(PromptPrefixChangedReason::Initial);
    }
    if input.compacted {
        return Some(PromptPrefixChangedReason::ContextCompacted);
    }
    if previous_active_scope != Some(input.scope) {
        return Some(PromptPrefixChangedReason::PromptScopeChanged);
    }
    let Some(previous) = previous else {
        return Some(PromptPrefixChangedReason::Initial);
    };
    if previous.prefix_changed_reason == PromptPrefixChangedReason::ContextRecovered {
        return Some(PromptPrefixChangedReason::ContextRecovered);
    }
    if previous.provider_hash != hashes.provider {
        return Some(PromptPrefixChangedReason::ProviderChanged);
    }
    if previous.model != input.model {
        return Some(PromptPrefixChangedReason::ModelChanged);
    }
    if previous.tool_schema_hash != hashes.tool_schema {
        return Some(PromptPrefixChangedReason::ToolSchemaChanged);
    }
    if previous.fixed_prefix_section_hashes != input.fixed_prefix_section_hashes {
        return Some(PromptPrefixChangedReason::FixedPrefixChanged);
    }
    if previous.request_properties_hash != hashes.request_properties {
        return Some(PromptPrefixChangedReason::RequestPropertiesChanged);
    }
    if previous.fixed_prefix_hash != hashes.fixed_prefix {
        return Some(PromptPrefixChangedReason::FixedPrefixChanged);
    }
    (previous.context_hash != hashes.context).then_some(PromptPrefixChangedReason::ContextAppended)
}

fn provider_hash(provider: &ProviderEndpoint) -> Result<String, PureError> {
    let value = serde_json::json!({
        "adapter": provider.adapter,
        "baseUrl": provider.base_url,
        "toolWirePolicy": provider.tool_wire_policy,
        "applyPatchToolType": provider.apply_patch_tool_type,
        "serviceCapabilities": provider.service_capabilities,
    });
    Ok(canonical_json_hash(&value))
}

fn fixed_prefix_hash(
    input: &PromptCacheInput<'_>,
    provider_hash: &str,
) -> Result<String, PureError> {
    let value = serde_json::json!({
        "providerHash": provider_hash,
        "model": input.model,
        "instructions": input.instructions,
        "preludeMessages": input.prelude_messages,
        "toolChoice": input.tool_choice,
        "parallelToolCalls": input.parallel_tool_calls,
        "reasoning": input.reasoning,
        "outputSchema": input.output_schema,
        "serviceTier": input.service_tier,
        "store": false,
    });
    Ok(canonical_json_hash(&value))
}

fn request_properties_hash(
    input: &PromptCacheInput<'_>,
    provider_hash: &str,
) -> Result<String, PureError> {
    let value = serde_json::json!({
        "providerHash": provider_hash,
        "model": input.model,
        "toolChoice": input.tool_choice,
        "parallelToolCalls": input.parallel_tool_calls,
        "reasoning": input.reasoning,
        "outputSchema": input.output_schema,
        "serviceTier": input.service_tier,
        "store": false,
        "promptCachePolicy": input.prompt_cache_policy,
    });
    Ok(canonical_json_hash(&value))
}
fn canonical_json_hash(value: &serde_json::Value) -> String {
    let mut value = value.clone();
    crate::completion::canonicalize_json(&mut value);
    let digest = Sha256::digest(value.to_string().as_bytes());
    format!("sha256:{digest:x}")
}
