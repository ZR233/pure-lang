use pl_model::{ModelInfo, ProviderEndpoint};

use super::*;
use crate::config::{
    ConfigPaths, ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole,
};
use crate::{ConfigStore, StudioHostKind, StudioMode, StudioRuntimeOptions, StudioTaskState};

fn test_config(base_url: String) -> StudioConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.transport = pl_model::ModelTransportProfile::responses_http();
    model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    let info = ProviderEndpoint::openai(Some(base_url));
    let provider = crate::ProviderConfig::from_explicit_models(info, vec![model]);
    let provider_id = ProviderId::new("local").unwrap();
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        effort: Some(ReasoningEffort::new("none")),
    };
    test_product_config(provider_id, provider, route)
}

fn test_product_config(
    provider_id: ProviderId,
    provider: crate::ProviderConfig,
    route: ModelRouteConfig,
) -> StudioConfig {
    let mut config = StudioConfig::default_config();
    config.models = crate::AgentModelConfig {
        providers: std::collections::BTreeMap::from([(provider_id, provider)]),
        routes: StudioRole::all()
            .into_iter()
            .map(|role| (role.id(), route.clone()))
            .collect(),
    };
    config
}

#[tokio::test]
async fn task_record_is_persisted_before_runtime_readiness_is_checked() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = root.path().join("workspace");
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(ConfigPaths::from_home(&home));
    config_store
        .save(&test_config("http://127.0.0.1:9".to_string()))
        .unwrap();
    let runtime = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(home),
        host: StudioHostKind::Test,
    })
    .await
    .unwrap();
    let project = runtime.open_project(&workspace).await.unwrap();
    let thread = runtime
        .create_thread(&project.id, "Persist before runtime")
        .await
        .unwrap();
    runtime
        .set_thread_mode(&thread.id, StudioMode::Task)
        .await
        .unwrap();

    let error = runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: thread.id.clone(),
            prompt: "deliver the requested feature".to_string(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await
        .unwrap_err();

    assert!(error.to_string().contains("runtime is not ready"));
    let task = runtime
        .thread_task_view(&thread.id)
        .await
        .unwrap()
        .expect("Task record must survive model/runtime readiness failure");
    assert!(matches!(task.state, StudioTaskState::Planning(_)));
    assert_eq!(task.revision, 0);
    assert_eq!(task.generation, 0);
}

mod deepseek_cache;
mod openai_cache;
