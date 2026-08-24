use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use pl_core::{AgentIdentity, AgentWorkspace, WorkspaceMutability, resolve_workspace_root};

use crate::StudioMode;
use crate::config::StudioRole;
use crate::studio::records::{ProjectRecord, ThreadRecord};
use crate::studio::task_coordinator::{ReviewScope, TaskRun, WorkCompletionRecord};
use crate::studio::task_projection::LoadedTaskAggregate;

/// 从 Studio durable owner 解析单个 Agent 的 canonical workspace。
#[derive(Clone)]
pub(super) struct AgentWorkspaceResolver {}

impl AgentWorkspaceResolver {
    pub(super) fn new() -> Self {
        Self {}
    }

    pub(super) async fn resolve(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
        project: &ProjectRecord,
        active_task: Option<&LoadedTaskAggregate>,
    ) -> Result<AgentWorkspace> {
        let mode = thread.mode;
        if identity.parent_id.is_none() {
            return self
                .resolve_root(mode, project, active_task.map(|task| &task.run))
                .await;
        }
        match identity.role.as_str() {
            role if role == StudioRole::Executor.key() && mode == StudioMode::Task => {
                self.resolve_executor(identity, thread, active_task).await
            }
            role if role == StudioRole::Reviewer.key() && mode == StudioMode::Task => {
                self.resolve_reviewer(identity, thread, active_task).await
            }
            role if role == StudioRole::Explorer.key() => {
                let root = project_workspace(project)?;
                Ok(AgentWorkspace::confined(
                    root,
                    WorkspaceMutability::ReadWrite,
                ))
            }
            role => bail!("unsupported Studio child role for workspace resolution: {role}"),
        }
    }

    async fn resolve_root(
        &self,
        mode: StudioMode,
        project: &ProjectRecord,
        active_task_run: Option<&TaskRun>,
    ) -> Result<AgentWorkspace> {
        let root = project_workspace(project)?;
        match mode {
            StudioMode::Simple => Ok(AgentWorkspace::local(root)),
            StudioMode::Task => {
                if let Some(run) = active_task_run {
                    validate_main_workspace(&root, run).await?;
                }
                Ok(AgentWorkspace::confined(
                    root,
                    WorkspaceMutability::ReadWrite,
                ))
            }
        }
    }

    async fn resolve_executor(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
        active_task: Option<&LoadedTaskAggregate>,
    ) -> Result<AgentWorkspace> {
        let active_task = active_task.context("executor has no resident Task aggregate")?;
        let work_unit = active_task
            .work_units
            .iter()
            .find(|unit| unit.executor_thread_id.as_deref() == Some(identity.id.as_str()))
            .with_context(|| format!("executor {} has no hot WorkUnit owner", identity.id))?;
        let run = &active_task.run;
        ensure!(
            run.root_thread_id == thread.root_thread_id,
            "executor WorkUnit belongs to another TaskRun root"
        );
        let root = validate_child_workspace(&work_unit.worktree_path, &work_unit.branch, run)?;
        Ok(AgentWorkspace::confined(
            root,
            WorkspaceMutability::ReadWrite,
        ))
    }

    async fn resolve_reviewer(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
        active_task: Option<&LoadedTaskAggregate>,
    ) -> Result<AgentWorkspace> {
        let active_task = active_task.context("reviewer has no resident Task aggregate")?;
        let round = active_task
            .reviews
            .iter()
            .find(|round| round.reviewer_thread_id() == Some(identity.id.as_str()))
            .with_context(|| format!("reviewer {} has no hot ReviewRound owner", identity.id))?;
        let run = &active_task.run;
        ensure!(
            run.root_thread_id == thread.root_thread_id,
            "review round belongs to another TaskRun root"
        );
        let root = match round.scope {
            ReviewScope::Delivery => {
                let completion_id = round
                    .completion_id
                    .as_deref()
                    .context("delivery review has no completion id")?;
                let completion = active_task
                    .completions
                    .iter()
                    .find(|completion| completion.id == completion_id)
                    .context("delivery review completion not found")?;
                validate_delivery_target(round, completion)?;
                let root =
                    validate_child_workspace(&completion.worktree_path, &completion.branch, run)?;
                let expected_head = completion
                    .head_commit()
                    .unwrap_or(completion.base_commit.as_str());
                ensure!(
                    round.reviewed_head == expected_head,
                    "delivery review no longer matches its Completion revision"
                );
                root
            }
            ReviewScope::Integrated => {
                validate_main_workspace(Path::new(&run.workspace_root), run).await?
            }
        };
        Ok(AgentWorkspace::confined(
            root,
            WorkspaceMutability::ReadWrite,
        ))
    }
}

fn project_workspace(project: &ProjectRecord) -> Result<PathBuf> {
    resolve_workspace_root(Path::new(&project.path)).map_err(anyhow::Error::from)
}

async fn validate_main_workspace(root: &Path, run: &TaskRun) -> Result<PathBuf> {
    let root = resolve_workspace_root(root).map_err(anyhow::Error::from)?;
    ensure!(
        same_path(&root, Path::new(&run.workspace_root)),
        "TaskRun main workspace does not match the project workspace"
    );
    Ok(root)
}

fn validate_child_workspace(
    stored_path: &str,
    expected_branch: &str,
    run: &TaskRun,
) -> Result<PathBuf> {
    ensure!(
        expected_branch.starts_with("pure-task-"),
        "Task child branch is not Pure-owned"
    );
    let root = resolve_workspace_root(Path::new(stored_path)).map_err(anyhow::Error::from)?;
    let task_workspace =
        resolve_workspace_root(Path::new(&run.workspace_root)).map_err(anyhow::Error::from)?;
    let worktree_root = task_workspace.join(".pure").join("worktrees");
    ensure!(
        path_is_descendant(&root, &worktree_root),
        "Task child workspace is outside .pure/worktrees"
    );
    Ok(root)
}

fn validate_delivery_target(
    round: &crate::studio::task_coordinator::ReviewRoundRecord,
    completion: &WorkCompletionRecord,
) -> Result<()> {
    ensure!(
        round.completion_id.as_deref() == Some(completion.id.as_str())
            && round.completion_revision == Some(completion.revision)
            && round.work_unit_id.as_deref() == Some(completion.work_unit_id.as_str())
            && round.task_run_id == completion.task_run_id,
        "delivery ReviewRound no longer matches its locked Completion"
    );
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = canonical_comparable(left);
    let right = canonical_comparable(right);
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn path_is_descendant(path: &Path, parent: &Path) -> bool {
    let path = canonical_comparable(path);
    let mut parent = canonical_comparable(parent);
    if cfg!(windows) {
        parent = parent.to_ascii_lowercase();
        let path = path.to_ascii_lowercase();
        return path.starts_with(&format!("{parent}/"));
    }
    path.starts_with(&format!("{parent}/"))
}

fn canonical_comparable(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string()
}
