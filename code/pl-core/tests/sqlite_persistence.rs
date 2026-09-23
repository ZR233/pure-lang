mod support;

use pl_core::context::OpaquePayload;
use pl_core::model::DynModelSession;
use pl_core::persistence::{ResourceAdmissionError, SqliteSessionOptions, SqliteSessionStore};
use pl_core::thread::cold::ColdStoreHandle;
use pl_core::thread::{ThreadEffectBatch, ThreadHandle, TurnOutcome, TurnState};

use support::{ScriptedModel, turn};

#[tokio::test]
async fn a_thread_turn_is_durable_in_the_sqlite_cold_store() {
    let store = SqliteSessionStore::open_memory().await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("stored-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();

    let completion = thread.run_turn(turn("stored-turn")).await.unwrap();
    assert_eq!(completion.outcome, TurnOutcome::Completed);
    thread.flush().await.unwrap();
    let snapshot = thread.snapshot();
    assert!(snapshot.persistence.durable_sequence >= snapshot.commit_sequence);
    let history = store.replay_entries("stored-thread", None).await.unwrap();
    let turns: Vec<_> = history
        .iter()
        .filter(|entry| entry.id.starts_with("pl.resource.thread-commit."))
        .filter_map(|entry| {
            let payload =
                OpaquePayload::new(&*entry.type_id, entry.schema_version, &*entry.payload).unwrap();
            ThreadEffectBatch::decode(&payload).unwrap().turn
        })
        .collect();
    assert!(turns.iter().any(|turn| {
        turn.turn_id == "stored-turn" && turn.state == TurnState::Finished(TurnOutcome::Completed)
    }));
    thread.close().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn sqlite_reopen_preserves_opaque_resource_and_replay_history() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("history.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let original = OpaquePayload::new("unknown.v3", 3, "  {\"number\":1e2}\n").unwrap();
    store
        .register_resource("session", "attachment", original.clone())
        .unwrap();
    store.flush().await.unwrap();
    let first = store.read_entry_history("session", None).await.unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].sequence(), 1);
    store.shutdown().await.unwrap();

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    let replay = reopened.replay_entries("session", None).await.unwrap();
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].id, "pl.resource.attachment");
    assert_eq!(replay[0].type_id, original.format());
    assert_eq!(replay[0].schema_version, original.version());
    assert_eq!(replay[0].payload, original.content());
    assert!(matches!(
        reopened.register_resource("session", "attachment", OpaquePayload::text("different")),
        Err(ResourceAdmissionError::Conflict { .. })
    ));
    assert_eq!(
        reopened.replay_entries("session", Some(1)).await.unwrap(),
        replay
    );
    reopened.shutdown().await.unwrap();
}
