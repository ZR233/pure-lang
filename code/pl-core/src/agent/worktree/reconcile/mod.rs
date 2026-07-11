use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use super::git_compatible_path;

const WORKTREE_ROOT: &str = ".pure/worktrees";
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableWorktreeDisposition {
    Protect,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurableWorktreePresence {
    MustExist,
    MayBeUncreated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurableWorktreeResource {
    pub(crate) task_run_id: String,
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
    pub(crate) expected_head: Option<String>,
    pub(crate) presence: DurableWorktreePresence,
    pub(crate) disposition: DurableWorktreeDisposition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorktreeReconciliation {
    pub(crate) cleaned_registrations: usize,
    pub(crate) cleaned_paths: usize,
    pub(crate) cleaned_branches: usize,
}

#[cfg(test)]
pub(crate) async fn reconcile_task_worktrees(
    repository: impl AsRef<Path>,
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    reconcile_task_worktree_group(&[repository.as_ref().to_path_buf()], durable).await
}

pub(crate) async fn reconcile_task_worktree_group(
    repositories: &[PathBuf],
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let repositories = repositories.to_vec();
    let durable = durable.to_vec();
    tokio::task::spawn_blocking(move || reconcile_blocking(&repositories, &durable))
        .await
        .context("worktree reconciliation task failed")?
}

fn reconcile_blocking(
    repositories: &[PathBuf],
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let representative = repositories
        .first()
        .context("worktree reconciliation requires a repository")?;
    let roots = repositories
        .iter()
        .map(|repository| WorktreeRoot {
            repository,
            normalized: normalize_path(&repository.join(WORKTREE_ROOT)),
        })
        .collect::<Vec<_>>();
    let inventory = inventory(representative)?;
    validate_durable(representative, durable, &inventory, &roots)?;

    let known_repositories = repositories
        .iter()
        .map(|repository| normalize_path(repository))
        .collect::<HashSet<_>>();
    let mut protected_paths = durable
        .iter()
        .filter(|resource| resource.disposition == DurableWorktreeDisposition::Protect)
        .map(|resource| normalize_path(&resource.path))
        .collect::<HashSet<_>>();
    protected_paths.extend(known_repositories.iter().cloned());
    let mut protected_branches = durable
        .iter()
        .filter(|resource| resource.disposition == DurableWorktreeDisposition::Protect)
        .map(|resource| resource.branch.clone())
        .collect::<HashSet<_>>();
    protected_branches.extend(
        inventory
            .registrations
            .iter()
            .filter(|(path, _)| known_repositories.contains(*path))
            .filter_map(|(_, registration)| registration.branch.clone()),
    );
    let mut summary = WorktreeReconciliation::default();

    for registration in inventory.registrations.values() {
        let path_key = normalize_path(&registration.path);
        let Some(root) = root_for_path(&path_key, &roots) else {
            continue;
        };
        if protected_paths.contains(&path_key) {
            continue;
        }
        ensure_safe_existing_path(root.repository, &registration.path)?;
        remove_registration(representative, &registration.path)?;
        pause_after_registration_remove(&registration.path);
        summary.cleaned_registrations += 1;
        if registration.path.exists() {
            ensure_safe_existing_path(root.repository, &registration.path)?;
            std::fs::remove_dir_all(&registration.path).with_context(|| {
                format!(
                    "failed to remove worktree leaf {}",
                    registration.path.display()
                )
            })?;
            summary.cleaned_paths += 1;
        }
    }

    for root in &roots {
        for path in leaf_directories(root.repository)? {
            let path_key = normalize_path(&path);
            if protected_paths.contains(&path_key)
                || inventory.registrations.contains_key(&path_key)
            {
                continue;
            }
            ensure_safe_existing_path(root.repository, &path)?;
            std::fs::remove_dir_all(&path).with_context(|| {
                format!("failed to remove orphan worktree leaf {}", path.display())
            })?;
            summary.cleaned_paths += 1;
        }
    }

    for branch in &inventory.branches {
        if protected_branches.contains(branch) {
            continue;
        }
        delete_branch(representative, branch)?;
        summary.cleaned_branches += 1;
    }
    Ok(summary)
}

fn validate_durable(
    representative: &Path,
    durable: &[DurableWorktreeResource],
    inventory: &Inventory,
    roots: &[WorktreeRoot<'_>],
) -> Result<()> {
    let mut paths = HashSet::new();
    let mut branches = HashSet::new();
    for resource in durable {
        if resource.task_run_id.trim().is_empty() || !is_pure_branch(&resource.branch) {
            bail!("invalid durable worktree owner {}", resource.task_run_id);
        }
        let path_key = normalize_path(&resource.path);
        let root = root_for_path(&path_key, roots);
        if root.is_none()
            || !paths.insert(path_key.clone())
            || !branches.insert(resource.branch.clone())
        {
            let allowed_roots = roots
                .iter()
                .map(|root| root.normalized.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!(
                "duplicate or unsafe durable worktree for {}: path={}, roots={}",
                resource.task_run_id,
                path_key,
                allowed_roots
            );
        }
        let root = root.context("durable worktree root disappeared during validation")?;
        if resource.disposition == DurableWorktreeDisposition::Cleanup {
            continue;
        }
        let registration = inventory.registrations.get(&path_key);
        let registration_matches = registration
            .and_then(|registration| registration.branch.as_deref())
            .is_some_and(|branch| branch == resource.branch);
        let path_exists = resource.path.is_dir();
        let branch_exists = inventory.branches.contains(&resource.branch);
        let all_absent = !registration_matches && !path_exists && !branch_exists;
        if all_absent && resource.presence == DurableWorktreePresence::MayBeUncreated {
            continue;
        }
        if !registration_matches || !path_exists || !branch_exists {
            bail!(
                "durable worktree is partially missing for task {}: registration={}, path={}, branch={}",
                resource.task_run_id,
                registration_matches,
                path_exists,
                branch_exists
            );
        }
        ensure_safe_existing_path(root.repository, &resource.path)?;
        if let Some(expected_head) = resource.expected_head.as_deref() {
            let actual_head = git_output(representative, &["rev-parse", &resource.branch])?;
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

struct WorktreeRoot<'a> {
    repository: &'a Path,
    normalized: String,
}

fn root_for_path<'a, 'b>(
    path: &str,
    roots: &'a [WorktreeRoot<'b>],
) -> Option<&'a WorktreeRoot<'b>> {
    roots
        .iter()
        .filter(|root| path_is_below(path, &root.normalized))
        .max_by_key(|root| root.normalized.len())
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
    if !root.exists() {
        return Ok(Vec::new());
    }
    ensure_safe_worktree_root(repository, &root)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to inspect worktree root"),
    };
    let mut leaves = Vec::new();
    for parent in entries {
        let parent = parent?.path();
        ensure_safe_existing_path(repository, &parent)?;
        if !parent.is_dir() {
            continue;
        }
        for leaf in std::fs::read_dir(&parent)? {
            let leaf = leaf?.path();
            if leaf.is_dir() {
                ensure_safe_existing_path(repository, &leaf)?;
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
    let child = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    let mut child = KillOnDropChild::new(child);
    let stdout = child
        .child
        .stdout
        .take()
        .context("git stdout pipe is missing")?;
    let stderr = child
        .child
        .stderr
        .take()
        .context("git stderr pipe is missing")?;
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        if let Some(status) = child
            .child
            .try_wait()
            .context("failed to poll git process")?
        {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill_and_wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            bail!(
                "git {} timed out after {}s",
                args.join(" "),
                GIT_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.completed = true;
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("git stdout reader panicked"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("git stderr reader panicked"))??;
    if !status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&stdout).to_string())
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

struct KillOnDropChild {
    child: Child,
    completed: bool,
}

impl KillOnDropChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            completed: false,
        }
    }

    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.completed = true;
    }
}

impl Drop for KillOnDropChild {
    fn drop(&mut self) {
        if !self.completed {
            self.kill_and_wait();
        }
    }
}

fn git_status(repository: &Path, args: &[&str]) -> Result<()> {
    let _ = git_output(repository, args)?;
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = git_compatible_path(path);
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

#[cfg(test)]
pub(crate) fn set_after_registration_remove_barrier(
    path: PathBuf,
    barrier: std::sync::Arc<std::sync::Barrier>,
) {
    *after_registration_remove_barrier()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some((normalize_path(&path), barrier));
}

#[cfg(test)]
fn pause_after_registration_remove(path: &Path) {
    let barrier = {
        let mut slot = after_registration_remove_barrier()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|(target, _)| target == &normalize_path(path))
        {
            slot.take().map(|(_, barrier)| barrier)
        } else {
            None
        }
    };
    if let Some(barrier) = barrier {
        barrier.wait();
        barrier.wait();
    }
}

#[cfg(not(test))]
fn pause_after_registration_remove(_path: &Path) {}

#[cfg(test)]
type RegistrationRemoveBarrier = (String, std::sync::Arc<std::sync::Barrier>);

#[cfg(test)]
fn after_registration_remove_barrier()
-> &'static std::sync::Mutex<Option<RegistrationRemoveBarrier>> {
    static BARRIER: std::sync::OnceLock<std::sync::Mutex<Option<RegistrationRemoveBarrier>>> =
        std::sync::OnceLock::new();
    BARRIER.get_or_init(|| std::sync::Mutex::new(None))
}

fn ensure_safe_existing_path(repository: &Path, path: &Path) -> Result<()> {
    let root = repository.join(WORKTREE_ROOT);
    let canonical_root = ensure_safe_worktree_root(repository, &root)?;
    let canonical_path = std::fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize worktree path {}", path.display()))?;
    if canonical_path == canonical_root || canonical_path.strip_prefix(&canonical_root).is_err() {
        bail!(
            "worktree path escapes root through a link or reparse point: {}",
            path.display()
        );
    }

    let mut current = path.to_path_buf();
    loop {
        reject_link_or_reparse(&current)?;
        if normalize_path(&current) == normalize_path(&root) {
            break;
        }
        let Some(parent) = current.parent() else {
            bail!("worktree path is not below its root: {}", path.display());
        };
        current = parent.to_path_buf();
    }
    Ok(())
}

fn ensure_safe_worktree_root(repository: &Path, root: &Path) -> Result<PathBuf> {
    let canonical_repository =
        std::fs::canonicalize(repository).context("failed to canonicalize worktree repository")?;
    let canonical_root =
        std::fs::canonicalize(root).context("failed to canonicalize worktree root")?;
    if canonical_root.strip_prefix(&canonical_repository).is_err() {
        bail!("worktree root escapes repository through a link or reparse point");
    }
    if let Some(pure_dir) = root.parent() {
        reject_link_or_reparse(pure_dir)?;
    }
    reject_link_or_reparse(root)?;
    Ok(canonical_root)
}

fn reject_link_or_reparse(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect worktree path {}", path.display()))?;
    if is_link_or_reparse(&metadata) {
        bail!(
            "worktree path contains a link or reparse point: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
