use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn archive_session_hides_it_from_session_list() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();

    let archived = store.archive_session(&session.id).await.unwrap().unwrap();
    let sessions = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(archived.id, session.id);
    assert_eq!(sessions, Vec::<SessionRecord>::new());
}

#[tokio::test]
async fn set_session_mode_persists_mode_label() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Plan work", StudioMode::Simple)
        .await
        .unwrap();

    store
        .set_session_mode(&session.id, StudioMode::Task)
        .await
        .unwrap();
    let updated = store.read_session(&session.id).await.unwrap().unwrap();

    assert_eq!(updated.mode, "task");
}

#[tokio::test]
async fn instruction_snapshot_round_trips_with_session() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();
    let snapshot = InstructionSnapshot {
        base: InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::BuiltInBase,
                label: "base".to_string(),
                path: None,
            },
            content: "base".to_string(),
        },
        developer: vec![InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::ExecutionProfile,
                label: "mode".to_string(),
                path: None,
            },
            content: "developer".to_string(),
        }],
        user: vec![InstructionBlock {
            source: InstructionSource {
                kind: InstructionSourceKind::ProjectDoc,
                label: "AGENTS.md".to_string(),
                path: Some("C:/work/alpha/AGENTS.md".to_string()),
            },
            content: "project".to_string(),
        }],
    };

    assert_eq!(session.instruction_snapshot, None);
    let saved = store
        .save_instruction_snapshot(&session.id, &snapshot)
        .await
        .unwrap()
        .unwrap();
    let read = store.read_session(&session.id).await.unwrap().unwrap();
    let listed = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(saved.instruction_snapshot, Some(snapshot.clone()));
    assert_eq!(read.instruction_snapshot, Some(snapshot.clone()));
    assert_eq!(listed[0].instruction_snapshot, Some(snapshot));
}
