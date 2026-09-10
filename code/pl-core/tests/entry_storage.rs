use pl_core::context::OpaquePayload;
use pl_core::persistence::{SqliteSessionOptions, SqliteSessionStore};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn existing_unversioned_database_is_rejected_without_changing_its_data() {
    use sea_orm::{ConnectionTrait, Database};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("existing.sqlite");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Database::connect(&url).await.unwrap();
    db.execute_unprepared(
        "CREATE TABLE original(value TEXT); INSERT INTO original VALUES('preserve')",
    )
    .await
    .unwrap();
    db.close().await.unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = SqliteSessionStore::open(SqliteSessionOptions { path: path.clone() })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        pl_core::persistence::SessionStoreError::UnsupportedSchema {
            found: 0,
            supported: 6
        }
    ));
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[tokio::test]
async fn writer_preserves_unknown_strings_and_replays_after_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("sessions.sqlite"),
    };
    let content = " {\"number\":90071992547409931234567890, \"endTurn\":true} \r\n\0中文";
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let payload = OpaquePayload::new("pl.future-format", 900, content).unwrap();
    store
        .register_resource("thread", "output", payload.clone())
        .unwrap();
    store
        .register_resource("thread", "output", payload)
        .unwrap();
    store.flush().await.unwrap();
    let original = store.read_entries("thread", None).await.unwrap();
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].payload.as_bytes(), content.as_bytes());
    assert_eq!(
        store
            .read_entry_history("thread", None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.replay_entries("thread", None).await.unwrap(),
        original
    );
    store.shutdown().await.unwrap();
    drop(store);

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    assert_eq!(reopened.resources("thread", "pl.future-format"), original);
    assert_eq!(
        reopened.replay_entries("thread", Some(1)).await.unwrap(),
        original
    );
    let changed = OpaquePayload::new("pl.future-format", 900, "different").unwrap();
    assert!(
        reopened
            .register_resource("thread", "output", changed)
            .is_err()
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn committed_thread_payload_is_not_rejected_by_metadata_record_size_limit() {
    use pl_core::thread::cold::ColdStore;
    let store = SqliteSessionStore::open_memory().await.unwrap();
    let content = "载荷".repeat(400_000);
    let payload = OpaquePayload::new("plugin.future-record", 19, content.clone()).unwrap();
    ColdStore::admit(&store, "thread", 1, payload).unwrap();
    ColdStore::flush(&store, "thread", 1).await.unwrap();
    let records = store.replay_entries("thread", None).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].payload, content);
    assert_eq!(records[0].schema_version, 19);
    store.shutdown().await.unwrap();
}

struct JournalModel(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl pl_core::model::ModelSession for JournalModel {
    fn prepare(
        &mut self,
        request: pl_core::model::ModelRequest,
    ) -> impl std::future::Future<
        Output = Result<pl_core::model::PreparedModelCall, pl_core::model::ModelError>,
    > + Send {
        let calls = self.0.clone();
        async move {
            request
                .context
                .validate_complete()
                .expect("valid restored input");
            Ok(pl_core::model::PreparedModelCall::new(async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(pl_core::model::ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: vec![pl_core::context::ContextContent::Opaque {
                        payload: OpaquePayload::new("provider.future-native", 91, "native\0原文\n")
                            .unwrap(),
                    }],
                    tool_calls: Vec::new(),
                    private_context: Some(
                        OpaquePayload::new("provider.private", 13, "private\0state").unwrap(),
                    ),
                    usage: pl_core::model::ModelUsage {
                        input_tokens: Some(17),
                        ..Default::default()
                    },
                })
            }))
        }
    }

    async fn close(&mut self) -> Result<(), pl_core::model::ModelError> {
        Ok(())
    }
}

