mod support;

use std::sync::Arc;

use pl_core::context::OpaquePayload;
use pl_core::model::DynModelSession;
use pl_core::persistence::{
    ResourceAdmissionError, SessionStoreError, SqliteSessionOptions, SqliteSessionStore,
    ThreadEffectQuery, migration,
};
use pl_core::thread::cold::{ColdStore, ColdStoreHandle, ThreadWrite};
use pl_core::thread::{
    ThreadCheckpoint, ThreadEffectBatch, ThreadHandle, ThreadSnapshot, TurnOutcome, TurnState,
};

use support::{ScriptedModel, turn};

/// Opens the same SQLite file with a second connection so a test can tamper with one row directly.
///
/// This bypasses the store's own guards on purpose so the readers below are exercised against
/// hostile rows. A second connection can also corrupt a row after an existing store has opened.
async fn tamper(options: &SqliteSessionOptions, sql: &str) {
    let url = format!("sqlite://{}?mode=rw", options.path.display());
    let db = sea_orm::Database::connect(url).await.unwrap();
    sea_orm::ConnectionTrait::execute_unprepared(&db, sql)
        .await
        .unwrap();
    db.close().await.unwrap();
}

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
async fn deleting_a_session_requires_an_exclusive_lease_and_preserves_other_threads() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("session-deletion.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    for id in ["expired", "retained"] {
        let (model, _) = ScriptedModel::new(&[]);
        let thread = ThreadHandle::start(id.into(), DynModelSession::new(model)).unwrap();
        thread
            .attach_storage(ColdStoreHandle::new(store.clone()))
            .await
            .unwrap();
        thread.run_turn(turn(&format!("{id}-turn"))).await.unwrap();
        thread.close().await.unwrap();
    }
    assert!(
        SqliteSessionStore::delete_session(options.clone(), "expired")
            .await
            .is_err()
    );
    store.shutdown().await.unwrap();

    assert!(
        SqliteSessionStore::delete_session(options.clone(), "expired")
            .await
            .unwrap()
    );
    assert!(
        !SqliteSessionStore::delete_session(options.clone(), "expired")
            .await
            .unwrap()
    );
    let store = SqliteSessionStore::open(options).await.unwrap();
    assert!(
        store
            .read_thread_checkpoint("expired")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .query_thread_effects(
                "expired",
                ThreadEffectQuery {
                    before_sequence: None,
                    limit: 1,
                },
            )
            .await
            .unwrap()
            .effects
            .is_empty()
    );
    assert!(
        store
            .read_thread_checkpoint("retained")
            .await
            .unwrap()
            .is_some()
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

#[tokio::test]
async fn thread_checkpoint_is_durable_with_its_effect_and_reopens() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("checkpoint.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread =
        ThreadHandle::start("checkpoint-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread.run_turn(turn("checkpoint-turn")).await.unwrap();
    thread.flush().await.unwrap();

    let checkpoint = store
        .read_thread_checkpoint("checkpoint-thread")
        .await
        .unwrap()
        .expect("a flushed Thread write saves its checkpoint");
    assert_eq!(checkpoint.thread_id, "checkpoint-thread");
    assert_eq!(checkpoint.state_revision, checkpoint.state.commit_sequence);
    assert!(checkpoint.history_fence >= 1);
    assert!(checkpoint.history_fence <= checkpoint.state_revision);
    assert!(checkpoint.external_bodies.is_empty());
    // 同一事务写入：fence 处必须已经有一条已保存的 effect。
    assert!(
        store
            .read_thread_effect("checkpoint-thread", checkpoint.history_fence)
            .await
            .unwrap()
            .is_some()
    );
    thread.close().await.unwrap();
    store.shutdown().await.unwrap();

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    let recovered = reopened
        .read_thread_checkpoint("checkpoint-thread")
        .await
        .unwrap()
        .expect("reopen recovers the durable checkpoint");
    assert_eq!(recovered.thread_id, "checkpoint-thread");
    // close 本身还会提交生命周期 effect，因此重开时的最新 checkpoint 可前进。
    assert!(recovered.history_fence >= checkpoint.history_fence);
    assert!(recovered.state_revision >= checkpoint.state_revision);
    assert!(
        reopened
            .read_thread_effect("checkpoint-thread", recovered.history_fence)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        reopened
            .read_thread_checkpoint("missing-thread")
            .await
            .unwrap()
            .is_none()
    );
    let (model, _) = ScriptedModel::new(&[]);
    let resumed = ThreadHandle::resume(
        "checkpoint-thread".into(),
        DynModelSession::new(model),
        Some(recovered.clone()),
    )
    .unwrap();
    let empty_store = SqliteSessionStore::open_memory().await.unwrap();
    assert!(matches!(
        resumed
            .attach_storage(ColdStoreHandle::new(empty_store.clone()))
            .await,
        Err(pl_core::thread::ThreadError::StorageRecoveryPending { durable: 0, required })
            if required == recovered.history_fence
    ));
    empty_store.shutdown().await.unwrap();
    resumed
        .attach_storage(ColdStoreHandle::new(reopened.clone()))
        .await
        .unwrap();
    assert!(resumed.snapshot().persistence.admitted_sequence >= recovered.history_fence);
    assert!(resumed.snapshot().persistence.durable_sequence >= recovered.history_fence);
    tokio::time::timeout(std::time::Duration::from_secs(3), resumed.flush())
        .await
        .expect("a restored Thread must not wait on already durable history")
        .unwrap();
    resumed.close().await.unwrap();
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_checkpoint_rejects_corrupt_content_hash() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("corrupt-hash.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("corrupt-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread.run_turn(turn("corrupt-turn")).await.unwrap();
    thread.flush().await.unwrap();
    thread.close().await.unwrap();
    store.shutdown().await.unwrap();

    tamper(
        &options,
        "UPDATE thread_checkpoints SET payload_hash='sha256:0000000000000000000000000000000000000000000000000000000000000000' WHERE thread_id='corrupt-thread'",
    )
    .await;

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    assert!(
        reopened
            .read_thread_checkpoint("corrupt-thread")
            .await
            .is_err()
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_checkpoint_rejects_a_corrupt_fence_effect() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("corrupt-fence.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("fence-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread.run_turn(turn("fence-turn")).await.unwrap();
    thread.close().await.unwrap();
    let fence = store
        .read_thread_checkpoint("fence-thread")
        .await
        .unwrap()
        .unwrap()
        .history_fence;
    store.shutdown().await.unwrap();

    let reopened = SqliteSessionStore::open(options.clone()).await.unwrap();

    tamper(
        &options,
        &format!(
            "UPDATE session_entries SET payload_hash='invalid' WHERE session_id='fence-thread' AND id='pl.resource.thread-commit.{fence:020}'"
        ),
    )
    .await;

    assert!(
        reopened
            .read_thread_checkpoint("fence-thread")
            .await
            .is_err()
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_checkpoint_rejects_cross_thread_row() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("cross-thread.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("owner-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread.run_turn(turn("cross-turn")).await.unwrap();
    thread.flush().await.unwrap();
    thread.close().await.unwrap();
    store.shutdown().await.unwrap();

    // 把 owner 的 checkpoint 行改名为另一个 Thread：信封里的身份仍是 owner。
    tamper(
        &options,
        "UPDATE thread_checkpoints SET thread_id='other-thread' WHERE thread_id='owner-thread'",
    )
    .await;

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    assert!(
        reopened
            .read_thread_checkpoint("owner-thread")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        reopened
            .read_thread_checkpoint("other-thread")
            .await
            .is_err()
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn thread_write_transaction_failure_keeps_the_durable_watermark() {
    let store = SqliteSessionStore::open_memory().await.unwrap();
    // 先受理 revision 5，再受理 revision 2；第二份 checkpoint 回退使整批事务失败。
    let newer = ThreadWrite {
        effect: Arc::new(ThreadEffectBatch {
            thread_id: "crashed".into(),
            sequence: 3,
            ..Default::default()
        }),
        checkpoint: ThreadCheckpoint::capture_transfer(
            "crashed".into(),
            3,
            ThreadSnapshot {
                commit_sequence: 5,
                ..Default::default()
            },
        ),
        output_claim: None,
    };
    let older = ThreadWrite {
        effect: Arc::new(ThreadEffectBatch {
            thread_id: "crashed".into(),
            sequence: 2,
            ..Default::default()
        }),
        checkpoint: ThreadCheckpoint::capture_transfer(
            "crashed".into(),
            2,
            ThreadSnapshot {
                commit_sequence: 2,
                ..Default::default()
            },
        ),
        output_claim: None,
    };
    store.admit("crashed", newer).unwrap();
    store.admit("crashed", older).unwrap();
    let admitted = store.persistence();
    assert_eq!(admitted.admitted, 2);
    assert_eq!(admitted.durable, 0);

    assert!(store.flush().await.is_err());
    let failed = store.persistence();
    assert_eq!(
        failed.durable, 0,
        "a failed transaction must not advance the durable watermark"
    );
    assert!(failed.error.is_some());
    // 整批回滚：第一批的 effect 与 checkpoint 都没有落盘。
    assert!(
        store
            .read_thread_effect("crashed", 3)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .read_thread_checkpoint("crashed")
            .await
            .unwrap()
            .is_none()
    );
    let _ = store.shutdown().await;
}

#[tokio::test]
async fn migrate_v7_adds_checkpoint_table_without_losing_effects() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("legacy.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let (model, _) = ScriptedModel::new(&[]);
    let thread = ThreadHandle::start("legacy-thread".into(), DynModelSession::new(model)).unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread.run_turn(turn("legacy-turn")).await.unwrap();
    thread.flush().await.unwrap();
    let original_fence = store
        .read_thread_checkpoint("legacy-thread")
        .await
        .unwrap()
        .unwrap()
        .history_fence;
    thread.close().await.unwrap();
    assert!(
        migration::migrate_to_current(options.clone(), |_| Ok(()))
            .await
            .is_err()
    );
    store.shutdown().await.unwrap();

    // 退回 schema 7：删除新表并复位版本，保留已验证的 effect/entry/history 数据。
    tamper(&options, "DROP TABLE thread_checkpoints").await;
    tamper(&options, "PRAGMA user_version=7").await;

    // 迁移前 open 必须拒绝版本 7，而不是静默重建或迁移。
    assert!(matches!(
        SqliteSessionStore::open(options.clone()).await,
        Err(SessionStoreError::UnsupportedSchema { found: 7, .. })
    ));

    migration::migrate_to_current(options.clone(), |_| Ok(()))
        .await
        .unwrap();
    migration::migrate_to_current(options.clone(), |_| Ok(()))
        .await
        .unwrap();
    let reopened = SqliteSessionStore::open(options).await.unwrap();
    // effect 数据保留；尚无 checkpoint 行的旧 Thread 读取为空。
    assert!(
        reopened
            .read_thread_effect("legacy-thread", original_fence)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        reopened
            .read_thread_checkpoint("legacy-thread")
            .await
            .unwrap()
            .is_none()
    );
    reopened.shutdown().await.unwrap();
}
