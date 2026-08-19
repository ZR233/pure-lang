use pl_core::{
    AgentSession, ContextCompactionImplementation, CoreRuntimeProfile, ModelInfo, ProviderEndpoint,
    TurnBudget, TurnEngineBuilder, TurnRequest, TurnResultStatus, default_models,
    openai_default_model_slugs,
};
use pl_protocol::{MessageRole, ModelContextItem};

const OPENAI_LIVE_ENV_KEY: &str = "API_KEY_OPENAI";
const OPENAI_LIVE_BASE_URL_ENV_KEY: &str = "API_BASE_OPENAI";
const OPENAI_LIVE_MODEL_ENV_KEY: &str = "API_MODEL_OPENAI";

mod support;

fn live_api_key() -> Option<String> {
    match std::env::var(OPENAI_LIVE_ENV_KEY) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("{OPENAI_LIVE_ENV_KEY} is not set; skipping live context compaction test");
            None
        }
    }
}

fn live_base_url() -> Option<String> {
    std::env::var(OPENAI_LIVE_BASE_URL_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn live_model(default_model: &str) -> String {
    std::env::var(OPENAI_LIVE_MODEL_ENV_KEY)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_model.to_string())
}

fn compacting_model(model: &str) -> ModelInfo {
    let mut model_info = default_models()
        .into_iter()
        .find(|info| info.slug == model)
        .unwrap_or_else(|| ModelInfo::fallback(model));
    model_info.context_window = Some(256);
    model_info.max_context_window = Some(256);
    model_info.auto_compact_token_limit = Some(1);
    model_info.max_output_tokens = Some(256);
    model_info
}

#[tokio::test]
async fn openai_responses_compacts_context_live() {
    let Some(api_key) = live_api_key() else {
        return;
    };

    let mut info = ProviderEndpoint::openai(live_base_url());
    info.bearer_token = Some(api_key);
    let model = live_model(openai_default_model_slugs()[0]);
    let route = support::route("openai", info, compacting_model(&model), Some("medium"));
    let core = TurnEngineBuilder::from_route(&route)
        .unwrap()
        .with_runtime_profile(CoreRuntimeProfile::minimal())
        .build();

    let mut session = AgentSession::new();
    session.push_user_prompt(
        "旧上下文：项目代号 alpha，用户偏好回答要简短，并且最终回复只需要 ok。".to_string(),
    );
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(256);
    let mut recorder =
        pl_core::TraceRecorder::new("context-compaction-live".to_string(), event_tx, 0);
    let request = TurnRequest::new("请根据当前上下文只回答 ok，不要解释。".to_string())
        .with_budget(TurnBudget::new(180_000));

    let result = core
        .run_turn_with_trace(&mut session, request, &mut recorder, Default::default())
        .await
        .unwrap();

    assert_eq!(
        result.status,
        TurnResultStatus::Completed,
        "live OpenAI compaction turn failed: {:?}",
        result.error
    );
    assert_eq!(
        result.context_compactions.len(),
        1,
        "expected one forced compaction snapshot"
    );
    assert_eq!(
        result.context_compactions[0].implementation,
        ContextCompactionImplementation::RemoteV2
    );
    assert!(result.context_compactions[0].summary.is_none());
    let checkpoint_index = session
        .items()
        .iter()
        .position(ModelContextItem::is_compaction)
        .expect("remote v2 should install an encrypted checkpoint");
    assert!(matches!(
        session.items().get(checkpoint_index + 1),
        Some(ModelContextItem::Message { message }) if message.role == MessageRole::Assistant
    ));
    assert_eq!(checkpoint_index + 2, session.items().len());
    assert!(
        !result.content.trim().is_empty(),
        "final model output should not be empty"
    );
}
