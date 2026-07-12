//! 冲突现场持久化与重启恢复回归。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    auto_merged_entries, capture_conflict_workspace_evidence, parse_unmerged_entries,
    validate_conflict_status_scope, validate_conflict_workspace_evidence,
};
use crate::studio::task_coordinator::{ConflictEntry, ConflictKind, ConflictManifest};

#[test]
fn conflict_creation_rejects_untracked_and_unrelated_paths() {
    for (drift, expected) in [
        (Drift::Untracked, "untracked"),
        (Drift::UnrelatedTracked, "unrelated"),
    ] {
        let fixture = ConflictRepository::new(drift.name());
        drift.apply(&fixture.repository);
        let status = fixture.status();
        let allowed = HashSet::from(["shared.txt".to_string(), "auto.txt".to_string()]);

        let error = validate_conflict_status_scope(&status, &allowed).unwrap_err();

        let detail = error.to_string();
        assert!(
            detail.contains(expected),
            "{} produced unexpected scope error: {detail}",
            drift.name()
        );
    }
}

#[tokio::test]
async fn conflict_recovery_rejects_every_external_workspace_drift() {
    for drift in [
        Drift::ConflictWorktree,
        Drift::AutoMergedWorktree,
        Drift::NewTracked,
        Drift::UnrelatedTracked,
        Drift::Untracked,
    ] {
        let fixture = ConflictRepository::new(drift.name());
        let manifest = fixture.manifest().await;
        assert!(
            manifest
                .auto_merged_entries
                .iter()
                .any(|entry| entry.path == "auto.txt")
        );
        validate_conflict_workspace_evidence(&fixture.repository, &manifest)
            .await
            .unwrap();

        drift.apply(&fixture.repository);
        let status_before_validation = fixture.status();
        let index_before_validation = fixture.index();

        let error = validate_conflict_workspace_evidence(&fixture.repository, &manifest)
            .await
            .expect_err("external drift must invalidate durable conflict evidence");
        assert!(
            error.to_string().contains("drifted") || error.to_string().contains("untracked"),
            "{} produced unexpected error: {error:#}",
            drift.name()
        );
        assert!(
            fixture.merge_head_exists(),
            "validation must preserve MERGE_HEAD"
        );
        assert_eq!(fixture.status(), status_before_validation);
        assert_eq!(fixture.index(), index_before_validation);
    }
}

#[derive(Clone, Copy)]
enum Drift {
    ConflictWorktree,
    AutoMergedWorktree,
    NewTracked,
    UnrelatedTracked,
    Untracked,
}

impl Drift {
    fn name(self) -> &'static str {
        match self {
            Self::ConflictWorktree => "conflict-worktree",
            Self::AutoMergedWorktree => "auto-worktree",
            Self::NewTracked => "new-tracked",
            Self::UnrelatedTracked => "unrelated-tracked",
            Self::Untracked => "untracked",
        }
    }

    fn apply(self, repository: &Path) {
        match self {
            Self::ConflictWorktree => {
                std::fs::write(repository.join("shared.txt"), "manually edited conflict\n")
                    .unwrap();
            }
            Self::AutoMergedWorktree => {
                std::fs::write(repository.join("auto.txt"), "edited auto merge\n").unwrap();
            }
            Self::NewTracked => {
                std::fs::write(repository.join("new-tracked.txt"), "new tracked\n").unwrap();
                git(repository, &["add", "new-tracked.txt"], true);
            }
            Self::UnrelatedTracked => {
                std::fs::write(repository.join("stable.txt"), "external edit\n").unwrap();
            }
            Self::Untracked => {
                std::fs::write(repository.join("untracked.txt"), "external edit\n").unwrap();
            }
        }
    }
}

struct ConflictRepository {
    temp: PathBuf,
    repository: PathBuf,
}

