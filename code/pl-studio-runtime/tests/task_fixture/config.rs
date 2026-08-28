use std::collections::BTreeMap;

use pl_core::{
    AgentModelConfig, AgentRoleId, ModelInfo, ModelParameter, ModelRouteConfig, PermissionMode,
    ProviderConfig, ProviderId, ReasoningEffort,
};
use pl_studio_runtime::StudioConfig;

pub(super) fn task_test_config(base_url: String) -> StudioConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.transport = pl_model::ModelTransportProfile::responses_http();
    model.context_window = Some(128_000);
    model.parameters = vec![ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: BTreeMap::new(),
    }];

    let info = pl_model::ProviderEndpoint::openai(Some(base_url));
    let provider = ProviderConfig::from_explicit_models(info, vec![model]);
    let provider_id = ProviderId::new("local").unwrap();
    let route = ModelRouteConfig {
        provider: provider_id.clone(),
        model: "local-responses".to_string(),
        effort: Some(ReasoningEffort::new("none")),
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
    config.instructions.developer =
        "GLOBAL_DEVELOPER_CONTEXT_MARKER: deterministic Task wire acceptance.".to_string();
    config.instructions.user =
        "GLOBAL_USER_CONTEXT_MARKER: preserve the complete role prompt.".to_string();
    config.runtime.tool_capabilities.skills = true;
    config.runtime.tool_capabilities.mcp = false;
    config.runtime.tool_capabilities.lsp = false;
    config.skills.enabled = true;
    config.skills.system.enabled = false;
    config.skills.auto_learn = false;
    config
}
