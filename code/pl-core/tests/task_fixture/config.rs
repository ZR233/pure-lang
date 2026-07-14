use std::collections::BTreeMap;

use pl_core::{
    ModelInfo, ModelParameter, PermissionMode, ProviderConfig, PureConfig, ReasoningEffort,
    RoleConfig, RoleConfigs,
};

pub(super) fn task_test_config(base_url: String) -> PureConfig {
    let mut model = ModelInfo::fallback("local-responses");
    model.context_window = Some(128_000);
    model.parameters = vec![ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["none".to_string()],
        wire: BTreeMap::new(),
    }];

    let mut info = pl_model::ProviderInfo::openai(Some(base_url));
    info.default_model = "local-responses".to_string();
    let provider = ProviderConfig::from_provider_info(info, vec![model]);
    let role = RoleConfig {
        provider: "local".to_string(),
        model: "local-responses".to_string(),
        effort: ReasoningEffort::new("none"),
    };
    let mut config = PureConfig {
        roles: RoleConfigs::from_default_role(role),
        providers: BTreeMap::from([("local".to_string(), provider)]),
        ..PureConfig::default_config()
    };
    config.runtime.permission_mode = PermissionMode::FullAccess;
    config.runtime.tool_capabilities.skills = false;
    config.runtime.tool_capabilities.mcp = false;
    config.runtime.tool_capabilities.lsp = false;
    config.skills.enabled = false;
    config.skills.auto_learn = false;
    config
}
