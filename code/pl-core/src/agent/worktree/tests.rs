use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{CloseDisposition, CloseOutcome, WorktreeError, WorktreeManager};

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
