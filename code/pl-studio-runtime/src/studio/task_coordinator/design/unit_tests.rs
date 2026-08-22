use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::*;
use crate::studio::task_coordinator::AllocateExecutor;
use crate::{StudioMode, StudioStore};

#[tokio::test]
async fn dirty_workspace_finalizes_without_reading_or_modifying_git() {
    let repository = init_repository("dirty");
    std::fs::write(repository.join("README.md"), "unstaged\n").unwrap();
    std::fs::write(repository.join("staged.rs"), "pub fn staged() {}\n").unwrap();
    git(&repository, &["add", "staged.rs"]);
    std::fs::write(repository.join("untracked.rs"), "pub fn untracked() {}\n").unwrap();
    let head_before = git_output(&repository, &["rev-parse", "HEAD"]);
    let status_before = git_output(
        &repository,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );
    let fixture = DesignFixture::new(repository.clone()).await;

    let output = fixture
        .coordinator
        .finalize_design(&fixture.thread_id, &repository, "keep caller-owned changes")
        .await
        .unwrap();
    let run = fixture.store.read_task_run(&fixture.run_id).await.unwrap().unwrap();

    assert_eq!(output.task_run_id, fixture.run_id);
    assert_eq!(output.state, TaskRunStateKind::Implementing);
    assert_eq!(output.summary, "keep caller-owned changes");
    assert_eq!(run.design_summary(), Some("keep caller-owned changes"));
    assert_eq!(git_output(&repository, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_output(
            &repository,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ),
        status_before
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn non_git_workspace_can_confirm_and_finalize() {
    let workspace = unique_path("non-git");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("draft.txt"), "draft\n").unwrap();
    let fixture = DesignFixture::new(workspace.clone()).await;

    let output = fixture
        .coordinator
        .finalize_design(&fixture.thread_id, &workspace, "non-git design")
        .await
        .unwrap();

    assert_eq!(output.state, TaskRunStateKind::Implementing);
    assert_eq!(std::fs::read_to_string(workspace.join("draft.txt")).unwrap(), "draft\n");
    fixture.cleanup().await;
}

#[tokio::test]
async fn missing_workspace_can_confirm_and_finalize() {
    let workspace = unique_path("missing");
    let fixture = DesignFixture::new(workspace.clone()).await;
    assert!(!workspace.exists());

    let output = fixture
        .coordinator
        .finalize_design(&fixture.thread_id, &workspace, "missing workspace deferred")
        .await
        .unwrap();

    assert_eq!(output.state, TaskRunStateKind::Implementing);
    assert!(!workspace.exists());
    fixture.cleanup().await;
}

#[tokio::test]
async fn executor_gate_opens_only_after_finalize() {
    let workspace = unique_path("gate");
    let fixture = DesignFixture::new(workspace.clone()).await;
    let denied = fixture
        .store
        .allocate_executor(allocation(&fixture.thread_id, "before"))
        .await
        .unwrap_err();
    assert!(denied.to_string().contains("requires task phase"));

    fixture
        .coordinator
        .finalize_design(&fixture.thread_id, &workspace, "open executor gate")
        .await
        .unwrap();
    fixture
        .store
        .allocate_executor(allocation(&fixture.thread_id, "after"))
        .await
        .unwrap();
    fixture.cleanup().await;
}

#[tokio::test]
async fn concurrent_finalize_calls_have_one_revision_cas_winner() {
    let workspace = unique_path("concurrent");
    let fixture = DesignFixture::new(workspace.clone()).await;
    let start = Arc::new(tokio::sync::Barrier::new(3));
    let mut calls = Vec::new();
    for _ in 0..2 {
        let coordinator = fixture.coordinator.clone();
        let thread_id = fixture.thread_id.clone();
        let workspace = workspace.clone();
        let start = start.clone();
        calls.push(tokio::spawn(async move {
            start.wait().await;
            coordinator
                .finalize_design(&thread_id, &workspace, "concurrent finalize")
                .await
        }));
    }
    start.wait().await;
    let results = futures::future::join_all(calls).await;
    assert_eq!(
        results
            .iter()
            .filter(|result| result.as_ref().is_ok_and(|inner| inner.is_ok()))
            .count(),
        1
    );
    fixture.cleanup().await;
}

#[tokio::test]
async fn empty_summary_is_rejected_without_advancing_state() {
    let workspace = unique_path("empty-summary");
    let fixture = DesignFixture::new(workspace.clone()).await;
    let error = fixture
        .coordinator
        .finalize_design(&fixture.thread_id, &workspace, "  ")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("must not be empty"));
    let run = fixture.store.read_task_run(&fixture.run_id).await.unwrap().unwrap();
    assert_eq!(run.kind(), TaskRunStateKind::DesignUpdating);
    fixture.cleanup().await;
}

struct DesignFixture {
    workspace: PathBuf,
    store: StudioStore,
    coordinator: Arc<TaskCoordinator>,
    thread_id: String,
    run_id: String,
}

impl DesignFixture {
    async fn new(workspace: PathBuf) -> Self {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project(&workspace).await.unwrap();
        let thread = store
            .create_thread(&project.id, "Task", StudioMode::Task)
            .await
            .unwrap();
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&thread.id, "confirmed plan", &workspace)
            .await
            .unwrap();
        Self {
            workspace,
            store,
            coordinator,
            thread_id: thread.id,
            run_id: run.id,
        }
    }

    async fn cleanup(self) {
        self.coordinator.suspend();
        if self.workspace.exists() {
            let _ = std::fs::remove_dir_all(self.workspace);
        }
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

fn init_repository(name: &str) -> PathBuf {
    let path = unique_path(name);
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

fn unique_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "pure-task-design-{name}-{}-{stamp}",
        std::process::id()
    ))
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
