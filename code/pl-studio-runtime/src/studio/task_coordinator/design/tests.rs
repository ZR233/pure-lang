use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::*;
use crate::studio::task_coordinator::{
    AllocateExecutor, CreateMergeRecord, MergeStatus, UpdateMergeRecord,
};
use crate::{StudioMode, StudioStore};

const DESIGN_PATCH: &str =
    "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+after\n*** End Patch";

#[tokio::test]
async fn design_patch_commits_and_atomically_opens_executor_gate() {
    let fixture = DesignFixture::new("success").await;
    let before = fixture.run.expected_head.clone();

    let denied = fixture
        .store
        .allocate_executor(allocation(&fixture.session_id, "before-design"))
        .await;
    let denied = match denied {
        Ok(_) => panic!("executor allocation must remain gated before durable design"),
        Err(error) => error,
    };
    assert!(denied.to_string().contains("requires task phase"));

    let output = fixture.update(DESIGN_PATCH).await.unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(output.task_run_id, fixture.run.id);
    assert_eq!(output.previous_head, before);
    assert_eq!(output.changed_files, vec!["design/spec.md".to_string()]);
    assert_eq!(run.phase, TaskRunPhase::Implementing);
    assert_eq!(
        run.design_commit.as_deref(),
        Some(output.design_commit.as_str())
    );
    assert_eq!(run.expected_head, output.design_commit);
    assert_eq!(lease.expected_head, run.expected_head);
    assert_eq!(
        git_output(&fixture.repository, &["rev-parse", "HEAD"]),
        run.expected_head
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repository.join("design/spec.md")).unwrap(),
        "after\n"
    );

    fixture
        .store
        .allocate_executor(allocation(&fixture.session_id, "after-design"))
        .await
        .expect("executor allocation should open immediately after durable design");
    fixture.cleanup().await;
}

#[tokio::test]
async fn empty_noop_and_later_hunk_failure_restore_exact_state() {
    let fixture = DesignFixture::new("rollback").await;
    let before = fixture.head();
    for patch in [
        "*** Begin Patch\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n*** End Patch",
        "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-before\n+temporary\n*** Update File: design/missing.md\n@@\n-missing\n+changed\n*** End Patch",
    ] {
        assert!(fixture.update(patch).await.is_err());
        assert_eq!(fixture.head(), before);
        assert_eq!(fixture.design_text(), "before\n");
        assert!(fixture.status().is_empty());
    }
    fixture.cleanup().await;
}

#[tokio::test]
async fn concurrent_identical_calls_serialize_to_one_commit_and_one_cas() {
    let fixture = DesignFixture::new("concurrent").await;
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let mut calls = Vec::new();
    for _ in 0..2 {
        let coordinator = fixture.coordinator.clone();
        let session_id = fixture.session_id.clone();
        let repository = fixture.repository.clone();
        let start = start.clone();
        calls.push(tokio::spawn(async move {
            start.wait().await;
            coordinator
                .update_design(&session_id, &repository, DESIGN_PATCH)
                .await
        }));
    }
    start.wait().await;
    let results = futures::future::join_all(calls).await;
    let successes = results
        .into_iter()
        .filter(|result| result.as_ref().is_ok_and(|inner| inner.is_ok()))
        .count();

    assert_eq!(successes, 1);
    assert_eq!(
        git_output(
            &fixture.repository,
            &[
                "rev-list",
                "--count",
                &format!("{}..HEAD", fixture.run.base_commit)
            ]
        ),
        "1"
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn unsafe_sqlite_compensation_blocks_without_overwriting_external_change() {
    let fixture = DesignFixture::new("unsafe-compensation").await;
    inject_design_transaction_failure(&fixture.store).await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let session_id = fixture.session_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .update_design(&session_id, &repository, DESIGN_PATCH)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        barrier.wait_until_committed(),
    )
    .await
    .unwrap();
    std::fs::write(
        fixture.repository.join("design/spec.md"),
        "external-after-commit\n",
    )
    .unwrap();
    barrier.release().await;

    assert!(update.await.unwrap().is_err());
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.phase, TaskRunPhase::Blocked);
    assert_eq!(fixture.design_text(), "external-after-commit\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn no_source_cancel_creates_revert_and_atomically_advances_heads() {
    let fixture = DesignFixture::new("cancel-revert").await;
    let design = fixture.update(DESIGN_PATCH).await.unwrap();

    let reverted = fixture
        .coordinator
        .revert_design_for_no_source_cancel(&fixture.run.id)
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    let lease = fixture
        .store
        .read_branch_lease(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reverted.previous_head, design.design_commit);
    assert_ne!(reverted.revert_commit, reverted.previous_head);
    assert_eq!(run.expected_head, reverted.revert_commit);
    assert_eq!(lease.expected_head, run.expected_head);
    git(
        &fixture.repository,
        &[
            "diff",
            "--exit-code",
            &fixture.run.base_commit,
            "HEAD",
            "--",
            "design/spec.md",
        ],
    );
    assert_eq!(
        git_output(
            &fixture.repository,
            &["show", "-s", "--format=%P", &reverted.revert_commit]
        ),
        design.design_commit
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn accepted_source_merge_requires_a_final_design_consistency_commit() {
    let fixture = DesignFixture::new("source-merge-consistency").await;
    let design = fixture.update(DESIGN_PATCH).await.unwrap();
    std::fs::write(fixture.repository.join("source.rs"), "source\n").unwrap();
    git(&fixture.repository, &["add", "source.rs"]);
    git(
        &fixture.repository,
        &["commit", "-m", "merge accepted source"],
    );
    let merged_head = fixture.head();
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.run.id, &design.design_commit, &merged_head)
            .await
            .unwrap()
    );
    let merge = fixture
        .store
        .create_merge_record(CreateMergeRecord {
            task_run_id: fixture.run.id.clone(),
            agent_id: "agent-source".to_string(),
            expected_head: design.design_commit,
            source_commit: merged_head.clone(),
            conflict_files: Vec::new(),
        })
        .await
        .unwrap();
    fixture
        .store
        .update_merge_record(
            &merge.id,
            UpdateMergeRecord {
                status: MergeStatus::Merged,
                resolution_summary: Some("accepted".to_string()),
                verification: Some(vec!["cargo test".to_string()]),
                attempt: 1,
            },
        )
        .await
        .unwrap();

    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!design_commit_is_current(&run));

    let consistency = fixture
        .update(&replace_patch("after", "after-source-merge"))
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert!(design_commit_is_current(&run));
    assert_eq!(
        run.design_commit.as_deref(),
        Some(consistency.design_commit.as_str())
    );
    fixture.cleanup().await;
}

