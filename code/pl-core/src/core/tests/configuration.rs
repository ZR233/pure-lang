use super::*;
use pretty_assertions::assert_eq;

#[test]
fn config_core_uses_planner_role_model_and_effort() {
    let config = ConfigStore::new(crate::ConfigPaths::from_home("unused"))
        .load_or_default()
        .unwrap();
    let core = PureCore::from_config(&config, ModelRole::Planner).unwrap();

    assert_eq!(core.provider.default_model(), "deepseek-v4-flash");
    assert_eq!(core.reasoning_effort.unwrap().as_str(), "high");
}
