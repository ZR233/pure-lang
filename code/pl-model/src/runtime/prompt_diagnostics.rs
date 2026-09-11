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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::ReasoningSummary;
    use crate::provider::{ProviderEndpoint, ToolWirePolicy};

    fn prepare_prompt_context(
        metadata: &mut ThreadPromptMetadata,
        input: PromptCacheInput<'_>,
    ) -> Result<Option<ThreadPromptSnapshot>, PureError> {
        let update = prompt_diagnostics(metadata, input)?;
        if let Some(prompt) = &update {
            metadata.active_scope = prompt.scope.clone();
            metadata.slots.insert(prompt.scope.clone(), prompt.clone());
        }
        Ok(update)
    }

    fn input<'a>(
        scope: &'a str,
        provider: &'a ProviderEndpoint,
        model: &'a str,
        instructions: &'a str,
        tools: &'a [ToolSpec],
        compacted: bool,
    ) -> PromptCacheInput<'a> {
        PromptCacheInput {
            scope,
            provider,
            model,
            instructions,
            prelude_messages: &[],
            context_hash: "context",
            fixed_prefix_section_hashes: BTreeMap::new(),
            tools,
            tool_choice: "auto",
            parallel_tool_calls: false,
            reasoning: None,
            output_schema: None,
            service_tier: None,
            compacted,
            prompt_cache_policy: provider
                .effective_prompt_cache_policy(&crate::model::ModelInfo::compatible(model)),
            updated_at: 1,
        }
    }

    #[test]
    fn context_changes_append_without_incrementing_generation() {
        let provider = ProviderEndpoint::deepseek(None);
        let mut metadata = ThreadPromptMetadata::default();
        let first = prepare_prompt_context(
            &mut metadata,
            input("scope", &provider, "model", "fixed", &[], false),
        )
        .unwrap()
        .unwrap();
        let mut changed = input("scope", &provider, "model", "fixed", &[], false);
        changed.context_hash = "appended-context";
        let second = prepare_prompt_context(&mut metadata, changed)
            .unwrap()
            .unwrap();
        assert_eq!(first.generation, second.generation);
        assert_eq!(
            second.prefix_changed_reason,
            PromptPrefixChangedReason::ContextAppended
        );
    }

    #[test]
    fn provider_display_rename_does_not_change_the_cache_key() {
        let mut provider = ProviderEndpoint::deepseek(None);
        let mut metadata = ThreadPromptMetadata::default();
        let first = prepare_prompt_context(
            &mut metadata,
            input("scope", &provider, "model", "fixed", &[], false),
        )
        .unwrap()
        .unwrap();
        provider.name = "Renamed display label".into();
        assert!(
            prepare_prompt_context(
                &mut metadata,
                input("scope", &provider, "model", "fixed", &[], false,)
            )
            .unwrap()
            .is_none()
        );
        let mut appended = input("scope", &provider, "model", "fixed", &[], false);
        appended.context_hash = "next context";
        let second = prepare_prompt_context(&mut metadata, appended)
            .unwrap()
            .unwrap();
        assert_eq!(
            super::super::derive_prompt_cache_key("account", &first),
            super::super::derive_prompt_cache_key("account", &second)
        );
    }

    #[test]
    fn compaction_creates_a_new_generation() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let mut session = ThreadPromptMetadata::default();
        let first = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        let second = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                true,
            ),
        )
        .unwrap()
        .unwrap();

        assert_eq!(second.generation, first.generation + 1);
        assert_eq!(
            second.prefix_changed_reason,
            PromptPrefixChangedReason::ContextCompacted
        );
    }

    #[test]
    fn unsorted_tools_do_not_change_the_prompt_generation() {
        let provider = ProviderEndpoint::deepseek(None);
        let first_tools = vec![
            ToolSpec::function("zeta", "", serde_json::json!({"b": 2, "a": 1})),
            ToolSpec::function("alpha", "", serde_json::json!({})),
        ];
        let second_tools = vec![
            ToolSpec::function("alpha", "", serde_json::json!({})),
            ToolSpec::function("zeta", "", serde_json::json!({"a": 1, "b": 2})),
        ];
        let mut session = ThreadPromptMetadata::default();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &first_tools,
                false,
            ),
        )
        .unwrap();

        assert!(
            prepare_prompt_context(
                &mut session,
                input(
                    "simple:executor",
                    &provider,
                    "deepseek-flash",
                    "fixed",
                    &second_tools,
                    false,
                ),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn scope_switches_are_explicit_global_generation_boundaries() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let mut session = ThreadPromptMetadata::default();
        let first = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        let task = prepare_prompt_context(
            &mut session,
            input(
                "task:planner",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        let simple = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            first.prefix_changed_reason,
            PromptPrefixChangedReason::Initial
        );
        assert_eq!(
            task.prefix_changed_reason,
            PromptPrefixChangedReason::PromptScopeChanged
        );
        assert_eq!(
            simple.prefix_changed_reason,
            PromptPrefixChangedReason::PromptScopeChanged
        );
        assert_eq!(task.generation, first.generation + 1);
        assert_eq!(simple.generation, task.generation + 1);
    }

    #[test]
    fn fixed_provider_model_and_tool_changes_have_precise_reasons() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let mut session = ThreadPromptMetadata::default();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap();

        let fixed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "changed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            fixed.prefix_changed_reason,
            PromptPrefixChangedReason::FixedPrefixChanged
        );

        let mut renamed_provider = provider.clone();
        renamed_provider.base_url = "https://deepseek.example/v1".to_string();
        let provider_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &renamed_provider,
                "deepseek-flash",
                "changed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            provider_changed.prefix_changed_reason,
            PromptPrefixChangedReason::ProviderChanged
        );

        let model_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &renamed_provider,
                "deepseek-v4-pro",
                "changed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            model_changed.prefix_changed_reason,
            PromptPrefixChangedReason::ModelChanged
        );

        let changed_tools = vec![ToolSpec::function(
            "lookup",
            "lookup",
            serde_json::json!({"type": "object"}),
        )];
        let tools_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &renamed_provider,
                "deepseek-v4-pro",
                "changed",
                &changed_tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            tools_changed.prefix_changed_reason,
            PromptPrefixChangedReason::ToolSchemaChanged
        );
    }

    #[test]
    fn provider_route_and_wire_policy_changes_have_provider_reason() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let mut session = ThreadPromptMetadata::default();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap();

        let mut routed = provider.clone();
        routed.base_url = "https://deepseek.example/v1".to_string();
        let route_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &routed,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            route_changed.prefix_changed_reason,
            PromptPrefixChangedReason::ProviderChanged
        );

        routed.tool_wire_policy = ToolWirePolicy::NativeCustomTools;
        let wire_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &routed,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            wire_changed.prefix_changed_reason,
            PromptPrefixChangedReason::ProviderChanged
        );
    }

    #[test]
    fn every_fixed_request_attribute_changes_the_generation() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let output_schema = serde_json::json!({"type": "object"});
        let reasoning = ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        };
        let mut session = ThreadPromptMetadata::default();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap();

        let mut inputs = Vec::new();
        let mut tool_choice = input(
            "simple:executor",
            &provider,
            "deepseek-flash",
            "fixed",
            &tools,
            false,
        );
        tool_choice.tool_choice = "required";
        inputs.push(tool_choice);
        let mut parallel = input(
            "simple:executor",
            &provider,
            "deepseek-flash",
            "fixed",
            &tools,
            false,
        );
        parallel.parallel_tool_calls = true;
        inputs.push(parallel);
        let mut with_reasoning = input(
            "simple:executor",
            &provider,
            "deepseek-flash",
            "fixed",
            &tools,
            false,
        );
        with_reasoning.reasoning = Some(&reasoning);
        inputs.push(with_reasoning);
        let mut with_output_schema = input(
            "simple:executor",
            &provider,
            "deepseek-flash",
            "fixed",
            &tools,
            false,
        );
        with_output_schema.output_schema = Some(&output_schema);
        inputs.push(with_output_schema);
        let mut with_service_tier = input(
            "simple:executor",
            &provider,
            "deepseek-flash",
            "fixed",
            &tools,
            false,
        );
        with_service_tier.service_tier = Some("priority");
        inputs.push(with_service_tier);

        let mut previous_generation = 1;
        for changed in inputs {
            let snapshot = prepare_prompt_context(&mut session, changed)
                .unwrap()
                .unwrap();
            assert_eq!(
                snapshot.prefix_changed_reason,
                PromptPrefixChangedReason::RequestPropertiesChanged
            );
            assert!(snapshot.generation > previous_generation);
            previous_generation = snapshot.generation;
        }
    }

    #[test]
    fn opaque_instruction_sections_report_generic_prefix_changes() {
        let provider = ProviderEndpoint::deepseek(None);
        let tools = Vec::new();
        let sections = BTreeMap::from([
            ("plugin.alpha".to_string(), "v1".to_string()),
            ("unknown.beta".to_string(), "v1".to_string()),
        ]);
        for section in ["plugin.alpha", "unknown.beta"] {
            let expected = PromptPrefixChangedReason::FixedPrefixChanged;
            let mut session = ThreadPromptMetadata::default();
            let mut baseline = input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            );
            baseline.fixed_prefix_section_hashes = sections.clone();
            let first = prepare_prompt_context(&mut session, baseline)
                .unwrap()
                .unwrap();
            let mut changed_sections = sections.clone();
            changed_sections.insert(section.to_string(), "v2".to_string());
            let mut changed = input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &tools,
                false,
            );
            changed.fixed_prefix_section_hashes = changed_sections;

            let snapshot = prepare_prompt_context(&mut session, changed)
                .unwrap()
                .unwrap();

            assert_eq!(snapshot.prefix_changed_reason, expected, "{section}");
            assert_eq!(snapshot.generation, first.generation + 1, "{section}");
        }
    }

    #[test]
    fn recursive_schema_key_order_does_not_change_the_prompt_generation() {
        let provider = ProviderEndpoint::deepseek(None);
        let first_tools = stable_tool_schemas(vec![ToolSpec::function(
            "lookup",
            "lookup",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "query"},
                    "limit": {"maximum": 10, "type": "integer"}
                }
            }),
        )]);
        let second_tools = stable_tool_schemas(vec![ToolSpec::function(
            "lookup",
            "lookup",
            serde_json::json!({
                "properties": {
                    "limit": {"type": "integer", "maximum": 10},
                    "query": {"description": "query", "type": "string"}
                },
                "type": "object"
            }),
        )]);
        assert_eq!(
            serde_json::to_vec(&first_tools).unwrap(),
            serde_json::to_vec(&second_tools).unwrap()
        );

        let mut session = ThreadPromptMetadata::default();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-flash",
                "fixed",
                &first_tools,
                false,
            ),
        )
        .unwrap();
        assert!(
            prepare_prompt_context(
                &mut session,
                input(
                    "simple:executor",
                    &provider,
                    "deepseek-flash",
                    "fixed",
                    &second_tools,
                    false,
                ),
            )
            .unwrap()
            .is_none()
        );
    }
}
