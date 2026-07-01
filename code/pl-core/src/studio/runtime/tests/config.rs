use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn set_model_role_persists_planner_model_and_default_effort() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-role-runtime-home-{unique}"));
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    let mut config = test_config("http://127.0.0.1:9".to_string());
    let mut fast_model = ModelInfo::fallback("local-fast");
    fast_model.parameters = vec![crate::ModelParameter {
        name: "effort".to_string(),
        label: None,
        candidates: vec!["low".to_string(), "high".to_string()],
        wire: std::collections::BTreeMap::new(),
    }];
    config
        .providers
        .get_mut("local")
        .unwrap()
        .models
        .push(fast_model);
    config_store.save(&config).unwrap();
    let runtime = StudioRuntime::new(StudioStore::open_memory().await.unwrap(), config_store);

    let next = runtime
        .set_model_role(ModelRole::Planner, "local", "local-fast", None)
        .unwrap();

    assert_eq!(next.roles.planner.provider, "local");
    assert_eq!(next.roles.planner.model, "local-fast");
    assert_eq!(next.roles.planner.effort.as_str(), "low");
    let saved = runtime.config_store().load_or_default().unwrap();
    assert_eq!(saved.roles.planner, next.roles.planner);
    let _ = tokio::fs::remove_dir_all(home).await;
}
