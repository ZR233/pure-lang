use super::*;

#[tokio::test]
async fn legacy_mode_sessions_remain_stored_but_are_not_loadable() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/mode-visibility")
        .await
        .unwrap();
    let legacy_auto = store
        .create_session(&project.id, "Legacy auto", CompileMode::Simple)
        .await
        .unwrap();
    let legacy_plan = store
        .create_session(&project.id, "Legacy plan", CompileMode::Task)
        .await
        .unwrap();
    let current = store
        .create_session(&project.id, "Current", CompileMode::Simple)
        .await
        .unwrap();
    for (session_id, legacy_mode) in [(&legacy_auto.id, "auto"), (&legacy_plan.id, "plan")] {
        store
            .db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE sessions SET mode = ? WHERE id = ?",
                [legacy_mode.into(), session_id.clone().into()],
            ))
            .await
            .unwrap();
    }
    store
        .db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE sessions SET mode = 'simple' WHERE id = ?",
            [current.id.clone().into()],
        ))
        .await
        .unwrap();

    let sessions = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, current.id);
    assert_eq!(store.read_session(&legacy_auto.id).await.unwrap(), None);
    assert_eq!(store.read_session(&legacy_plan.id).await.unwrap(), None);
    assert!(store.read_session(&current.id).await.unwrap().is_some());
}
