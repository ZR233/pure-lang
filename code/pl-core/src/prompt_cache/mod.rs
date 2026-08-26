use std::collections::BTreeMap;

use pl_model::{EffectivePromptCachePolicy, ProviderEndpoint, ReasoningConfig, ToolSpec};
use pl_protocol::{
    Message, ModelContextSnapshot, PromptPrefixChangedReason, PureError, ThreadPromptSnapshot,
};

use crate::{AgentSession, canonical_json_hash};

/// 计算 prompt generation 所需的固定请求属性。
pub(crate) struct PromptCacheInput<'a> {
    pub scope: &'a str,
    pub provider: &'a ProviderEndpoint,
    pub model: &'a str,
    pub instructions: &'a str,
    pub prelude_messages: &'a [Message],
    pub working_context: Option<&'a ModelContextSnapshot>,
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

/// 以稳定名称和 canonical JSON 顺序冻结一次 Turn 的模型可见工具集合。
pub(crate) fn stable_tool_schemas(mut tools: Vec<ToolSpec>) -> Vec<ToolSpec> {
    for tool in &mut tools {
        canonicalize_tool_schema(tool);
    }
    sort_tool_schemas(&mut tools);
    tools
}

fn canonicalize_tool_schema(tool: &mut ToolSpec) {
    match tool {
        ToolSpec::Function {
            input_schema,
            output_schema,
            ..
        } => {
            canonicalize_json(input_schema);
            if let Some(output_schema) = output_schema {
                canonicalize_json(output_schema);
            }
        }
        ToolSpec::Custom { output_schema, .. } => {
            if let Some(output_schema) = output_schema {
                canonicalize_json(output_schema);
            }
        }
        ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => {}
    }
}

fn sort_tool_schemas(tools: &mut [ToolSpec]) {
    tools.sort_by(|left, right| {
        left.name().cmp(right.name()).then_with(|| {
            canonical_schema(left)
                .unwrap_or_default()
                .cmp(&canonical_schema(right).unwrap_or_default())
        })
    });
}

/// 更新请求缓存诊断所需的 working-context 快照。
pub(crate) fn prepare_prompt_context(
    session: &mut AgentSession,
    input: PromptCacheInput<'_>,
) -> Result<Option<ThreadPromptSnapshot>, PureError> {
    let current_context;
    let context = match input.working_context {
        Some(context) => context,
        None => {
            current_context = session.working_context_snapshot();
            &current_context
        }
    };
    let context_hash = canonical_json_hash(&serde_json::to_value(context)?);
    let stable_tools = stable_tool_schemas(input.tools.to_vec());
    let tool_schema_hash = canonical_json_hash(&serde_json::to_value(&stable_tools)?);
    let provider_hash = provider_hash(input.provider)?;
    let request_properties_hash = request_properties_hash(&input, &provider_hash)?;
    let fixed_prefix_hash = fixed_prefix_hash(&input, &provider_hash)?;
    let previous_metadata = session.prompt_metadata();
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
    let mut prompt_metadata = previous_metadata.clone();
    prompt_metadata.active_scope = input.scope.to_string();
    prompt_metadata
        .slots
        .insert(input.scope.to_string(), prompt.clone());
    session.replace_prompt_metadata(prompt_metadata);
    Ok(Some(prompt))
}

/// 为 OpenAI Responses 派生不暴露 Thread identity 的 generation-scoped cache key。
pub(crate) fn derive_prompt_cache_key(
    namespace: &str,
    prompt: &ThreadPromptSnapshot,
) -> Result<String, PureError> {
    let value = serde_json::json!({
        "namespace": namespace,
        "scope": prompt.scope,
        "generation": prompt.generation,
    });
    Ok(format!("pl:{}", canonical_json_hash(&value)))
}

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
    if previous.provider != input.provider.name || previous.provider_hash != hashes.provider {
        return Some(PromptPrefixChangedReason::ProviderChanged);
    }
    if previous.model != input.model {
        return Some(PromptPrefixChangedReason::ModelChanged);
    }
    if previous.tool_schema_hash != hashes.tool_schema {
        return Some(PromptPrefixChangedReason::ToolSchemaChanged);
    }
    if previous.fixed_prefix_section_hashes != input.fixed_prefix_section_hashes {
        return Some(changed_instruction_section(
            &previous.fixed_prefix_section_hashes,
            &input.fixed_prefix_section_hashes,
        ));
    }
    if previous.request_properties_hash != hashes.request_properties {
        return Some(PromptPrefixChangedReason::RequestPropertiesChanged);
    }
    if previous.fixed_prefix_hash != hashes.fixed_prefix {
        return Some(PromptPrefixChangedReason::FixedPrefixChanged);
    }
    (previous.context_hash != hashes.context).then_some(PromptPrefixChangedReason::ContextAppended)
}

fn changed_instruction_section(
    previous: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> PromptPrefixChangedReason {
    for (id, reason) in [
        ("base", PromptPrefixChangedReason::BaseInstructionsChanged),
        (
            "globalDeveloper",
            PromptPrefixChangedReason::GlobalInstructionsChanged,
        ),
        (
            "globalUser",
            PromptPrefixChangedReason::GlobalInstructionsChanged,
        ),
        ("modeRole", PromptPrefixChangedReason::ModeRoleChanged),
        ("skills", PromptPrefixChangedReason::SkillCatalogChanged),
        (
            "workspace",
            PromptPrefixChangedReason::WorkspaceInstructionsChanged,
        ),
    ] {
        if previous.get(id) != current.get(id) {
            return reason;
        }
    }
    PromptPrefixChangedReason::FixedPrefixChanged
}

fn provider_hash(provider: &ProviderEndpoint) -> Result<String, PureError> {
    let value = serde_json::json!({
        "name": provider.name,
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
        "scope": input.scope,
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
        "scope": input.scope,
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
fn canonical_schema(schema: &ToolSpec) -> Result<String, serde_json::Error> {
    serde_json::to_value(schema).map(|value| canonical_json_hash(&value))
}

fn canonicalize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            let mut sorted = object
                .iter_mut()
                .map(|(key, value)| {
                    canonicalize_json(value);
                    (key.clone(), value.clone())
                })
                .collect::<BTreeMap<_, _>>();
            object.clear();
            object.extend(
                sorted
                    .iter_mut()
                    .map(|(key, value)| (key.clone(), value.take())),
            );
        }
        serde_json::Value::Array(items) => {
            for item in items {
                canonicalize_json(item);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

#[cfg(test)]
mod unit_tests;
