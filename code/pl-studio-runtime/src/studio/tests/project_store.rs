use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn project_crud_orders_by_recent_open() {
    let store = StudioStore::open_memory().await.unwrap();

    let first = store.upsert_project("C:/work/alpha").await.unwrap();
    let second = store.upsert_project("C:/work/beta").await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    store.mark_project_opened(&first.id).await.unwrap();

    let projects = store.list_projects().await.unwrap();

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].id, first.id);
    assert_eq!(projects[1].id, second.id);
}

#[tokio::test]
async fn archive_project_hides_its_sessions_and_can_be_reopened() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();

    let archived = store.archive_project(&project.id).await.unwrap().unwrap();
    let hidden_projects = store.list_projects().await.unwrap();
    let sessions = store.list_sessions(&project.id).await.unwrap();
    let reopened = store.upsert_project("C:/work/alpha").await.unwrap();
    let visible_projects = store.list_projects().await.unwrap();
    let reopened_sessions = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(archived.id, project.id);
    assert_eq!(hidden_projects, Vec::<ProjectRecord>::new());
    assert_eq!(sessions, Vec::<SessionRecord>::new());
    assert_eq!(reopened.id, project.id);
    assert_eq!(visible_projects[0].id, project.id);
    assert_eq!(reopened_sessions, Vec::<SessionRecord>::new());
}