#[tokio::test]
async fn thread_journal_reopens_and_continues_without_replaying_model_work() {
    use pl_core::context::ContextContent;
    use pl_core::model::DynModelSession;
    use pl_core::thread::{StepInput, ThreadHandle, ThreadLifecycle, cold::ColdStoreHandle};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio_util::sync::CancellationToken;

    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("journal.sqlite"),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let thread = ThreadHandle::start(
        "thread".into(),
        DynModelSession::new(JournalModel(calls.clone())),
    )
    .unwrap();
    thread
        .attach_storage(ColdStoreHandle::new(store.clone()))
        .await
        .unwrap();
    thread
        .step(StepInput {
            turn_id: "turn-1".into(),
            attempt_id: "attempt-1".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("  user 原文\n"),
            }],
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    thread
        .submit_input(pl_core::thread::input::ThreadInput {
            id: "pending-input".into(),
            payload: OpaquePayload::new("future.input", 81, "raw\r\n{\"approved\":true} 原文")
                .unwrap(),
            context: vec![ContextContent::Text {
                text: Arc::from("pending user input"),
            }],
        })
        .await
        .unwrap();
    thread.close().await.unwrap();
    let saved = thread.snapshot();
    assert_eq!(saved.lifecycle, ThreadLifecycle::Closed);
    assert_eq!(saved.persistence.durable_sequence, saved.commit_sequence);
    store.shutdown().await.unwrap();
    drop(thread);
    drop(store);

    let reopened = SqliteSessionStore::open(options).await.unwrap();
    let replayed = reopened.replay_thread("thread").await.unwrap();
    assert_eq!(replayed.context, saved.context);
    assert_eq!(replayed.inputs, saved.inputs);
    assert_eq!(replayed.private_context, saved.private_context);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let history = reopened.read_thread_journal("thread").await.unwrap();
    let restored = ThreadHandle::restore(
        "thread".into(),
        DynModelSession::new(JournalModel(calls.clone())),
        history,
    )
    .unwrap();
    assert!(restored.snapshot().private_context.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    restored
        .attach_storage(ColdStoreHandle::new(reopened.clone()))
        .await
        .unwrap();
    restored
        .run_next_input(pl_core::thread::input::QueuedTurn {
            turn_id: "turn-2".into(),
            attempt_prefix: "attempt-2".into(),
            max_model_steps: std::num::NonZeroU32::new(2).unwrap(),
            cancellation: CancellationToken::new(),
        })
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    restored.close().await.unwrap();
    let continued = reopened.replay_thread("thread").await.unwrap();
    assert_eq!(continued.context, restored.snapshot().context);
    assert_eq!(continued.attempts.len(), 2);
    assert_eq!(continued.inputs[0].input, saved.inputs[0].input);
    assert!(matches!(
        continued.inputs[0].state,
        pl_core::thread::input::InputState::Consumed { .. }
    ));
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn immutable_record_durability_does_not_require_a_live_shared_writer_after_save() {
    let store = SqliteSessionStore::open_memory().await.unwrap();
    store
        .register_resource("first", "record", OpaquePayload::text("saved"))
        .unwrap();
    store
        .flush_resource("first", "pl.resource.record")
        .await
        .unwrap();
    store
        .register_resource("second", "record", OpaquePayload::text("another thread"))
        .unwrap();
    store
        .flush_resource("first", "pl.resource.record")
        .await
        .unwrap();
    store.shutdown().await.unwrap();
    store
        .flush_resource("first", "pl.resource.record")
        .await
        .unwrap();
    assert!(
        store
            .flush_resource("first", "pl.resource.missing")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn shutdown_closes_shared_connections_before_releasing_database_ownership() {
    let directory = tempfile::tempdir().unwrap();
    let options = SqliteSessionOptions {
        path: directory.path().join("sessions.sqlite"),
    };
    let store = SqliteSessionStore::open(options.clone()).await.unwrap();
    let reader = store.clone();
    store
        .register_resource("thread", "result", OpaquePayload::text("durable"))
        .unwrap();
    assert!(SqliteSessionStore::open(options.clone()).await.is_err());
    store.shutdown().await.unwrap();
    assert!(reader.read_entries("thread", None).await.is_err());
    let reopened = SqliteSessionStore::open(options).await.unwrap();
    assert_eq!(
        reopened.read_entries("thread", None).await.unwrap()[0].payload,
        "durable"
    );
    store.shutdown().await.unwrap();
    assert_eq!(
        reopened.read_entries("thread", None).await.unwrap().len(),
        1
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn shared_store_counts_pending_bytes_per_thread_and_releases_pressure_after_flush() {
    use pl_core::thread::cold::ColdStore;
    let store = SqliteSessionStore::open_memory().await.unwrap();
    let peer = store.clone();
    ColdStore::admit(&store, "first", 1, OpaquePayload::text("first")).unwrap();
    ColdStore::admit(&peer, "second", 1, OpaquePayload::text("second")).unwrap();
    // No await precedes these observations: the writer cannot run on this current-thread runtime.
    assert_eq!(store.pending_bytes("first"), (5, 11));
    assert_eq!(peer.pending_bytes("second"), (6, 11));
    assert_eq!(store.persistence().pending_commits, 2);
    ColdStore::flush(&store, "first", 1).await.unwrap();
    store.flush().await.unwrap();
    assert_eq!(store.pending_bytes("first"), (0, 0));
    assert_eq!(peer.pending_bytes("second"), (0, 0));
    assert_eq!(store.session_ids().await.unwrap(), vec!["first", "second"]);
    store.shutdown().await.unwrap();
    assert!(matches!(
        peer.register_resource("third", "new", OpaquePayload::text("closed")),
        Err(pl_core::persistence::ResourceAdmissionError::StoreClosed)
    ));
}

#[tokio::test]
async fn old_business_session_schema_is_rejected_without_rewriting_history() {
    use sea_orm::{ConnectionTrait, Database};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("old.sqlite");
    let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    db.execute_unprepared("CREATE TABLE session_entries(payload TEXT); INSERT INTO session_entries VALUES('old actor'); PRAGMA user_version=5")
        .await.unwrap();
    db.close().await.unwrap();
    let before = std::fs::read(&path).unwrap();
    let error = SqliteSessionStore::open(SqliteSessionOptions { path: path.clone() })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        pl_core::persistence::SessionStoreError::UnsupportedSchema {
            found: 5,
            supported: 6
        }
    ));
    assert_eq!(std::fs::read(path).unwrap(), before);
}
