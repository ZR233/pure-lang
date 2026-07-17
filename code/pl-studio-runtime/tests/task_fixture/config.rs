use std::collections::BTreeMap;

use pl_studio_runtime::{
    AgentModelConfig, AgentRoleId, ModelInfo, ModelParameter, ModelRouteConfig, PermissionMode,
    ProviderConfig, ProviderId, ReasoningEffort, StudioConfig,
};

pub(super) fn task_test_config(base_url: String) -> StudioConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    model.parameters = vec![ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: BTreeMap::new(),
    }];

    let mut info = pl_model::ProviderInfo::openai(Some(base_url));
    info.connection_mode = pl_model::ProviderConnectionMode::Http;
    info.default_model = "local-responses".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let provider_id = ProviderId::new("local").unwrap();
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        reasoning_effort: Some(ReasoningEffort::new("none")),
    };
    let mut config = StudioConfig::default_config();
    config.models = AgentModelConfig {
        providers: BTreeMap::from([(provider_id, provider)]),
        routes: ["explorer", "planner", "executor", "reviewer"]
            .into_iter()
            .map(|role| (AgentRoleId::new(role).unwrap(), route.clone()))
            .collect(),
    };
    config.runtime.permission_mode = PermissionMode::FullAccess;
    config.runtime.tool_capabilities.skills = false;
    config.runtime.tool_capabilities.mcp = false;
    config.runtime.tool_capabilities.lsp = false;
    config.skills.enabled = false;
    config.skills.auto_learn = false;
    config
}
