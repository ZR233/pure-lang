use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_runtime_snapshot_accumulates_usage_and_cost() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();

    store
        .upsert_agent_snapshot(AgentSnapshotRecord {
            id: "agent-1".to_string(),
            session_id: session.id.clone(),
            path: "/root/research".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            task: "research".to_string(),
            status: AgentStatus::Completed,
            summary: Some("done".to_string()),
            depth: 1,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            runtime_usage: None,
            updated_at: 5,
        })
        .await
        .unwrap();

    let root_usage = TokenUsageSnapshot {
        prompt_tokens: 100_000,
        completion_tokens: 10_000,
        total_tokens: 110_000,
        cached_prompt_tokens: 40_000,
    };
    let root_delta = AgentRuntimeDelta {
        inference_id: "root-1".to_string(),
        agent_id: "agent-root".to_string(),
        path: "/root".to_string(),
        parent_path: None,
        role: "root".to_string(),
        model: "priced-model".to_string(),
        context_window: Some(1_000_000),
        usage: root_usage.clone(),
        estimated_costs: vec![RuntimeCostAmount {
            currency: "CNY".to_string(),
            amount: 0.0808,
        }],
        has_unpriced_usage: false,
        updated_at: 10,
    };
    let second_root_delta = AgentRuntimeDelta {
        inference_id: "root-2".to_string(),
        updated_at: 20,
        ..root_delta.clone()
    };
    let subagent_delta = AgentRuntimeDelta {
        inference_id: "agent-1-inference".to_string(),
        agent_id: "agent-1".to_string(),
        path: "/root/research".to_string(),
        parent_path: Some("/root".to_string()),
        role: "executor".to_string(),
        model: "usd-model".to_string(),
        context_window: Some(400_000),
        usage: TokenUsageSnapshot {
            prompt_tokens: 50_000,
            completion_tokens: 5_000,
            total_tokens: 55_000,
            cached_prompt_tokens: 0,
        },
        estimated_costs: vec![RuntimeCostAmount {
            currency: "USD".to_string(),
            amount: 0.06,
        }],
        has_unpriced_usage: false,
        updated_at: 30,
    };
    let unpriced_delta = AgentRuntimeDelta {
        inference_id: "agent-1-unpriced".to_string(),
        agent_id: "agent-1".to_string(),
        path: "/root/research".to_string(),
        parent_path: Some("/root".to_string()),
        role: "executor".to_string(),
        model: "unpriced-model".to_string(),
        context_window: Some(400_000),
        usage: TokenUsageSnapshot {
            prompt_tokens: 10_000,
            completion_tokens: 1_000,
            total_tokens: 11_000,
            cached_prompt_tokens: 0,
        },
        estimated_costs: Vec::new(),
        has_unpriced_usage: true,
        updated_at: 40,
    };

    assert!(
        store
            .record_agent_runtime_delta(&session.id, &root_delta)
            .await
            .unwrap()
    );
    assert!(
        !store
            .record_agent_runtime_delta(&session.id, &root_delta)
            .await
            .unwrap()
    );
    assert!(
        store
            .record_agent_runtime_delta(&session.id, &second_root_delta)
            .await
            .unwrap()
    );
    assert!(
        store
            .record_agent_runtime_delta(&session.id, &subagent_delta)
            .await
            .unwrap()
    );
    assert!(
        store
            .record_agent_runtime_delta(&session.id, &unpriced_delta)
            .await
            .unwrap()
    );

    let runtime = store
        .load_session_runtime(&session.id)
        .await
        .unwrap()
        .unwrap();
    let agents = store.list_agents(&session.id).await.unwrap();

    assert_eq!(runtime.model, "unpriced-model");
    assert_eq!(runtime.context_window, Some(400_000));
    assert_eq!(runtime.latest_context_tokens, 10_000);
    assert_eq!(runtime.prompt_tokens, 260_000);
    assert_eq!(runtime.completion_tokens, 26_000);
    assert_eq!(runtime.cached_prompt_tokens, 80_000);
    assert_eq!(runtime.total_tokens, 286_000);
    assert_eq!(runtime.currency, None);
    assert_eq!(runtime.estimated_cost, None);
    assert_eq!(
        runtime
            .estimated_costs
            .iter()
            .map(|cost| cost.currency.as_str())
            .collect::<Vec<_>>(),
        vec!["CNY", "USD"],
    );
    assert!(
        runtime.estimated_costs[0].amount.is_finite()
            && (runtime.estimated_costs[0].amount - 0.1616).abs() < 0.000_001
    );
    assert!(
        runtime.estimated_costs[1].amount.is_finite()
            && (runtime.estimated_costs[1].amount - 0.06).abs() < 0.000_001
    );
    assert!(runtime.has_unpriced_usage);
    assert_eq!(agents.len(), 1);
    assert_eq!(
        agents[0].runtime_usage.as_ref().map(|usage| (
            usage.model.as_str(),
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
            usage.has_unpriced_usage,
        )),
        Some(("unpriced-model", 60_000, 6_000, 66_000, true)),
    );
}

