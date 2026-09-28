mod support;

use pl_core::context::OpaquePayload;
use pl_core::model::DynModelSession;
use pl_core::persistence::{
    ResourceAdmissionError, SqliteSessionOptions, SqliteSessionStore, ThreadEffectQuery,
};
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
    let history = store
        .query_thread_effects(
            "stored-thread",
            ThreadEffectQuery {
                before_sequence: None,
                limit: 32,
            },
        )
        .await
        .unwrap();
    let turns: Vec<_> = history
        .effects
        .iter()
        .filter_map(|effect| effect.turn.as_ref())
        .collect();
    assert!(turns.iter().any(|turn| {
        turn.turn_id == "stored-turn" && turn.state == TurnState::Finished(TurnOutcome::Completed)
    }));
    thread.close().await.unwrap();
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_effect_queries_page_by_sequence_and_keep_threads_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("thread-history.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    for (thread_id, turns) in [("first", 3), ("second", 1)] {
        let (model, _) = ScriptedModel::new(&[]);
        let thread = ThreadHandle::start(thread_id.into(), DynModelSession::new(model)).unwrap();
        thread
            .attach_storage(ColdStoreHandle::new(store.clone()))
            .await
            .unwrap();
        for index in 0..turns {
            thread
                .run_turn(turn(&format!("{thread_id}-{index}")))
                .await
                .unwrap();
        }
        thread.flush().await.unwrap();
        thread.close().await.unwrap();
    }
    store.shutdown().await.unwrap();
    let store = SqliteSessionStore::open(options).await.unwrap();
    let mut cursor = None;
    let mut sequences = Vec::new();
    loop {
        let page = store
            .query_thread_effects(
                "first",
                ThreadEffectQuery {
                    before_sequence: cursor,
                    limit: 2,
                },
            )
            .await
            .unwrap();
        sequences.extend(page.effects.iter().map(|effect| effect.sequence));
        match page.next_before_sequence {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert!(sequences.len() > 2);
    assert!(sequences.windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(
        sequences.len(),
        sequences
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    for sequence in sequences {
        let effect = store
            .read_thread_effect("first", sequence)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(effect.thread_id, "first");
        assert_eq!(effect.sequence, sequence);
    }
    assert!(
        store
            .read_thread_effect("second", u64::MAX)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .query_thread_effects(
                "missing",
                ThreadEffectQuery {
                    before_sequence: None,
                    limit: 2,
                }
            )
            .await
            .unwrap()
            .effects
            .is_empty()
    );
    assert!(
        store
            .query_thread_effects(
                "first",
                ThreadEffectQuery {
                    before_sequence: None,
                    limit: 0,
                }
            )
            .await
            .is_err()
    );
    store.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_effect_queries_reject_mismatched_ownership() {
    let store = SqliteSessionStore::open_memory().await.unwrap();
    let effect = ThreadEffectBatch {
        thread_id: "other".into(),
        sequence: 1,
        ..Default::default()
    };
    store
        .register_resource(
            "claimed",
            "thread-commit.00000000000000000001",
            effect.encode().unwrap(),
        )
        .unwrap();
    store.flush().await.unwrap();
    assert!(store.read_thread_effect("claimed", 1).await.is_err());
    assert!(
        store
            .query_thread_effects(
                "claimed",
                ThreadEffectQuery {
                    before_sequence: None,
                    limit: 1,
                }
            )
            .await
            .is_err()
    );
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