impl ConflictRepository {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir().join(format!(
            "pure-task-conflict-{label}-{}-{nonce}",
            std::process::id()
        ));
        let repository = temp.join("repository");
        std::fs::create_dir_all(&repository).unwrap();
        git(&repository, &["init", "-b", "main"], true);
        git(&repository, &["config", "user.name", "Pure Test"], true);
        git(
            &repository,
            &["config", "user.email", "pure@example.invalid"],
            true,
        );
        std::fs::write(repository.join("shared.txt"), "base\n").unwrap();
        std::fs::write(repository.join("auto.txt"), "base auto\n").unwrap();
        std::fs::write(repository.join("stable.txt"), "stable\n").unwrap();
        git(&repository, &["add", "."], true);
        git(&repository, &["commit", "-m", "base"], true);
        git(&repository, &["checkout", "-b", "executor"], true);
        std::fs::write(repository.join("shared.txt"), "executor\n").unwrap();
        std::fs::write(repository.join("auto.txt"), "executor auto\n").unwrap();
        git(&repository, &["add", "."], true);
        git(&repository, &["commit", "-m", "executor"], true);
        git(&repository, &["checkout", "main"], true);
        std::fs::write(repository.join("shared.txt"), "planner\n").unwrap();
        git(&repository, &["add", "shared.txt"], true);
        git(&repository, &["commit", "-m", "planner"], true);
        git(
            &repository,
            &["merge", "--no-ff", "--no-commit", "executor"],
            false,
        );
        assert!(repository.join("shared.txt").exists());
        Self { temp, repository }
    }

    async fn manifest(&self) -> ConflictManifest {
        let unmerged = git_bytes(&self.repository, &["ls-files", "-u", "-z"], true);
        let grouped = parse_unmerged_entries(&unmerged).unwrap();
        let mut conflicts = grouped
            .into_iter()
            .map(|(path, stages)| ConflictEntry {
                path,
                kind: ConflictKind::Text,
                stages,
                worktree_object_id: None,
                binary: false,
                rename_source: None,
                rename_destination: None,
            })
            .collect::<Vec<_>>();
        let evidence = capture_conflict_workspace_evidence(&self.repository, &mut conflicts)
            .await
            .unwrap();
        let auto_merged_entries = auto_merged_entries(&self.repository, &conflicts)
            .await
            .unwrap();
        ConflictManifest {
            merge_head: git_text(&self.repository, &["rev-parse", "MERGE_HEAD"]),
            merge_base: String::new(),
            pre_index_tree: String::new(),
            conflicts,
            status_porcelain_v1_z: evidence.status_porcelain_v1_z,
            index_stage_zero_entries: evidence.index_stage_zero_entries,
            auto_merged_entries,
        }
    }

    fn status(&self) -> Vec<u8> {
        git_bytes(
            &self.repository,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            true,
        )
    }

    fn merge_head_exists(&self) -> bool {
        Command::new("git")
            .current_dir(&self.repository)
            .args(["rev-parse", "--verify", "MERGE_HEAD"])
            .output()
            .unwrap()
            .status
            .success()
    }

    fn index(&self) -> Vec<u8> {
        git_bytes(&self.repository, &["ls-files", "--stage", "-z"], true)
    }
}

impl Drop for ConflictRepository {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .current_dir(&self.repository)
            .args(["merge", "--abort"])
            .output();
        let _ = std::fs::remove_dir_all(&self.temp);
    }
}

fn git(repository: &Path, args: &[&str], expect_success: bool) {
    let output = Command::new("git")
        .current_dir(repository)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        expect_success,
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_bytes(repository: &Path, args: &[&str], expect_success: bool) -> Vec<u8> {
    let output = Command::new("git")
        .current_dir(repository)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(output.status.success(), expect_success, "git {args:?}");
    output.stdout
}

fn git_text(repository: &Path, args: &[&str]) -> String {
    String::from_utf8(git_bytes(repository, args, true))
        .unwrap()
        .trim()
        .to_string()
}