#[tokio::test]
async fn upsert_session_runtime_updates_context_after_existing_root_snapshot() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/runtime").await.unwrap();
    let session = store
        .create_session(&project.id, "Runtime", StudioMode::Simple)
        .await
        .unwrap();
    let model = test_model_info("priced-model", Some(1_000_000));

    let first = turn_result_with_usage("priced-model", 10_000, 1_000, 1);
    store
        .upsert_session_runtime(&session.id, &first, Some(&model))
        .await
        .unwrap();
    let second = turn_result_with_usage("priced-model", 22_000, 2_000, 2);
    store
        .upsert_session_runtime(&session.id, &second, Some(&model))
        .await
        .unwrap();

    let runtime = store
        .load_session_runtime(&session.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(runtime.latest_context_tokens, 22_000);
    assert_eq!(runtime.prompt_tokens, 32_000);
    assert_eq!(runtime.completion_tokens, 3_000);
    assert_eq!(runtime.total_tokens, 35_000);
    assert_eq!(runtime.context_window, Some(1_000_000));
}

#[tokio::test]
async fn turn_runtime_upsert_does_not_double_count_recorded_inference_usage() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/runtime-dedupe")
        .await
        .unwrap();
    let session = store
        .create_session(&project.id, "Runtime dedupe", StudioMode::Simple)
        .await
        .unwrap();
    let model = test_model_info("priced-model", Some(1_000_000));
    let delta = AgentRuntimeDelta {
        inference_id: "turn-runtime-dedupe-inf-0".to_string(),
        agent_id: "agent-root".to_string(),
        path: "/root".to_string(),
        parent_path: None,
        role: "root".to_string(),
        model: "priced-model".to_string(),
        context_window: Some(1_000_000),
        usage: TokenUsageSnapshot {
            prompt_tokens: 11_000,
            completion_tokens: 1_000,
            total_tokens: 12_000,
            cached_prompt_tokens: 0,
        },
        estimated_costs: Vec::new(),
        has_unpriced_usage: true,
        updated_at: 10,
    };
    store
        .record_agent_runtime_delta(&session.id, &delta)
        .await
        .unwrap();
    let result = turn_result_with_usage("priced-model", 11_000, 1_000, 1);

    store
        .upsert_session_runtime_for_turn(&session.id, "turn-runtime-dedupe", &result, Some(&model))
        .await
        .unwrap();

    let runtime = store
        .load_session_runtime(&session.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(runtime.latest_context_tokens, 11_000);
    assert_eq!(runtime.prompt_tokens, 11_000);
    assert_eq!(runtime.completion_tokens, 1_000);
    assert_eq!(runtime.total_tokens, 12_000);
}

fn turn_result_with_usage(
    model: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    session_message_count: usize,
) -> TurnResult {
    TurnResult {
        content: String::new(),
        reasoning_content: None,
        model: model.to_string(),
        usage: TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cached_prompt_tokens: 0,
            reasoning_tokens: 0,
        },
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        status: TurnResultStatus::Completed,
        abort_reason: None,
        error: None,
        budget_limit_kind: None,
        budget_usage: None,
        trace_events: Vec::new(),
    }
}

fn test_model_info(slug: &str, context_window: Option<u64>) -> ModelInfo {
    ModelInfo {
        slug: slug.to_string(),
        display_name: slug.to_string(),
        description: None,
        context_window,
        max_context_window: context_window,
        auto_compact_token_limit: None,
        default_temperature: None,
        max_output_tokens: None,
        currency: Some("CNY".to_string()),
        input_price_per_mtok: Some(1.0),
        output_price_per_mtok: Some(2.0),
        cache_read_price_per_mtok: Some(0.5),
        parameters: Vec::new(),
        capabilities: ModelCapabilities::default(),
        request_profile: ModelRequestProfile::default(),
        truncation_policy: TruncationPolicy::default(),
        base_instructions: String::new(),
        used_fallback: false,
    }
}
