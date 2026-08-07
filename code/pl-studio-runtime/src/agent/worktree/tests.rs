//! Studio worktree 生命周期测试。

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource, WorktreeBackend,
    WorktreeCreateFailure, WorktreeError, reconcile_task_worktrees,
    set_after_registration_remove_barrier,
};
use super::{WorktreeCreateSpec, WorktreeManager};

/// 临时仓库目录序号，避免并发测试因时间戳碰撞命中同一目录。
static REPO_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 在临时目录初始化一个带初始提交的 git 仓库，返回其路径。
fn temp_git_repo() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = REPO_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pure-worktree-test-{stamp}-{seq}"));
    fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init"]);
    run_git(&dir, &["config", "user.email", "test@pure.local"]);
    run_git(&dir, &["config", "user.name", "pure test"]);
    run_git(&dir, &["config", "commit.gpgsign", "false"]);
    fs::write(dir.join("README.md"), "init\n").unwrap();
    run_git(&dir, &["add", "-A"]);
    run_git(&dir, &["commit", "-m", "init"]);
    dir
}

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .status()
        .expect("git binary is required for worktree tests");
    assert!(
        status.success(),
        "git {:?} failed in {}",
        args,
        cwd.display()
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git binary is required for worktree tests");
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[tokio::test]
async fn create_from_spec_uses_exact_path_branch_and_base_commit() {
    let repo = temp_git_repo();
    let base_commit = git_output(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("later.txt"), "later\n").unwrap();
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "later"]);
    let task_path = repo
        .join(".pure")
        .join("worktrees")
        .join("task-run-1")
        .join("agent-7");
    let manager = WorktreeManager::local(repo.clone());

    let handle = manager
        .create_from_spec(WorktreeCreateSpec {
            repo_root: repo.clone(),
            path: task_path.clone(),
            branch: "pure-task-run-1-agent-7".to_string(),
            base_commit: base_commit.clone(),
        })
        .await
        .unwrap();

    assert_eq!(handle.path, task_path);
    assert_eq!(handle.branch, "pure-task-run-1-agent-7");
    assert_eq!(
        git_output(&handle.path, &["rev-parse", "HEAD"]),
        base_commit
    );
    assert!(!handle.path.join("later.txt").exists());
    manager.discard(&handle).await.unwrap();
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn durable_reconciliation_preserves_owned_leaf_and_cleans_exact_orphan() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let protected = task_worktree_spec(&repo, "run-protected", "agent-protected");
    let orphan = task_worktree_spec(&repo, "run-orphan", "agent-orphan");
    manager.create_from_spec(protected.clone()).await.unwrap();
    manager.create_from_spec(orphan.clone()).await.unwrap();
    let orphan_parent = orphan.path.parent().unwrap().to_path_buf();
    fs::write(orphan_parent.join("audit.txt"), "keep parent").unwrap();

    let first = reconcile_task_worktrees(
        &repo,
        &[DurableWorktreeResource {
            task_run_id: "run-protected".to_string(),
            path: protected.path.clone(),
            branch: protected.branch.clone(),
            expected_head: None,
            presence: DurableWorktreePresence::MustExist,
            disposition: DurableWorktreeDisposition::Protect,
        }],
    )
    .await
    .unwrap();
    let second = reconcile_task_worktrees(
        &repo,
        &[DurableWorktreeResource {
            task_run_id: "run-protected".to_string(),
            path: protected.path.clone(),
            branch: protected.branch.clone(),
            expected_head: None,
            presence: DurableWorktreePresence::MustExist,
            disposition: DurableWorktreeDisposition::Protect,
        }],
    )
    .await
    .unwrap();

    assert!(protected.path.is_dir());
    assert!(!orphan.path.exists());
    assert!(orphan_parent.join("audit.txt").exists());
    assert_eq!(first.cleaned_registrations, 1);
    assert_eq!(second.cleaned_registrations, 0);
    assert!(git_output(&repo, &["branch", "--list", &orphan.branch]).is_empty());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn durable_reconciliation_rejects_partial_missing_without_cleanup() {
    let repo = temp_git_repo();
    let spec = task_worktree_spec(&repo, "run-partial", "agent-partial");
    run_git(&repo, &["branch", &spec.branch]);

    let error = reconcile_task_worktrees(
        &repo,
        &[DurableWorktreeResource {
            task_run_id: "run-partial".to_string(),
            path: spec.path.clone(),
            branch: spec.branch.clone(),
            expected_head: None,
            presence: DurableWorktreePresence::MustExist,
            disposition: DurableWorktreeDisposition::Protect,
        }],
    )
    .await
    .expect_err("a durable owner with only a branch must block reconciliation");

    assert!(error.to_string().contains("run-partial"));
    assert!(!git_output(&repo, &["branch", "--list", &spec.branch]).is_empty());
    assert!(!spec.path.exists());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn fallback_delete_revalidates_after_registration_remove_race() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let orphan = task_worktree_spec(&repo, "run-race", "agent-race");
    manager.create_from_spec(orphan.clone()).await.unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let external = std::env::temp_dir().join(format!(
        "pure-worktree-race-external-{}-{stamp}-{}",
        std::process::id(),
        REPO_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let external_leaf = external.join("agent-race");
    fs::create_dir_all(&external_leaf).unwrap();
    fs::write(external_leaf.join("keep.txt"), "keep").unwrap();
    let barrier = Arc::new(Barrier::new(2));
    set_after_registration_remove_barrier(orphan.path.clone(), barrier.clone());

    let reconcile_repo = repo.clone();
    let reconcile =
        tokio::spawn(async move { reconcile_task_worktrees(&reconcile_repo, &[]).await });
    let run_parent = orphan.path.parent().unwrap().to_path_buf();
    let race = tokio::task::spawn_blocking(move || {
        barrier.wait();
        if run_parent.exists() {
            fs::remove_dir(&run_parent).unwrap();
        }
        create_directory_link(&external, &run_parent);
        barrier.wait();
        (external, run_parent)
    });
    let (external, linked_parent) = race.await.unwrap();
    let error = reconcile
        .await
        .unwrap()
        .expect_err("fallback delete must revalidate");

    let error = format!("{error:#}");
    assert!(
        error.contains("link") || error.contains("reparse"),
        "unexpected fallback safety error: {error}"
    );
    assert_eq!(
        fs::read_to_string(external_leaf.join("keep.txt")).unwrap(),
        "keep"
    );
    fs::remove_dir(&linked_parent).ok();
    fs::remove_dir_all(repo).ok();
    fs::remove_dir_all(external).ok();
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link)
        .expect("directory symlink support is required for worktree safety tests");
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn task_worktree_spec(repo: &Path, run_id: &str, agent_id: &str) -> WorktreeCreateSpec {
    WorktreeCreateSpec {
        repo_root: repo.to_path_buf(),
        path: repo
            .join(".pure")
            .join("worktrees")
            .join(run_id)
            .join(agent_id),
        branch: format!("pure-task-{run_id}-{agent_id}"),
        base_commit: "HEAD".to_string(),
    }
}

#[tokio::test]
async fn create_failure_preserves_preexisting_branch() {
    let repo = temp_git_repo();
    let branch = "pure-agent-preexisting";
    let original_head = git_output(&repo, &["rev-parse", "HEAD"]);
    run_git(&repo, &["branch", branch, &original_head]);
    let manager = WorktreeManager::local(repo.clone());

    manager
        .create_from_spec(WorktreeCreateSpec {
            repo_root: repo.clone(),
            path: repo.join(".pure/worktrees/new-target"),
            branch: branch.to_string(),
            base_commit: original_head.clone(),
        })
        .await
        .expect_err("an existing branch must reject create");

    assert_eq!(git_output(&repo, &["branch", "--list", branch]), branch);
    assert_eq!(git_output(&repo, &["rev-parse", branch]), original_head);
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn discard_removes_worktree_and_branch() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-1").await.unwrap();
    manager.discard(&handle).await.unwrap();
    assert!(!handle.path.exists());
    fs::remove_dir_all(repo).ok();
}

#[derive(Debug)]
struct RemoveReportsFailureAfterDeletingLeaf;

impl WorktreeBackend for RemoveReportsFailureAfterDeletingLeaf {
    fn create<'a>(
        &'a self,
        _repo_root: &'a Path,
        _branch: &'a str,
        _target_path: &'a Path,
        _base_commit: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorktreeCreateFailure>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn remove<'a>(
        &'a self,
        _repo_root: &'a Path,
        target_path: &'a Path,
        _force: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorktreeError>> + Send + 'a>> {
        Box::pin(async move {
            tokio::fs::remove_dir_all(target_path).await.unwrap();
            Err(WorktreeError::GitCommand {
                args: "worktree remove --force".to_string(),
                stderr: "Filename too long".to_string(),
            })
        })
    }

    fn delete_branch<'a>(
        &'a self,
        _repo_root: &'a Path,
        _branch: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorktreeError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn discard_uses_final_resource_state_after_partial_git_remove_failure() {
    let repo = temp_git_repo();
    let handle = super::WorktreeHandle {
        path: repo.join(".pure/worktrees/task-run/agent"),
        branch: "pure-task-task-run-agent".to_string(),
    };
    fs::create_dir_all(&handle.path).unwrap();
    let manager = WorktreeManager::with_backend(
        repo.clone(),
        Arc::new(RemoveReportsFailureAfterDeletingLeaf),
    );

    manager.discard(&handle).await.unwrap();

    assert!(!handle.path.exists());
    fs::remove_dir_all(repo).ok();
}

#[cfg(windows)]
#[tokio::test]
async fn discard_cleans_a_worktree_containing_windows_long_paths() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-long-path").await.unwrap();
    let component = "long-path-component-0123456789-0123456789-0123456789";
    let nested = handle
        .path
        .join(component)
        .join(component)
        .join(component)
        .join(component);
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("artifact.txt"), "long path fixture").unwrap();
    assert!(nested.as_os_str().len() > 260);

    manager.discard(&handle).await.unwrap();

    assert!(!handle.path.exists());
    assert!(git_output(&repo, &["branch", "--list", &handle.branch]).is_empty());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn enable_never_scans_or_cleans_orphan_worktrees() {
    let repo = temp_git_repo();
    let orphan = repo.join(".pure").join("worktrees").join("orphan");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("junk.txt"), "junk").unwrap();
    let manager = WorktreeManager::disabled();
    manager.enable(repo.clone()).await;
    assert!(orphan.exists());
    fs::remove_dir_all(repo).ok();
}
