use super::StudioStore;
use crate::studio::store::directory::{
    DirectoryDelta, RegisteredChildThread, apply_directory_delta,
};
use crate::studio::task_coordinator::{
    AllocateExecutor, CreateTaskRun, TaskExecutorBlueprint, TaskWorktreeDisposition, WorkUnitState,
    current_work_units,
};
use crate::{PlanConfirmationResolution, PlanConfirmationResolutionPayload, StudioMode};
use pl_protocol::ThreadMode;
use sea_orm::{ConnectionTrait, TransactionTrait};
use sha2::{Digest, Sha256};

mod schema;

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test]
async fn tool_images_reuse_content_addressed_blob_and_rollback_failed_metadata() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Tool image", StudioMode::Simple)
        .await
        .unwrap();
    let bytes = b"verified-image-snapshot".to_vec();
    let content_sha256 = digest(&bytes);
    let input = || pl_core::ToolImageAttachmentInput {
        filename: "pure-7429.png".to_string(),
        media_type: "image/png".to_string(),
        data: bytes.clone(),
        content_sha256: content_sha256.clone(),
        width: 320,
        height: 180,
    };

    let first = store.persist_tool_image(&thread.id, input()).await.unwrap();
    let second = store.persist_tool_image(&thread.id, input()).await.unwrap();
    assert_ne!(first.id, second.id);
    let records = store.list_thread_attachments(&thread.id).await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].storage_path, records[1].storage_path);
    assert_eq!(
        tokio::fs::read(&records[0].storage_path).await.unwrap(),
        bytes
    );

    let failed_bytes = b"uncommitted-image-snapshot".to_vec();
    let failed_digest = digest(&failed_bytes);
    let failed_blob = store
        .attachments_dir()
        .join("objects")
        .join(&failed_digest[..2])
        .join(&failed_digest);
    let error = store
        .persist_tool_image(
            "missing-thread",
            pl_core::ToolImageAttachmentInput {
                filename: "missing.png".to_string(),
                media_type: "image/png".to_string(),
                data: failed_bytes,
                content_sha256: failed_digest,
                width: 1,
                height: 1,
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("FOREIGN KEY"));
    assert!(!tokio::fs::try_exists(failed_blob).await.unwrap());
}

#[tokio::test]
async fn tool_image_batch_is_ordered_and_rolls_back_all_new_blobs() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Tool image batch", StudioMode::Simple)
        .await
        .unwrap();
    let input = |name: &str, bytes: &[u8]| pl_core::ToolImageAttachmentInput {
        filename: name.to_string(),
        media_type: "image/png".to_string(),
        data: bytes.to_vec(),
        content_sha256: digest(bytes),
        width: 8,
        height: 4,
    };

    let attachments = store
        .persist_tool_images(
            &thread.id,
            vec![input("first.png", b"first"), input("second.png", b"second")],
        )
        .await
        .unwrap();
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0].filename.as_deref(), Some("first.png"));
    assert_eq!(attachments[1].filename.as_deref(), Some("second.png"));

    let failed_first = b"failed-first";
    let failed_second = b"failed-second";
    let failed_paths = [failed_first.as_slice(), failed_second.as_slice()].map(|bytes| {
        let digest = digest(bytes);
        store
            .attachments_dir()
            .join("objects")
            .join(&digest[..2])
            .join(digest)
    });
    let error = store
        .persist_tool_images(
            "missing-thread",
            vec![
                input("failed-first.png", failed_first),
                input("failed-second.png", failed_second),
            ],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("FOREIGN KEY"));
    for path in failed_paths {
        assert!(!tokio::fs::try_exists(path).await.unwrap());
    }
    assert_eq!(
        store
            .list_thread_attachments(&thread.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn tool_image_snapshot_survives_restart_and_enforces_thread_owner() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("studio.sqlite");
    let workspace = tempfile::tempdir().unwrap();
    let store = StudioStore::open(&database).await.unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let owner = store
        .create_thread(&project.id, "Image owner", StudioMode::Simple)
        .await
        .unwrap();
    let other = store
        .create_thread(&project.id, "Other thread", StudioMode::Simple)
        .await
        .unwrap();
    let bytes = b"durable-tool-image".to_vec();
    let attachment = store
        .persist_tool_image(
            &owner.id,
            pl_core::ToolImageAttachmentInput {
                filename: "durable.png".to_string(),
                media_type: "image/png".to_string(),
                data: bytes.clone(),
                content_sha256: digest(&bytes),
                width: 12,
                height: 8,
            },
        )
        .await
        .unwrap();
    drop(store);

    let reopened = StudioStore::open(&database).await.unwrap();
    assert_eq!(
        reopened
            .read_attachment_bytes(&owner.id, &attachment.id)
            .await
            .unwrap(),
        bytes
    );
    let error = reopened
        .read_attachment_bytes(&other.id, &attachment.id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("does not belong to Thread"));
}

/// 测试 seed：在独立事务中直接应用一次目录 delta。
async fn seed_directory(store: &StudioStore, delta: DirectoryDelta) {
    let tx = store.database().begin().await.unwrap();
    apply_directory_delta(&tx, &delta).await.unwrap();
    tx.commit().await.unwrap();
}

async fn seed_child(
    store: &StudioStore,
    root: &super::super::records::ThreadRecord,
    child_suffix: &str,
    role: &str,
) -> String {
    let project = store
        .read_thread(&root.id)
        .await
        .unwrap()
        .unwrap()
        .project_id;
    let child_id = format!("{}-{child_suffix}", root.id);
    seed_directory(
        store,
        DirectoryDelta::register_child_thread(RegisteredChildThread {
            id: child_id.clone(),
            parent_thread_id: root.id.clone(),
            agent_path: child_id.clone(),
            project_id: project,
            root_thread_id: root.root_thread_id.clone(),
            mode: ThreadMode::Simple,
            role: role.to_string(),
            title: "Child".to_string(),
        }),
    )
    .await;
    child_id
}

#[tokio::test]
async fn directory_archive_rolls_back_the_complete_tree_on_update_failure() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let root = store
        .create_thread(&project.id, "Root", StudioMode::Simple)
        .await
        .unwrap();
    let child_id = seed_child(&store, &root, "child", "executor").await;
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

    let tx = store.database().begin().await.unwrap();
    let error = apply_directory_delta(
        &tx,
        &DirectoryDelta::archive_threads(vec![root.id.clone(), child_id.clone()]),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("forced child archive failure"));
    tx.rollback().await.unwrap();

    let remaining = store.list_threads_for_root(&root.id).await.unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.iter().any(|thread| thread.id == root.id));
    assert!(remaining.iter().any(|thread| thread.id == child_id));
}

