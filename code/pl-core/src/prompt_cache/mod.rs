use std::collections::BTreeMap;

use pl_model::{EffectivePromptCachePolicy, ProviderInfo, ReasoningConfig, ToolSchema};
use pl_protocol::{
    Message, ModelContextSnapshot, PromptPrefixChangedReason, PureError, ThreadPromptSnapshot,
};

use crate::{AgentSession, canonical_json_hash};

/// 计算 prompt generation 所需的固定请求属性。
pub(crate) struct PromptCacheInput<'a> {
    pub scope: &'a str,
    pub provider: &'a ProviderInfo,
    pub model: &'a str,
    pub instructions: &'a str,
    pub prelude_messages: &'a [Message],
    pub working_context: Option<&'a ModelContextSnapshot>,
    pub fixed_prefix_section_hashes: BTreeMap<String, String>,
    pub tools: &'a [ToolSchema],
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
pub(crate) fn stable_tool_schemas(mut tools: Vec<ToolSchema>) -> Vec<ToolSchema> {
    for tool in &mut tools {
        if let ToolSchema::Function { input_schema, .. } = tool {
            canonicalize_json(input_schema);
        }
    }
    tools.sort_by(|left, right| {
        left.name().cmp(right.name()).then_with(|| {
            canonical_schema(left)
                .unwrap_or_default()
                .cmp(&canonical_schema(right).unwrap_or_default())
        })
    });
    tools
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

fn provider_hash(provider: &ProviderInfo) -> Result<String, PureError> {
    let value = serde_json::json!({
        "name": provider.name,
        "baseUrl": provider.base_url,
        "protocol": provider.protocol,
        "connectionMode": provider.connection_mode,
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
fn canonical_schema(schema: &ToolSchema) -> Result<String, serde_json::Error> {
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
mod tests {
    use pl_model::{ProviderInfo, ProviderWireProtocol, ReasoningSummary, ToolWirePolicy};
    use pl_protocol::MessageRole;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::context_section;

    fn input<'a>(
        scope: &'a str,
        provider: &'a ProviderInfo,
        model: &'a str,
        instructions: &'a str,
        tools: &'a [ToolSchema],
        compacted: bool,
    ) -> PromptCacheInput<'a> {
        PromptCacheInput {
            scope,
            provider,
            model,
            instructions,
            prelude_messages: &[],
            working_context: None,
            fixed_prefix_section_hashes: BTreeMap::new(),
            tools,
            tool_choice: "auto",
            parallel_tool_calls: false,
            reasoning: None,
            output_schema: None,
            service_tier: None,
            compacted,
            prompt_cache_policy: provider
                .effective_prompt_cache_policy(&pl_model::ModelInfo::fallback(model)),
            updated_at: 1,
        }
    }

    #[test]
    fn context_changes_append_without_incrementing_generation() {
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        session.upsert_pinned_context(context_section("todo", 1, "Todo", "first").unwrap());
        let first = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "deepseek-v4-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        session.push_assistant_response("done".to_string(), None);
        session.upsert_pinned_context(context_section("todo", 2, "Todo", "second").unwrap());
        let second = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "deepseek-v4-flash",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();

        assert_eq!(first.generation, second.generation);
        assert_eq!(
            second.prefix_changed_reason,
            PromptPrefixChangedReason::ContextAppended
        );
        assert_eq!(session.items().len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Assistant);
    }

    #[test]
    fn compaction_creates_a_new_generation() {
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        let first = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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
    fn tools_are_sorted_by_model_visible_name() {
        let tools = stable_tool_schemas(vec![
            ToolSchema::function("zeta", "", serde_json::json!({"b": 2, "a": 1})),
            ToolSchema::function("alpha", "", serde_json::json!({})),
        ]);

        assert_eq!(
            tools.iter().map(ToolSchema::name).collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
    }

    #[test]
    fn unsorted_tools_do_not_change_the_prompt_generation() {
        let provider = ProviderInfo::deepseek(None);
        let first_tools = vec![
            ToolSchema::function("zeta", "", serde_json::json!({"b": 2, "a": 1})),
            ToolSchema::function("alpha", "", serde_json::json!({})),
        ];
        let second_tools = vec![
            ToolSchema::function("alpha", "", serde_json::json!({})),
            ToolSchema::function("zeta", "", serde_json::json!({"a": 1, "b": 2})),
        ];
        let mut session = AgentSession::new();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                    "deepseek-v4-flash",
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
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        let first = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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
        renamed_provider.name = "DeepSeek mirror".to_string();
        let provider_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &renamed_provider,
                "deepseek-v4-flash",
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

        let changed_tools = vec![ToolSchema::function(
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
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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

        routed.protocol = ProviderWireProtocol::Responses;
        routed.tool_wire_policy = ToolWirePolicy::NativeCustomTools;
        let wire_changed = prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &routed,
                "deepseek-v4-flash",
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
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let output_schema = serde_json::json!({"type": "object"});
        let reasoning = ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        };
        let mut session = AgentSession::new();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
            "deepseek-v4-flash",
            "fixed",
            &tools,
            false,
        );
        tool_choice.tool_choice = "required";
        inputs.push(tool_choice);
        let mut parallel = input(
            "simple:executor",
            &provider,
            "deepseek-v4-flash",
            "fixed",
            &tools,
            false,
        );
        parallel.parallel_tool_calls = true;
        inputs.push(parallel);
        let mut with_reasoning = input(
            "simple:executor",
            &provider,
            "deepseek-v4-flash",
            "fixed",
            &tools,
            false,
        );
        with_reasoning.reasoning = Some(&reasoning);
        inputs.push(with_reasoning);
        let mut with_output_schema = input(
            "simple:executor",
            &provider,
            "deepseek-v4-flash",
            "fixed",
            &tools,
            false,
        );
        with_output_schema.output_schema = Some(&output_schema);
        inputs.push(with_output_schema);
        let mut with_service_tier = input(
            "simple:executor",
            &provider,
            "deepseek-v4-flash",
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
    fn every_fixed_instruction_layer_has_a_precise_change_reason() {
        let provider = ProviderInfo::deepseek(None);
        let tools = Vec::new();
        let sections = BTreeMap::from([
            ("base".to_string(), "v1".to_string()),
            ("globalDeveloper".to_string(), "v1".to_string()),
            ("globalUser".to_string(), "v1".to_string()),
            ("modeRole".to_string(), "v1".to_string()),
            ("skills".to_string(), "v1".to_string()),
            ("workspace".to_string(), "v1".to_string()),
        ]);

        for (section, expected) in [
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
            let mut session = AgentSession::new();
            let mut baseline = input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                "deepseek-v4-flash",
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
    fn openai_cache_key_is_stable_within_generation_and_rotates_at_boundary() {
        let provider = ProviderInfo::openai(None);
        let tools = Vec::new();
        let mut session = AgentSession::new();
        let prompt = prepare_prompt_context(
            &mut session,
            input(
                "simple:root",
                &provider,
                "gpt-5.6-sol",
                "fixed",
                &tools,
                false,
            ),
        )
        .unwrap()
        .unwrap();
        let same_generation = derive_prompt_cache_key("thread-1", &prompt).unwrap();
        let repeated = derive_prompt_cache_key("thread-1", &prompt).unwrap();
        let mut next_generation = prompt.clone();
        next_generation.generation += 1;
        let rotated = derive_prompt_cache_key("thread-1", &next_generation).unwrap();

        assert_eq!(same_generation, repeated);
        assert_ne!(same_generation, rotated);
        assert_ne!(
            same_generation,
            derive_prompt_cache_key("thread-2", &prompt).unwrap()
        );
    }

    #[test]
    fn recursive_schema_key_order_does_not_change_the_prompt_generation() {
        let provider = ProviderInfo::deepseek(None);
        let first_tools = stable_tool_schemas(vec![ToolSchema::function(
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
        let second_tools = stable_tool_schemas(vec![ToolSchema::function(
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

        let mut session = AgentSession::new();
        prepare_prompt_context(
            &mut session,
            input(
                "simple:executor",
                &provider,
                "deepseek-v4-flash",
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
                    "deepseek-v4-flash",
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
