use std::path::{Path, PathBuf};

use anyhow::Result;

use pl_core::tool::{ExecutionBackend, ExecutionRequest};

use crate::{
    AgentRuntimeHandle, AgentState, RemoteWorktreeBackend, ThreadId, WorktreeHandle,
    WorktreeManager,
};

use super::git::{checked_git, run_git};
use crate::studio::task_coordinator::{MergeCleanupResult, TaskCoordinator, TaskMergeScope};

impl TaskCoordinator {
    pub(in crate::studio::task_coordinator) async fn validate_accepted_cleanup_replay(
        &self,
        _scope: &TaskMergeScope,
    ) -> Result<()> {
        Ok(())
    }
}

pub(crate) async fn cleanup_accepted_delivery(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
    runtime: Option<&AgentRuntimeHandle>,
) -> MergeCleanupResult {
    match validate_cleanup_identity(coordinator, scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => return MergeCleanupResult::AlreadyAbsent,
        Ok(CleanupPresence::Present) => {}
        Err(error) => return cleanup_failure(error.to_string()),
    }
    if let Some(runtime) = runtime {
        let agent_id = match ThreadId::new(scope.completion.executor_agent_id.clone()) {
            Ok(agent_id) => agent_id,
            Err(error) => return cleanup_failure(error.to_string()),
        };
        let listed = match runtime.list().await {
            Ok(listed) => listed,
            Err(error) => return cleanup_failure(error.to_string()),
        };
        if listed.iter().any(|snapshot| {
            snapshot.identity.id == agent_id
                && !matches!(
                    snapshot.state,
                    AgentState::Closing(_) | AgentState::Closed(_)
                )
        }) {
            let cleanup = match runtime.close(agent_id).await {
                Ok(_) => MergeCleanupResult::Discarded,
                Err(error) => cleanup_failure(error.to_string()),
            };
            return verify_cleanup_result(coordinator, scope, cleanup).await;
        }
    }

    let manager = match worktree_manager(coordinator, scope).await {
        Ok(manager) => manager,
        Err(error) => return cleanup_failure(error.to_string()),
    };
    let handle = WorktreeHandle {
        path: PathBuf::from(&scope.work_unit.worktree_path),
        branch: scope.work_unit.branch.clone(),
    };
    let cleanup = match manager.discard(&handle).await {
        Ok(()) => MergeCleanupResult::Discarded,
        Err(error) => cleanup_failure(error.to_string()),
    };
    verify_cleanup_result(coordinator, scope, cleanup).await
}

enum CleanupPresence {
    Present,
    AlreadyAbsent,
}

async fn validate_cleanup_identity(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
) -> anyhow::Result<CleanupPresence> {
    let project = coordinator
        .store
        .read_project(&scope.run.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("cleanup project not found"))?;
    let Some(server_id) = project.ssh_server_id else {
        return validate_local_cleanup_identity(scope).await;
    };
    let host = coordinator
        .ssh_manager
        .open_workspace_host(&server_id, scope.run.workspace_root.clone())
        .await?;
    validate_remote_cleanup_identity(scope, &host).await
}