#[tokio::test]
async fn thread_directory_rejects_unknown_mode_and_state_variants() {
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

    let state_error = store
        .database()
        .execute_unprepared(&format!(
            "UPDATE threads SET mode = 'simple', state_json = '{{\"kind\":\"legacy\",\"data\":{{}}}}' WHERE id = '{}'",
            thread.id
        ))
        .await
        .unwrap_err();
    assert!(state_error.to_string().contains("CHECK constraint failed"));
}

#[tokio::test]
async fn unregistered_child_spawn_failure_is_persisted_as_canonical_faulted_state() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let root = store
        .create_thread(&project.id, "Root", StudioMode::Task)
        .await
        .unwrap();
    let child_id = seed_child(&store, &root, "executor", "executor").await;

    assert_eq!(
        store
            .fault_unregistered_child_thread(&child_id, "registration failed")
            .await
            .unwrap(),
        super::UnregisteredThreadFault::Faulted
    );
    assert_eq!(
        store.read_thread(&child_id).await.unwrap().unwrap().status,
        pl_protocol::ThreadStatus::Faulted
    );
}

#[tokio::test]
async fn same_project_allows_independent_active_tasks_on_different_root_threads() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let first = store
        .create_thread(&project.id, "First", StudioMode::Task)
        .await
        .unwrap();
    let second = store
        .create_thread(&project.id, "Second", StudioMode::Task)
        .await
        .unwrap();

    for (thread, request) in [(&first, "first request"), (&second, "second request")] {
        store
            .create_task_run(CreateTaskRun {
                project_id: project.id.clone(),
                root_thread_id: thread.id.clone(),
                request: request.to_string(),
                workspace_root: workspace.path().to_string_lossy().to_string(),
            })
            .await
            .unwrap();
    }

    let runs = store.list_task_runs_for_project(&project.id).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| run.kind().as_str() == "planning"));
}

#[tokio::test]
async fn failed_executor_allocation_creates_an_explicit_attempt_chain() {
    let store = StudioStore::open_memory().await.unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let project = store.upsert_project(workspace.path()).await.unwrap();
    let thread = store
        .create_thread(&project.id, "Attempts", StudioMode::Task)
        .await
        .unwrap();
    store
        .create_task_run(CreateTaskRun {
            project_id: project.id,
            root_thread_id: thread.id.clone(),
            request: "deliver feature".to_string(),
            workspace_root: workspace.path().to_string_lossy().to_string(),
        })
        .await
        .unwrap();
    let (pending, interaction) = store
        .submit_task_plan(&thread.id, "implementation plan", "plan-call", 0, 0)
        .await
        .unwrap();
    let (editing, _) = store
        .resolve_task_plan_confirmation(
            &interaction.interaction_id,
            PlanConfirmationResolutionPayload {
                decision: PlanConfirmationResolution::Confirm,
                content: None,
                reason: None,
            },
        )
        .await
        .unwrap();
    let working = store
        .finish_task_document_editing(
            &thread.id,
            editing.revision,
            editing.generation(),
            "documents ready",
        )
        .await
        .unwrap();
    assert!(working.revision > pending.revision);

    let first = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread.id.clone(),
            title: "implement feature".to_string(),
            scope_hints: vec!["src".to_string()],
            blueprint: TaskExecutorBlueprint::for_test(
                "implement feature",
                vec!["src".to_string()],
            ),
            agent_id: "executor-1".to_string(),
            requested_by_call_id: "spawn-call-1".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(first.work_unit.attempt, 1);
    assert_eq!(first.work_unit.supersedes_work_unit_id, None);
    store
        .update_work_unit_state_for_test(
            &first.work_unit.id,
            WorkUnitState::failed_for_test(
                "spawn-1",
                "allocation failed",
                TaskWorktreeDisposition::Protect,
            ),
        )
        .await
        .unwrap();

    let second = store
        .allocate_executor(AllocateExecutor {
            thread_id: thread.id,
            title: "implement feature".to_string(),
            scope_hints: vec!["src".to_string()],
            blueprint: TaskExecutorBlueprint::for_test(
                "implement feature",
                vec!["src".to_string()],
            ),
            agent_id: "executor-2".to_string(),
            requested_by_call_id: "spawn-call-2".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(second.work_unit.attempt, 2);
    assert_eq!(
        second.work_unit.supersedes_work_unit_id.as_deref(),
        Some(first.work_unit.id.as_str())
    );
    let units = store.list_work_units(&working.id).await.unwrap();
    assert_eq!(current_work_units(&units), vec![&second.work_unit]);
}
