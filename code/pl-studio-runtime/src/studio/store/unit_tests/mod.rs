use super::StudioStore;
use crate::StudioMode;
use sea_orm::ConnectionTrait;

mod schema;
mod task_coordinator;

#[tokio::test]
async fn archive_thread_rolls_back_the_complete_tree_on_update_failure() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let root = store
        .create_thread(&project.id, "Root", StudioMode::Simple)
        .await
        .unwrap();
    let child_id = format!("{}-child", root.id);
    let child = store
        .create_child_thread(crate::studio::ChildThreadSpec {
            id: child_id.clone(),
            parent_thread_id: root.id.clone(),
            agent_path: child_id,
            role: "executor".to_string(),
            title: "Child".to_string(),
        })
        .await
        .unwrap();
    store
        .database()
        .execute_unprepared(
            "CREATE TRIGGER fail_child_archive \
             BEFORE UPDATE OF archived ON threads \
             WHEN OLD.parent_thread_id IS NOT NULL \
             BEGIN SELECT RAISE(FAIL, 'forced child archive failure'); END",
        )
        .await
        .unwrap();

    let error = store.archive_thread(&root.id).await.unwrap_err();

    assert!(error.to_string().contains("forced child archive failure"));
    let remaining = store.list_threads(&project.id).await.unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().any(|thread| thread.id == root.id));
    assert!(remaining.iter().any(|thread| thread.id == child.id));
}

#[tokio::test]
async fn thread_directory_rejects_unknown_mode_and_status_labels() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Typed state", StudioMode::Simple)
        .await
        .unwrap();

    store
        .database()
        .execute_unprepared(&format!(
            "UPDATE threads SET mode = 'legacy' WHERE id = '{}'",
            thread.id
        ))
        .await
        .unwrap();
    let mode_error = store.read_thread(&thread.id).await.unwrap_err();
    assert!(mode_error.to_string().contains("unsupported Thread mode"));
    assert!(mode_error.to_string().contains(&thread.id));

    store
        .database()
        .execute_unprepared(&format!(
            "UPDATE threads SET mode = 'simple', status = 'legacy' WHERE id = '{}'",
            thread.id
        ))
        .await
        .unwrap();
    let status_error = store.read_thread(&thread.id).await.unwrap_err();
    assert!(
        status_error
            .to_string()
            .contains("unsupported Thread status")
    );
    assert!(status_error.to_string().contains(&thread.id));
}
