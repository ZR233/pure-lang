use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::*;
use crate::studio::task_coordinator::AllocateExecutor;
use crate::{StudioMode, StudioStore};

#[tokio::test]
async fn ordinary_workspace_edits_are_committed_and_atomically_open_executor_gate() {
    let fixture = DesignFixture::new("success").await;
    let before = fixture.run.expected_head.clone();

    let denied = fixture
        .store
        .allocate_executor(allocation(&fixture.thread_id, "before-design"))
        .await;
    let denied = match denied {
        Ok(_) => panic!("executor allocation must remain gated before durable design"),
        Err(error) => error,
    };
    assert!(denied.to_string().contains("requires task phase"));

    std::fs::write(fixture.repository.join("design/spec.md"), "after\n").unwrap();
    let generated = Command::new("sh")
        .current_dir(&fixture.repository)
        .args(["-c", "printf 'generated\\n' > source.rs"])
        .status()
        .unwrap();
    assert!(generated.success());
    fixture.observe("call-command-generated-files").await;
    let output = fixture
        .finalize("updated design and generated source")
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

    assert_eq!(output.task_run_id, fixture.run.id);
    assert_eq!(output.previous_head, before);
    assert_eq!(
        output.changed_files,
        vec!["design/spec.md".to_string(), "source.rs".to_string()]
    );
    assert_eq!(run.kind(), TaskRunStateKind::Implementing);
    assert_eq!(run.design_phase_commit(), output.phase_commit.as_deref());
    assert_eq!(
        run.design_summary(),
        Some("updated design and generated source")
    );
    assert_eq!(run.expected_head, output.finalized_head);
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
        .allocate_executor(allocation(&fixture.thread_id, "after-design"))
        .await
        .expect("executor allocation should open immediately after durable design");
    fixture.cleanup().await;
}

#[tokio::test]
async fn clean_workspace_finalizes_without_creating_a_commit() {
    let fixture = DesignFixture::new("clean").await;
    let before = fixture.head();
    let output = fixture
        .finalize("no workspace edits were needed")
        .await
        .unwrap();
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(output.previous_head, before);
    assert_eq!(output.finalized_head, before);
    assert_eq!(output.phase_commit, None);
    assert!(output.changed_files.is_empty());
    assert_eq!(run.kind(), TaskRunStateKind::Implementing);
    assert_eq!(run.design_finalized_head(), Some(before.as_str()));
    assert_eq!(run.design_phase_commit(), None);
    assert_eq!(fixture.head(), before);
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn concurrent_finalize_calls_serialize_to_one_commit_and_one_cas() {
    let fixture = DesignFixture::new("concurrent").await;
    std::fs::write(fixture.repository.join("design/spec.md"), "after\n").unwrap();
    fixture.observe("call-concurrent-edit").await;
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let mut calls = Vec::new();
    for _ in 0..2 {
        let coordinator = fixture.coordinator.clone();
        let thread_id = fixture.thread_id.clone();
        let repository = fixture.repository.clone();
        let start = start.clone();
        calls.push(tokio::spawn(async move {
            start.wait().await;
            coordinator
                .finalize_design(&thread_id, &repository, "concurrent finalize")
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
    std::fs::write(fixture.repository.join("design/spec.md"), "after\n").unwrap();
    fixture.observe("call-unsafe-compensation-edit").await;
    inject_design_transaction_failure(&fixture.store).await;
    let barrier = DesignCommitTestBarrier::new();
    fixture
        .coordinator
        .set_design_after_commit_barrier(barrier.clone());
    let coordinator = fixture.coordinator.clone();
    let thread_id = fixture.thread_id.clone();
    let repository = fixture.repository.clone();
    let update = tokio::spawn(async move {
        coordinator
            .finalize_design(&thread_id, &repository, "unsafe compensation")
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
    assert_eq!(run.kind(), TaskRunStateKind::Blocked);
    assert_eq!(fixture.design_text(), "external-after-commit\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn no_source_cancel_creates_revert_and_atomically_advances_heads() {
    let fixture = DesignFixture::new("cancel-revert").await;
    std::fs::write(fixture.repository.join("design/spec.md"), "after\n").unwrap();
    fixture.observe("call-cancel-edit").await;
    let design = fixture
        .finalize("design before cancellation")
        .await
        .unwrap();
    let design_commit = design.phase_commit.clone().unwrap();

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

    assert_eq!(reverted.previous_head, design_commit);
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
        design_commit
    );
    assert!(fixture.status().is_empty());
    fixture.cleanup().await;
}

#[tokio::test]
async fn finalize_rejects_unobserved_external_edits_without_touching_the_draft() {
    let fixture = DesignFixture::new("unobserved-external-edit").await;
    std::fs::write(fixture.repository.join("design/spec.md"), "external\n").unwrap();

    let error = fixture
        .finalize("must reject an edit outside the observation chain")
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("outside the Task tool observation chain")
    );
    let run = fixture
        .store
        .read_task_run(&fixture.run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.kind(), TaskRunStateKind::DesignUpdating);
    assert_eq!(fixture.design_text(), "external\n");
    assert_ne!(fixture.status(), "");
    fixture.cleanup().await;
}

struct DesignFixture {
    repository: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    thread_id: String,
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
            .create_thread(&project.id, "Task", StudioMode::Task)
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
            thread_id: session.id,
            run,
        }
    }

    async fn finalize(&self, summary: &str) -> anyhow::Result<DesignFinalizeOutput> {
        self.coordinator
            .finalize_design(&self.thread_id, &self.repository, summary)
            .await
    }

    async fn observe(&self, tool_call_id: &str) {
        let run = self
            .store
            .read_task_run(&self.run.id)
            .await
            .unwrap()
            .unwrap();
        let fingerprint =
            fingerprint_repository(&self.repository, &run.base_commit, &run.expected_head)
                .await
                .unwrap();
        assert!(
            self.store
                .record_task_design_observation(
                    &run.id,
                    "turn-design-test",
                    tool_call_id,
                    fingerprint,
                )
                .await
                .unwrap()
        );
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

fn allocation(thread_id: &str, suffix: &str) -> AllocateExecutor {
    AllocateExecutor {
        thread_id: thread_id.to_string(),
        title: suffix.to_string(),
        scope_hints: vec![format!("src/{suffix}.rs")],
        agent_id: format!("agent-{suffix}"),
        requested_by_call_id: format!("call-{suffix}"),
    }
}

async fn inject_design_transaction_failure(store: &StudioStore) {
    store
        .execute_test_sql(
            "CREATE TRIGGER fail_design_update BEFORE UPDATE OF state_json ON task_runs \
             WHEN json_extract(NEW.state_json, '$.kind') = 'implementing' \
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