struct DesignFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    session_id: String,
    run: TaskRunRecord,
}

impl DesignFixture {
    async fn new(name: &str) -> Self {
        Self::new_with_plan(name, "plan").await
    }

    async fn new_with_plan(name: &str, plan: &str) -> Self {
        let repository = init_repository(name);
        std::fs::create_dir_all(repository.join("design")).unwrap();
        std::fs::write(repository.join("design/spec.md"), "before\n").unwrap();
        std::fs::write(repository.join(".gitignore"), "design/ignored.md\n").unwrap();
        git(&repository, &["add", "design/spec.md", ".gitignore"]);
        git(&repository, &["commit", "-m", "initial design"]);
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(&repository).await.unwrap();
        let session = store
            .create_session(&project.id, "Task", StudioMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, plan, &repository)
            .await
            .unwrap();
        Self {
            repository,
            store,
            coordinator,
            session_id: session.id,
            run,
        }
    }

    async fn update(&self, patch: &str) -> anyhow::Result<DesignUpdateOutput> {
        self.coordinator
            .update_design(&self.session_id, &self.repository, patch)
            .await
    }

    fn head(&self) -> String {
        git_output(&self.repository, &["rev-parse", "HEAD"])
    }

    fn status(&self) -> String {
        git_output(
            &self.repository,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
    }

    fn design_text(&self) -> String {
        std::fs::read_to_string(self.repository.join("design/spec.md")).unwrap()
    }

    async fn cleanup(self) {
        self.coordinator.suspend();
        let _ = std::fs::remove_dir_all(self.repository);
    }
}

fn allocation(session_id: &str, suffix: &str) -> AllocateExecutor {
    AllocateExecutor {
        session_id: session_id.to_string(),
        title: suffix.to_string(),
        owned_paths: vec![format!("src/{suffix}.rs")],
        agent_id: format!("agent-{suffix}"),
        owner_path: "/root".to_string(),
        requested_by_call_id: format!("call-{suffix}"),
    }
}

fn replace_patch(before: &str, after: &str) -> String {
    format!(
        "*** Begin Patch\n*** Update File: design/spec.md\n@@\n-{before}\n+{after}\n*** End Patch"
    )
}

async fn inject_design_transaction_failure(store: &StudioStore) {
    store
        .execute_test_sql(
            "CREATE TRIGGER fail_design_update BEFORE UPDATE OF design_commit ON task_runs \
             BEGIN SELECT RAISE(FAIL, 'injected design transaction failure'); END;",
        )
        .await;
}

fn init_repository(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "pure-task-design-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init"]);
    git(&path, &["checkout", "-b", "main"]);
    git(&path, &["config", "user.email", "pure@example.invalid"]);
    git(&path, &["config", "user.name", "Pure Test"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-m", "initial"]);
    path
}

fn git(repository: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {} failed", args.join(" "));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
