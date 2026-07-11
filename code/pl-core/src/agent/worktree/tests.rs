use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::backend::BoxFuture;
use super::{
    CloseDisposition, CloseOutcome, MergeOutcome, WorktreeBackend, WorktreeCreateFailure,
    WorktreeCreateSpec, WorktreeError, WorktreeHandle, WorktreeManager,
};

#[derive(Debug)]
struct FailingCleanupBackend {
    calls: Arc<Mutex<Vec<String>>>,
    create_fails: bool,
}

impl WorktreeBackend for FailingCleanupBackend {
    fn create<'a>(
        &'a self,
        _repo_root: &'a Path,
        _branch: &'a str,
        _target_path: &'a Path,
        _base_commit: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeCreateFailure>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push("create".to_string());
            if self.create_fails {
                Err(WorktreeCreateFailure::may_have_created(git_error(
                    "worktree add",
                    "create failed after partial setup",
                )))
            } else {
                Ok(())
            }
        })
    }

    fn remove<'a>(
        &'a self,
        _repo_root: &'a Path,
        _target_path: &'a Path,
        force: bool,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("remove:{force}"));
            Err(git_error("worktree remove", "remove cleanup failed"))
        })
    }

    fn delete_branch<'a>(
        &'a self,
        _repo_root: &'a Path,
        _branch: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async move {
            self.calls.lock().unwrap().push("delete_branch".to_string());
            Err(git_error("branch -D", "branch cleanup failed"))
        })
    }

    fn commit_all<'a>(
        &'a self,
        _worktree_path: &'a Path,
        _message: &'a str,
    ) -> BoxFuture<'a, Result<(), WorktreeError>> {
        Box::pin(async { Ok(()) })
    }

    fn merge_branch<'a>(
        &'a self,
        _main_workspace: &'a Path,
        _branch: &'a str,
    ) -> BoxFuture<'a, Result<MergeOutcome, WorktreeError>> {
        Box::pin(async { Ok(MergeOutcome::Merged) })
    }
}

fn git_error(args: &str, stderr: &str) -> WorktreeError {
    WorktreeError::GitCommand {
        args: args.to_string(),
        stderr: stderr.to_string(),
    }
}

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
async fn disabled_manager_create_returns_disabled() {
    let manager = WorktreeManager::disabled();
    let result = manager.create("agent-1").await;
    assert!(matches!(result, Err(WorktreeError::Disabled)));
}

#[tokio::test]
async fn create_adds_worktree_on_new_branch() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-1").await.unwrap();
    assert!(handle.path.is_dir());
    assert_eq!(handle.branch, "pure-agent-agent-1");
    // worktree checkout 出主仓库的初始文件。
    assert!(handle.path.join("README.md").exists());
    fs::remove_dir_all(repo).ok();
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
    manager
        .close(&handle, CloseDisposition::Discard)
        .await
        .unwrap();
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn create_failure_reports_all_cleanup_failures() {
    let repo = std::env::temp_dir().join("pure-worktree-create-rollback");
    let path = repo.join(".pure/worktrees/agent-1");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let manager = WorktreeManager::with_backend(
        repo.clone(),
        Arc::new(FailingCleanupBackend {
            calls: Arc::clone(&calls),
            create_fails: true,
        }),
    );

    let error = manager
        .create_from_spec(WorktreeCreateSpec {
            repo_root: repo.clone(),
            path,
            branch: "pure-agent-agent-1".to_string(),
            base_commit: "HEAD".to_string(),
        })
        .await
        .expect_err("partial create must report rollback failures");

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["create", "remove:true", "delete_branch"]
    );
    let error = error.to_string();
    for expected in [
        "create failed after partial setup",
        "remove cleanup failed",
        "branch cleanup failed",
    ] {
        assert!(
            error.contains(expected),
            "missing `{expected}` in `{error}`"
        );
    }
    fs::remove_dir_all(repo).ok();
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
async fn create_failure_preserves_preexisting_target_worktree() {
    let repo = temp_git_repo();
    let target = repo.join(".pure/worktrees/existing-target");
    let target_text = target.to_string_lossy().to_string();
    run_git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "pure-agent-existing-target",
            &target_text,
            "HEAD",
        ],
    );
    let original_head = git_output(&target, &["rev-parse", "HEAD"]);
    let manager = WorktreeManager::local(repo.clone());

    manager
        .create_from_spec(WorktreeCreateSpec {
            repo_root: repo.clone(),
            path: target.clone(),
            branch: "pure-agent-new-target".to_string(),
            base_commit: original_head.clone(),
        })
        .await
        .expect_err("an existing target worktree must reject create");

    assert!(target.is_dir(), "preexisting worktree was removed");
    assert_eq!(git_output(&target, &["rev-parse", "HEAD"]), original_head);
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn discard_reports_remove_and_branch_cleanup_failures() {
    let repo = std::env::temp_dir().join("pure-worktree-discard-rollback");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let manager = WorktreeManager::with_backend(
        repo.clone(),
        Arc::new(FailingCleanupBackend {
            calls: Arc::clone(&calls),
            create_fails: false,
        }),
    );
    let handle = WorktreeHandle {
        path: repo.join(".pure/worktrees/agent-1"),
        branch: "pure-agent-agent-1".to_string(),
    };

    let error = manager
        .close(&handle, CloseDisposition::Discard)
        .await
        .expect_err("discard cleanup failures must be returned");

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        ["remove:true", "delete_branch"]
    );
    let error = error.to_string();
    for expected in ["remove cleanup failed", "branch cleanup failed"] {
        assert!(
            error.contains(expected),
            "missing `{expected}` in `{error}`"
        );
    }
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn close_discard_removes_worktree_and_branch() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-1").await.unwrap();
    manager
        .close(&handle, CloseDisposition::Discard)
        .await
        .unwrap();
    assert!(!handle.path.exists());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn close_merge_merges_into_main_workspace() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-1").await.unwrap();
    fs::write(handle.path.join("feature.txt"), "new feature\n").unwrap();
    let outcome = manager
        .close(
            &handle,
            CloseDisposition::Merge {
                target_branch: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(outcome, CloseOutcome::Merged));
    assert!(repo.join("feature.txt").exists());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn close_merge_conflict_keeps_worktree() {
    let repo = temp_git_repo();
    let manager = WorktreeManager::local(repo.clone());
    let handle = manager.create("agent-1").await.unwrap();
    // worktree 与主仓库分别改同一文件，制造 merge 冲突。
    fs::write(handle.path.join("README.md"), "worktree change\n").unwrap();
    fs::write(repo.join("README.md"), "main change\n").unwrap();
    run_git(&repo, &["add", "-A"]);
    run_git(&repo, &["commit", "-m", "main change"]);
    let result = manager
        .close(
            &handle,
            CloseDisposition::Merge {
                target_branch: None,
            },
        )
        .await;
    assert!(matches!(result, Err(WorktreeError::MergeConflict { .. })));
    // 冲突时 worktree 保留，调用方可重试或改 discard。
    assert!(handle.path.exists());
    fs::remove_dir_all(repo).ok();
}

#[tokio::test]
async fn enable_cleans_orphan_worktrees() {
    let repo = temp_git_repo();
    let orphan = repo.join(".pure").join("worktrees").join("orphan");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("junk.txt"), "junk").unwrap();
    let manager = WorktreeManager::disabled();
    // 首次 enable 会扫描并清理 .pure/worktrees/ 下的残留目录。
    manager.enable(repo.clone()).await;
    assert!(!orphan.exists());
    fs::remove_dir_all(repo).ok();
}
