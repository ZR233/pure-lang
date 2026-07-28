//! Studio task worktree 持久化资源的启动恢复与补偿。

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pl_core::path_safety::{remove_dir_all_no_follow, validate_existing_path};

use super::git_compatible_path;

mod git;
use git::{delete_branch, delete_task_branch_at_head, git_output, remove_registration};

const WORKTREE_ROOT: &str = ".pure/worktrees";
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableWorktreeDisposition {
    Protect,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableWorktreePresence {
    MustExist,
    MayBeUncreated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableWorktreeResource {
    pub task_run_id: String,
    pub path: PathBuf,
    pub branch: String,
    pub expected_head: Option<String>,
    pub presence: DurableWorktreePresence,
    pub disposition: DurableWorktreeDisposition,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorktreeReconciliation {
    pub cleaned_registrations: usize,
    pub cleaned_paths: usize,
    pub cleaned_branches: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableWorktreeResourcePresence {
    Absent,
    Complete,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableWorktreeInspection {
    pub task_run_id: String,
    pub path: PathBuf,
    pub branch: String,
    pub presence: DurableWorktreeResourcePresence,
    pub registration_exists: bool,
    pub path_exists: bool,
    pub branch_exists: bool,
    pub branch_head: Option<String>,
}

#[cfg(test)]
pub(crate) async fn reconcile_task_worktrees(
    repository: impl AsRef<Path>,
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    reconcile_task_worktree_group(&[repository.as_ref().to_path_buf()], durable).await
}

pub async fn reconcile_task_worktree_group(
    repositories: &[PathBuf],
    durable: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let repositories = repositories.to_vec();
    let durable = durable.to_vec();
    tokio::task::spawn_blocking(move || reconcile_blocking(&repositories, &durable))
        .await
        .context("worktree reconciliation task failed")?
}

pub async fn inspect_task_worktree_resources(
    repository: impl AsRef<Path>,
    resources: &[DurableWorktreeResource],
) -> Result<Vec<DurableWorktreeInspection>> {
    let repository = repository.as_ref().to_path_buf();
    let resources = resources.to_vec();
    tokio::task::spawn_blocking(move || inspect_resources_blocking(&repository, &resources))
        .await
        .context("worktree inspection task failed")?
}

pub async fn cleanup_task_worktree_resources(
    repository: impl AsRef<Path>,
    resources: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let repository = repository.as_ref().to_path_buf();
    let resources = resources.to_vec();
    tokio::task::spawn_blocking(move || cleanup_resources_blocking(&repository, &resources))
        .await
        .context("worktree cleanup task failed")?
}

pub fn validate_task_worktree_resource_identities(
    repository: impl AsRef<Path>,
    resources: &[DurableWorktreeResource],
) -> Result<()> {
    let repository = repository.as_ref();
    let root = WorktreeRoot {
        repository,
        normalized: normalize_path(&repository.join(WORKTREE_ROOT)),
    };
    for resource in resources {
        validate_resource_identity(resource, std::slice::from_ref(&root))?;
    }
    Ok(())
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
        if std::fs::symlink_metadata(&registration.path).is_ok() {
            ensure_safe_existing_path(root.repository, &registration.path)?;
            remove_dir_all_no_follow(&root.repository.join(WORKTREE_ROOT), &registration.path)
                .with_context(|| {
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
            remove_dir_all_no_follow(&root.repository.join(WORKTREE_ROOT), &path).with_context(
                || format!("failed to remove orphan worktree leaf {}", path.display()),
            )?;
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

fn inspect_resources_blocking(
    repository: &Path,
    resources: &[DurableWorktreeResource],
) -> Result<Vec<DurableWorktreeInspection>> {
    let inventory = inventory(repository)?;
    let root = WorktreeRoot {
        repository,
        normalized: normalize_path(&repository.join(WORKTREE_ROOT)),
    };
    resources
        .iter()
        .map(|resource| {
            validate_resource_identity(resource, std::slice::from_ref(&root))?;
            let path_key = normalize_path(&resource.path);
            let registration_exists = inventory
                .registrations
                .get(&path_key)
                .and_then(|registration| registration.branch.as_deref())
                .is_some_and(|branch| branch == resource.branch);
            let path_exists = resource.path.is_dir();
            let branch_exists = inventory.branches.contains(&resource.branch);
            if path_exists {
                ensure_safe_existing_path(repository, &resource.path)?;
            }
            let presence = if !registration_exists && !path_exists && !branch_exists {
                DurableWorktreeResourcePresence::Absent
            } else if registration_exists && path_exists && branch_exists {
                DurableWorktreeResourcePresence::Complete
            } else {
                DurableWorktreeResourcePresence::Partial
            };
            let branch_head = branch_exists
                .then(|| git_output(repository, &["rev-parse", &resource.branch]))
                .transpose()?
                .map(|head| head.trim().to_string());
            Ok(DurableWorktreeInspection {
                task_run_id: resource.task_run_id.clone(),
                path: resource.path.clone(),
                branch: resource.branch.clone(),
                presence,
                registration_exists,
                path_exists,
                branch_exists,
                branch_head,
            })
        })
        .collect()
}

fn cleanup_resources_blocking(
    repository: &Path,
    resources: &[DurableWorktreeResource],
) -> Result<WorktreeReconciliation> {
    let initial_inventory = inventory(repository)?;
    let root = WorktreeRoot {
        repository,
        normalized: normalize_path(&repository.join(WORKTREE_ROOT)),
    };
    let mut paths = HashSet::new();
    let mut branches = HashSet::new();
    for resource in resources {
        validate_resource_identity(resource, std::slice::from_ref(&root))?;
        let path_key = normalize_path(&resource.path);
        if !paths.insert(path_key.clone()) || !branches.insert(resource.branch.clone()) {
            bail!(
                "duplicate recovery cleanup resource for task {}",
                resource.task_run_id
            );
        }
        if let Some(registration) = initial_inventory.registrations.get(&path_key)
            && registration.branch.as_deref() != Some(resource.branch.as_str())
        {
            bail!(
                "recovery cleanup registration identity changed for task {}",
                resource.task_run_id
            );
        }
        if initial_inventory
            .registrations
            .iter()
            .any(|(registered_path, registration)| {
                registered_path != &path_key
                    && registration.branch.as_deref() == Some(resource.branch.as_str())
            })
        {
            bail!(
                "recovery cleanup branch belongs to another worktree for task {}",
                resource.task_run_id
            );
        }
        validate_cleanup_branch_head(repository, resource, &initial_inventory)?;
    }

    let mut summary = WorktreeReconciliation::default();
    for resource in resources {
        let path_key = normalize_path(&resource.path);
        if let Some(registration) = initial_inventory.registrations.get(&path_key) {
            ensure_safe_existing_path(repository, &registration.path)?;
            remove_registration(repository, &registration.path)?;
            pause_after_registration_remove(&registration.path);
            summary.cleaned_registrations += 1;
        }
        let current = inventory(repository)?;
        if current.registrations.contains_key(&path_key)
            || current
                .registrations
                .iter()
                .any(|(registered_path, registration)| {
                    registered_path != &path_key
                        && registration.branch.as_deref() == Some(resource.branch.as_str())
                })
        {
            bail!(
                "recovery cleanup registration identity changed for task {}",
                resource.task_run_id
            );
        }
        validate_cleanup_branch_head(repository, resource, &current)?;
        if std::fs::symlink_metadata(&resource.path).is_ok() {
            ensure_safe_existing_path(repository, &resource.path)?;
            remove_dir_all_no_follow(&repository.join(WORKTREE_ROOT), &resource.path)
                .with_context(|| {
                    format!(
                        "failed to remove recovery worktree leaf {}",
                        resource.path.display()
                    )
                })?;
            summary.cleaned_paths += 1;
        }
        if current.branches.contains(&resource.branch) {
            let expected_head = resource.expected_head.as_deref().context(
                "recovery cleanup branch appeared after preview; refresh before confirming",
            )?;
            delete_task_branch_at_head(repository, &resource.branch, expected_head)?;
            summary.cleaned_branches += 1;
        }
    }
    Ok(summary)
}

fn validate_cleanup_branch_head(
    repository: &Path,
    resource: &DurableWorktreeResource,
    inventory: &Inventory,
) -> Result<()> {
    if !inventory.branches.contains(&resource.branch) {
        return Ok(());
    }
    let expected_head = resource
        .expected_head
        .as_deref()
        .context("recovery cleanup branch appeared after preview; refresh before confirming")?;
    let actual_head = git_output(repository, &["rev-parse", &resource.branch])?;
    if actual_head.trim() != expected_head {
        bail!(
            "recovery cleanup branch tip changed for task {}: expected {}, actual {}",
            resource.task_run_id,
            expected_head,
            actual_head.trim()
        );
    }
    Ok(())
}

fn validate_resource_identity(
    resource: &DurableWorktreeResource,
    roots: &[WorktreeRoot<'_>],
) -> Result<()> {
    if !is_single_path_component(&resource.task_run_id) {
        bail!("invalid durable worktree owner {}", resource.task_run_id);
    }
    let branch_prefix = format!("pure-task-{}-", resource.task_run_id);
    let agent_id = resource
        .branch
        .strip_prefix(&branch_prefix)
        .filter(|agent_id| is_single_path_component(agent_id))
        .with_context(|| {
            format!(
                "invalid recovery cleanup branch {} for task {}",
                resource.branch, resource.task_run_id
            )
        })?;
    let path_key = normalize_path(&resource.path);
    let Some(root) = root_for_path(&path_key, roots) else {
        bail!(
            "unsafe durable worktree for {}: path={}",
            resource.task_run_id,
            path_key
        );
    };
    let expected_path = root
        .repository
        .join(WORKTREE_ROOT)
        .join(&resource.task_run_id)
        .join(agent_id);
    if path_key != normalize_path(&expected_path) {
        bail!(
            "recovery cleanup requires exact worktree leaf for task {}: path={}",
            resource.task_run_id,
            path_key
        );
    }
    Ok(())
}

fn is_single_path_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
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
    match std::fs::symlink_metadata(&root) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("failed to inspect worktree root"),
    }
    ensure_safe_worktree_root(repository, &root)?;
    let entries = std::fs::read_dir(&root).context("failed to inspect worktree root")?;
    let mut leaves = Vec::new();
    for parent in entries {
        let parent = parent?.path();
        ensure_safe_existing_path(repository, &parent)?;
        if !parent.is_dir() {
            continue;
        }
        for leaf in std::fs::read_dir(&parent)? {
            let leaf = leaf?.path();
            ensure_safe_existing_path(repository, &leaf)?;
            if leaf.is_dir() {
                leaves.push(leaf);
            }
        }
    }
    Ok(leaves)
}

fn normalize_path(path: &Path) -> String {
    let path = canonicalize_with_missing_suffix(path);
    let path = git_compatible_path(path);
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn canonicalize_with_missing_suffix(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut canonical) => {
                for component in missing.into_iter().rev() {
                    canonical.push(component);
                }
                return canonical;
            }
            Err(_) => {
                let Some(component) = existing.file_name().map(ToOwned::to_owned) else {
                    return path.to_path_buf();
                };
                missing.push(component);
                if !existing.pop() {
                    return path.to_path_buf();
                }
            }
        }
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

    validate_existing_path(&root, path)
        .with_context(|| format!("unsafe worktree path {}", path.display()))?;
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
    validate_existing_path(repository, root)
        .context("worktree root contains a link or reparse point")?;
    Ok(canonical_root)
}
