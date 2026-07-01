use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn settings_round_trip() {
    let store = StudioStore::open_memory().await.unwrap();

    store
        .save_setting("activeProject", "project-1")
        .await
        .unwrap();
    let value = store.load_setting("activeProject").await.unwrap();

    assert_eq!(value.as_deref(), Some("project-1"));
}