async fn validate_local_cleanup_identity(
    scope: &TaskMergeScope,
) -> anyhow::Result<CleanupPresence> {
    let repository = Path::new(&scope.run.workspace_root);
    let worktree = Path::new(&scope.work_unit.worktree_path);
    let reference = format!("refs/heads/{}", scope.work_unit.branch);
    let branch = run_git(
        repository,
        vec!["rev-parse".into(), "--verify".into(), reference],
    )
    .await?;
    if !worktree.exists() && !branch.success {
        return Ok(CleanupPresence::AlreadyAbsent);
    }
    if !worktree.exists() || !branch.success {
        anyhow::bail!("executor cleanup resources are partially missing");
    }
    let branch_tip = branch.stdout_text()?.trim().to_string();
    let worktree_head = checked_git(worktree, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let delivery_head = resolve_delivery_head(worktree, &scope.delivery.head_commit).await?;
    let worktree_branch = checked_git(
        worktree,
        vec![
            "symbolic-ref".into(),
            "--quiet".into(),
            "--short".into(),
            "HEAD".into(),
        ],
    )
    .await?;
    if branch_tip != delivery_head
        || worktree_head != delivery_head
        || worktree_branch != scope.work_unit.branch
    {
        anyhow::bail!("executor branch or worktree tip drifted before cleanup");
    }
    Ok(CleanupPresence::Present)
}

async fn validate_remote_cleanup_identity(
    scope: &TaskMergeScope,
    host: &pl_core::remote::RemoteWorkspaceHost,
) -> anyhow::Result<CleanupPresence> {
    let repository = Path::new(&scope.run.workspace_root);
    let worktree = Path::new(&scope.work_unit.worktree_path);
    let relative_worktree = worktree
        .strip_prefix(repository)?
        .to_string_lossy()
        .replace('\\', "/");
    let reference = format!("refs/heads/{}", scope.work_unit.branch);
    let branch = remote_git(
        &host.git,
        repository,
        vec!["rev-parse".into(), "--verify".into(), reference],
    )
    .await?;
    let worktree_exists = host
        .files
        .stat_optional(relative_worktree, None)
        .await?
        .is_some();
    if !worktree_exists && branch.status != 0 {
        return Ok(CleanupPresence::AlreadyAbsent);
    }
    if !worktree_exists || branch.status != 0 {
        anyhow::bail!("executor cleanup resources are partially missing");
    }
    let branch_tip = branch.stdout.trim().to_string();
    let worktree_head =
        checked_remote_git(&host.git, worktree, vec!["rev-parse".into(), "HEAD".into()]).await?;
    let delivery_head = checked_remote_git(
        &host.git,
        worktree,
        vec![
            "rev-parse".into(),
            "--verify".into(),
            format!("{}^{{commit}}", scope.delivery.head_commit),
        ],
    )
    .await?;
    let worktree_branch = checked_remote_git(
        &host.git,
        worktree,
        vec![
            "symbolic-ref".into(),
            "--quiet".into(),
            "--short".into(),
            "HEAD".into(),
        ],
    )
    .await?;
    if branch_tip != delivery_head
        || worktree_head != delivery_head
        || worktree_branch != scope.work_unit.branch
    {
        anyhow::bail!("executor branch or worktree tip drifted before cleanup");
    }
    Ok(CleanupPresence::Present)
}

async fn remote_git(
    backend: &pl_core::remote::RemoteExecutionBackend,
    repository: &Path,
    arguments: Vec<String>,
) -> anyhow::Result<pl_core::ExecutionOutput> {
    backend
        .run(ExecutionRequest {
            program: PathBuf::from("git"),
            args: arguments,
            cwd: repository.to_path_buf(),
            env: Default::default(),
            timeout: Some(std::time::Duration::from_secs(120)),
        })
        .await
        .map_err(anyhow::Error::msg)
}

async fn checked_remote_git(
    backend: &pl_core::remote::RemoteExecutionBackend,
    repository: &Path,
    arguments: Vec<String>,
) -> anyhow::Result<String> {
    let command = arguments.join(" ");
    let output = remote_git(backend, repository, arguments).await?;
    if output.status != 0 {
        anyhow::bail!("git {command} failed: {}", output.stderr.trim());
    }
    Ok(output.stdout.trim().to_string())
}

async fn worktree_manager(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
) -> anyhow::Result<WorktreeManager> {
    let repo_root = PathBuf::from(&scope.run.workspace_root);
    let project = coordinator
        .store
        .read_project(&scope.run.project_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("cleanup project not found"))?;
    Ok(match project.ssh_server_id {
        Some(server_id) => WorktreeManager::with_backend(
            repo_root.clone(),
            std::sync::Arc::new(RemoteWorktreeBackend::new(
                coordinator.ssh_manager.clone(),
                server_id,
                repo_root,
            )),
        ),
        None => WorktreeManager::local(repo_root),
    })
}

async fn resolve_delivery_head(worktree: &Path, delivery_head: &str) -> Result<String> {
    checked_git(
        worktree,
        vec![
            "rev-parse".into(),
            "--verify".into(),
            format!("{delivery_head}^{{commit}}"),
        ],
    )
    .await
}

async fn verify_cleanup_result(
    coordinator: &TaskCoordinator,
    scope: &TaskMergeScope,
    cleanup: MergeCleanupResult,
) -> MergeCleanupResult {
    if matches!(cleanup, MergeCleanupResult::Failed { .. }) {
        return cleanup;
    }
    match validate_cleanup_identity(coordinator, scope).await {
        Ok(CleanupPresence::AlreadyAbsent) => cleanup,
        Ok(CleanupPresence::Present) => {
            cleanup_failure("executor cleanup reported success but resources remain".to_string())
        }
        Err(error) => cleanup_failure(error.to_string()),
    }
}

fn cleanup_failure(detail: String) -> MergeCleanupResult {
    MergeCleanupResult::Failed { detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_delivery_head_resolves_to_the_canonical_commit() {
        let repository = tempfile::tempdir().expect("temporary repository");
        checked_git(
            repository.path(),
            vec!["init".into(), "-b".into(), "main".into()],
        )
        .await
        .expect("initialize repository");
        std::fs::write(repository.path().join("README.md"), "fixture\n").expect("write fixture");
        checked_git(repository.path(), vec!["add".into(), "README.md".into()])
            .await
            .expect("stage fixture");
        checked_git(
            repository.path(),
            vec!["commit".into(), "-m".into(), "initialize fixture".into()],
        )
        .await
        .expect("commit fixture");
        let full_head = checked_git(repository.path(), vec!["rev-parse".into(), "HEAD".into()])
            .await
            .expect("resolve full head");
        let short_head = &full_head[..7];

        let resolved = resolve_delivery_head(repository.path(), short_head)
            .await
            .expect("resolve short head");

        assert_eq!(resolved, full_head);
    }
}
