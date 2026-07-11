use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const WORKTREE_ROOT: &str = ".pure/worktrees";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableWorktreeDisposition {
    Protect,
    Cleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurableWorktreeResource {
    pub(crate) task_run_id: String,
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
    pub(crate) expected_head: Option<String>,
    pub(crate) disposition: DurableWorktreeDisposition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorktreeReconciliation {
    pub(crate) cleaned_registrations: usize,
    pub(crate) cleaned_paths: usize,
    pub(crate) cleaned_branches: usize,
}

pub(crate) async fn reconcile_task_worktrees(
    repository: impl AsRef<Path>,
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let repository = repository.as_ref().to_path_buf();
    let durable = durable.to_vec();
    tokio::task::spawn_blocking(move || reconcile_blocking(&repository, &durable))
        .await
        .context("worktree reconciliation task failed")?
}

fn reconcile_blocking(
    repository: &Path,
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let inventory = inventory(repository)?;
    let worktree_root = normalize_path(&repository.join(WORKTREE_ROOT));
    validate_durable(repository, durable, &inventory, &worktree_root)?;

    let protected_paths = durable
        .iter()
        .filter(|resource| resource.disposition == DurableWorktreeDisposition::Protect)
        .map(|resource| normalize_path(&resource.path))
        .collect::<HashSet<_>>();
    let protected_branches = durable
        .iter()
        .filter(|resource| resource.disposition == DurableWorktreeDisposition::Protect)
        .map(|resource| resource.branch.clone())
        .collect::<HashSet<_>>();
    let mut summary = WorktreeReconciliation::default();

    for registration in inventory.registrations.values() {
        let path_key = normalize_path(&registration.path);
        if !path_is_below(&path_key, &worktree_root) || protected_paths.contains(&path_key) {
            continue;
        }
        remove_registration(repository, &registration.path)?;
        summary.cleaned_registrations += 1;
        if registration.path.exists() {
            std::fs::remove_dir_all(&registration.path).with_context(|| {
                format!(
                    "failed to remove worktree leaf {}",
                    registration.path.display()
                )
            })?;
            summary.cleaned_paths += 1;
        }
    }

    for path in leaf_directories(repository)? {
        let path_key = normalize_path(&path);
        if protected_paths.contains(&path_key) || inventory.registrations.contains_key(&path_key) {
            continue;
        }
        std::fs::remove_dir_all(&path)
            .with_context(|| format!("failed to remove orphan worktree leaf {}", path.display()))?;
        summary.cleaned_paths += 1;
    }

    for branch in &inventory.branches {
        if protected_branches.contains(branch) {
            continue;
        }
        delete_branch(repository, branch)?;
        summary.cleaned_branches += 1;
    }
    Ok(summary)
}

fn validate_durable(
    repository: &Path,
    durable: &[DurableWorktreeResource],
    inventory: &Inventory,
    worktree_root: &str,
) -> Result<()> {
    let mut paths = HashSet::new();
    let mut branches = HashSet::new();
    for resource in durable {
        if resource.task_run_id.trim().is_empty() || !is_pure_branch(&resource.branch) {
            bail!("invalid durable worktree owner {}", resource.task_run_id);
        }
        let path_key = normalize_path(&resource.path);
        if !path_is_below(&path_key, worktree_root)
            || !paths.insert(path_key.clone())
            || !branches.insert(resource.branch.clone())
        {
            bail!(
                "duplicate or unsafe durable worktree for {}",
                resource.task_run_id
            );
        }
        if resource.disposition == DurableWorktreeDisposition::Cleanup {
            continue;
        }
        let registration = inventory.registrations.get(&path_key);
        let registration_matches = registration
            .and_then(|registration| registration.branch.as_deref())
            .is_some_and(|branch| branch == resource.branch);
        let path_exists = resource.path.is_dir();
        let branch_exists = inventory.branches.contains(&resource.branch);
        if !registration_matches || !path_exists || !branch_exists {
            bail!(
                "durable worktree is partially missing for task {}: registration={}, path={}, branch={}",
                resource.task_run_id,
                registration_matches,
                path_exists,
                branch_exists
            );
        }
        if let Some(expected_head) = resource.expected_head.as_deref() {
            let actual_head = git_output(repository, &["rev-parse", &resource.branch])?;
            if actual_head.trim() != expected_head {
                bail!(
                    "durable worktree branch tip drifted for task {}: expected {}, actual {}",
                    resource.task_run_id,
                    expected_head,
                    actual_head.trim()
                );
            }
        }
    }
    Ok(())
}

struct Inventory {
    registrations: HashMap<String, Registration>,
    branches: HashSet<String>,
}

struct Registration {
    path: PathBuf,
    branch: Option<String>,
}

fn inventory(repository: &Path) -> Result<Inventory> {
    let output = git_output(repository, &["worktree", "list", "--porcelain"])?;
    let mut registrations = HashMap::new();
    let mut path = None;
    let mut branch = None;
    for line in output.lines().chain(std::iter::once("")) {
        if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
            branch = Some(value.to_string());
        } else if line.is_empty()
            && let Some(path) = path.take()
        {
            registrations.insert(
                normalize_path(&path),
                Registration {
                    path,
                    branch: branch.take(),
                },
            );
        }
    }
    let branches = git_output(
        repository,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads/pure-task-*",
            "refs/heads/pure-agent-*",
        ],
    )?
    .lines()
    .filter(|branch| is_pure_branch(branch))
    .map(str::to_string)
    .collect();
    Ok(Inventory {
        registrations,
        branches,
    })
}

fn leaf_directories(repository: &Path) -> Result<Vec<PathBuf>> {
    let root = repository.join(WORKTREE_ROOT);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to inspect worktree root"),
    };
    let mut leaves = Vec::new();
    for parent in entries {
        let parent = parent?.path();
        if !parent.is_dir() {
            continue;
        }
        for leaf in std::fs::read_dir(&parent)? {
            let leaf = leaf?.path();
            if leaf.is_dir() {
                leaves.push(leaf);
            }
        }
    }
    Ok(leaves)
}

fn remove_registration(repository: &Path, path: &Path) -> Result<()> {
    let path = path.to_string_lossy().to_string();
    git_status(repository, &["worktree", "remove", "--force", &path])
}

fn delete_branch(repository: &Path, branch: &str) -> Result<()> {
    if !is_pure_branch(branch) {
        bail!("refusing to delete non-Pure worktree branch {branch}");
    }
    git_status(repository, &["branch", "-D", branch])
}

fn git_output(repository: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_status(repository: &Path, args: &[&str]) -> Result<()> {
    let _ = git_output(repository, args)?;
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn path_is_below(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_pure_branch(branch: &str) -> bool {
    branch.starts_with("pure-task-") || branch.starts_with("pure-agent-")
}
