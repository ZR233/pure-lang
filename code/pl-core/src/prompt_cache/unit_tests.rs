//! prompt_cache 单元测试。

use pl_model::{ProviderEndpoint, ReasoningSummary, ToolWirePolicy};
use pl_protocol::MessageRole;
use pretty_assertions::assert_eq;

use super::*;
use crate::context_section;

fn input<'a>(
    scope: &'a str,
    provider: &'a ProviderEndpoint,
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
        tool_catalog_hash: None,
        registry_revision: None,
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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
    let provider = ProviderEndpoint::openai(None);
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
    let provider = ProviderEndpoint::deepseek(None);
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

#[test]
fn tool_schemas_are_canonicalized_and_sorted_across_key_orders() {
    let first = stable_tool_schemas(vec![
        ToolSchema::function(
            "git_status",
            "status",
            serde_json::json!({"type": "object", "properties": {}}),
        ),
        ToolSchema::function(
            "git_diff",
            "diff",
            serde_json::json!({
                "type": "object",
                "properties": {"path": {"type": "string", "description": "path"}}
            }),
        ),
    ]);
    let second = stable_tool_schemas(vec![
        ToolSchema::function(
            "git_diff",
            "diff",
            serde_json::json!({
                "properties": {"path": {"description": "path", "type": "string"}},
                "type": "object"
            }),
        ),
        ToolSchema::function(
            "git_status",
            "status",
            serde_json::json!({"properties": {}, "type": "object"}),
        ),
    ]);

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
}
