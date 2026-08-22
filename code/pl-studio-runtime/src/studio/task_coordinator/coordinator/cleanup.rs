//! 线程/项目级恢复清理的 scope、preview、授权与执行。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::super::git::inspect_worktree_changes;
use super::super::{TaskRun, TaskWorktreeOwnerSnapshot, WorkUnit};
use super::TaskCoordinator;
use crate::agent::worktree::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    DurableWorktreeResourcePresence, cleanup_task_worktree_resources,
    inspect_task_worktree_resources, validate_task_worktree_resource_identities,
};
use crate::studio::runtime_state::{
    StudioRecoveryCleanupPreview, StudioRecoveryCleanupResource, StudioRecoveryIssue,
    StudioRecoveryIssueAction, StudioRecoveryIssueCategory, StudioRecoveryIssueScope,
    StudioRecoveryResourcePresence,
};

pub(super) struct WorktreeRecoveryGroup {
    pub(super) repositories: Vec<PathBuf>,
    pub(super) owners: Vec<TaskWorktreeOwnerSnapshot>,
}

struct RecoveryCleanupRun {
    run: TaskRun,
    work_units: Vec<WorkUnit>,
}

struct RecoveryCleanupScope {
    project_updated_at: Option<i64>,
    thread_updated_at: Option<i64>,
    runs: Vec<RecoveryCleanupRun>,
}

pub(crate) struct RecoveryCleanupAuthorization {
    preview: StudioRecoveryCleanupPreview,
    scope: RecoveryCleanupScope,
}

impl TaskCoordinator {
    pub(crate) async fn preview_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<StudioRecoveryCleanupPreview> {
        if !matches!(
            issue.action,
            StudioRecoveryIssueAction::CleanupThread | StudioRecoveryIssueAction::RemoveProject
        ) {
            bail!("recovery issue does not authorize destructive cleanup");
        }
        let scope = self.recovery_cleanup_scope(issue).await?;
        self.recovery_cleanup_preview_for_scope(issue, &scope).await
    }

    pub(crate) async fn project_cleanup_issue(
        &self,
        project_id: &str,
    ) -> Result<StudioRecoveryIssue> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("project cleanup target not found")?;
        Ok(StudioRecoveryIssue {
            id: format!("project-cleanup-{}", project.id),
            scope: StudioRecoveryIssueScope::Project,
            category: StudioRecoveryIssueCategory::Worktree,
            action: StudioRecoveryIssueAction::RemoveProject,
            project_id: Some(project.id),
            thread_id: None,
            task_run_id: None,
            message: format!(
                "Remove {} from Studio and discard its Pure-owned task worktrees.",
                project.name
            ),
        })
    }

    pub(crate) async fn preview_project_cleanup(
        &self,
        project_id: &str,
    ) -> Result<StudioRecoveryCleanupPreview> {
        let issue = self.project_cleanup_issue(project_id).await?;
        self.preview_recovery_cleanup(&issue).await
    }

    pub(crate) async fn validate_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
        expected_revision: &str,
    ) -> Result<RecoveryCleanupAuthorization> {
        let scope = self.recovery_cleanup_scope(issue).await?;
        let preview = self
            .recovery_cleanup_preview_for_scope(issue, &scope)
            .await?;
        if preview.expected_revision != expected_revision {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }
        Ok(RecoveryCleanupAuthorization { preview, scope })
    }

    pub(crate) async fn execute_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
        authorization: &RecoveryCleanupAuthorization,
    ) -> Result<()> {
        let scope = self.recovery_cleanup_scope(issue).await?;
        if !recovery_cleanup_scope_matches(&authorization.scope, &scope) {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }
        let current_resources = self
            .collect_recovery_cleanup_resources(issue, &scope)
            .await?;
        if current_resources != authorization.preview.resources {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }

        let preview_by_work_unit = authorization
            .preview
            .resources
            .iter()
            .map(|resource| (resource.work_unit_id.as_str(), resource))
            .collect::<HashMap<_, _>>();
        let mut durable_by_workspace = BTreeMap::<String, Vec<DurableWorktreeResource>>::new();
        for cleanup in &scope.runs {
            let durable = durable_by_workspace
                .entry(cleanup.run.workspace_root.clone())
                .or_default();
            durable.extend(cleanup.work_units.iter().map(|unit| {
                DurableWorktreeResource {
                    task_run_id: cleanup.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: preview_by_work_unit
                        .get(unit.id.as_str())
                        .and_then(|resource| resource.branch_head.clone()),
                    presence: DurableWorktreePresence::MayBeUncreated,
                    disposition: DurableWorktreeDisposition::Cleanup,
                }
            }));
        }
        for (workspace_root, durable) in &durable_by_workspace {
            validate_task_worktree_resource_identities(workspace_root, durable)?;
        }
        for cleanup in &scope.runs {
            self.store
                .authorize_recovery_cleanup(&cleanup.run.id)
                .await?;
            self.release_owned_process_lease(&cleanup.run.id);
        }
        for (workspace_root, durable) in durable_by_workspace {
            cleanup_task_worktree_resources(workspace_root, &durable).await?;
        }
        Ok(())
    }

    async fn recovery_cleanup_scope(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<RecoveryCleanupScope> {
        let (project_updated_at, thread_updated_at, runs) = match issue.action {
            StudioRecoveryIssueAction::CleanupThread => {
                let thread_id = issue
                    .thread_id
                    .as_deref()
                    .context("recovery issue has no Thread")?;
                let thread = self
                    .store
                    .read_thread(thread_id)
                    .await?
                    .context("recovery cleanup Thread not found")?;
                let runs = if let Some(task_run_id) = issue.task_run_id.as_deref() {
                    vec![
                        self.store
                            .read_task_run(task_run_id)
                            .await?
                            .context("recovery cleanup task run not found")?,
                    ]
                } else {
                    Vec::new()
                };
                (None, Some(thread.updated_at), runs)
            }
            StudioRecoveryIssueAction::RemoveProject => {
                let project_id = issue
                    .project_id
                    .as_deref()
                    .context("project recovery cleanup has no project")?;
                let project = self
                    .store
                    .read_project(project_id)
                    .await?
                    .context("project recovery cleanup target not found")?;
                (
                    Some(project.updated_at),
                    None,
                    self.store.list_task_runs_for_project(project_id).await?,
                )
            }
            StudioRecoveryIssueAction::Retry => {
                bail!("recovery issue does not authorize destructive cleanup")
            }
        };
        let mut cleanup_runs = Vec::with_capacity(runs.len());
        for run in runs {
            let work_units = self.store.list_work_units(&run.id).await?;
            cleanup_runs.push(RecoveryCleanupRun { run, work_units });
        }
        Ok(RecoveryCleanupScope {
            project_updated_at,
            thread_updated_at,
            runs: cleanup_runs,
        })
    }

    async fn recovery_cleanup_preview_for_scope(
        &self,
        issue: &StudioRecoveryIssue,
        scope: &RecoveryCleanupScope,
    ) -> Result<StudioRecoveryCleanupPreview> {
        let resources = self
            .collect_recovery_cleanup_resources(issue, scope)
            .await?;
        let expected_revision = recovery_cleanup_revision(issue, scope, &resources);
        Ok(StudioRecoveryCleanupPreview {
            issue_id: issue.id.clone(),
            expected_revision,
            scope: issue.scope,
            project_id: issue.project_id.clone(),
            thread_id: issue.thread_id.clone(),
            message: issue.message.clone(),
            resources,
        })
    }

    async fn collect_recovery_cleanup_resources(
        &self,
        issue: &StudioRecoveryIssue,
        scope: &RecoveryCleanupScope,
    ) -> Result<Vec<StudioRecoveryCleanupResource>> {
        let resource_capacity = scope
            .runs
            .iter()
            .map(|cleanup| cleanup.work_units.len())
            .sum();
        let mut resources = Vec::with_capacity(resource_capacity);
        for cleanup in &scope.runs {
            let durable = cleanup
                .work_units
                .iter()
                .map(|unit| DurableWorktreeResource {
                    task_run_id: cleanup.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: None,
                    presence: DurableWorktreePresence::MayBeUncreated,
                    disposition: DurableWorktreeDisposition::Cleanup,
                })
                .collect::<Vec<_>>();
            validate_task_worktree_resource_identities(&cleanup.run.workspace_root, &durable)?;
            let inspections = match inspect_task_worktree_resources(
                &cleanup.run.workspace_root,
                &durable,
            )
            .await
            {
                Ok(inspections) => Some(inspections),
                Err(_) if issue.scope == StudioRecoveryIssueScope::Project => None,
                Err(error) => return Err(error),
            };
            for (index, unit) in cleanup.work_units.iter().enumerate() {
                let inspection = inspections
                    .as_ref()
                    .and_then(|inspections| inspections.get(index));
                let changes = if inspection.is_some_and(|inspection| inspection.path_exists) {
                    inspect_worktree_changes(&unit.worktree_path, &unit.base_commit)
                        .await
                        .ok()
                } else {
                    None
                };
                resources.push(StudioRecoveryCleanupResource {
                    work_unit_id: unit.id.clone(),
                    path: unit.worktree_path.clone(),
                    branch: unit.branch.clone(),
                    presence: match inspection.map(|inspection| inspection.presence) {
                        Some(DurableWorktreeResourcePresence::Absent) => {
                            StudioRecoveryResourcePresence::Absent
                        }
                        Some(DurableWorktreeResourcePresence::Complete) => {
                            StudioRecoveryResourcePresence::Complete
                        }
                        Some(DurableWorktreeResourcePresence::Partial) | None => {
                            StudioRecoveryResourcePresence::Partial
                        }
                    },
                    registration_exists: inspection
                        .is_some_and(|inspection| inspection.registration_exists),
                    path_exists: inspection.is_some_and(|inspection| inspection.path_exists),
                    branch_exists: inspection.is_some_and(|inspection| inspection.branch_exists),
                    branch_head: inspection.and_then(|inspection| inspection.branch_head.clone()),
                    dirty: changes.as_ref().is_some_and(|changes| changes.dirty),
                    ahead_by: changes.as_ref().map_or(0, |changes| changes.ahead_by),
                    changed_file_count: changes
                        .as_ref()
                        .map_or(0, |changes| changes.changed_file_count),
                });
            }
        }
        Ok(resources)
    }
}

fn recovery_cleanup_scope_matches(
    authorized: &RecoveryCleanupScope,
    current: &RecoveryCleanupScope,
) -> bool {
    if authorized.project_updated_at != current.project_updated_at
        || authorized.thread_updated_at != current.thread_updated_at
    {
        return false;
    }
    let authorized_runs = authorized
        .runs
        .iter()
        .map(|cleanup| {
            (
                cleanup.run.id.as_str(),
                cleanup.run.root_thread_id.as_str(),
                cleanup.run.workspace_root.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let current_runs = current
        .runs
        .iter()
        .map(|cleanup| {
            (
                cleanup.run.id.as_str(),
                cleanup.run.root_thread_id.as_str(),
                cleanup.run.workspace_root.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    if authorized_runs != current_runs {
        return false;
    }
    let authorized_work_units = authorized
        .runs
        .iter()
        .flat_map(|cleanup| cleanup.work_units.iter())
        .map(|unit| {
            (
                unit.id.as_str(),
                unit.task_run_id.as_str(),
                unit.base_commit.as_str(),
                unit.worktree_path.as_str(),
                unit.branch.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let current_work_units = current
        .runs
        .iter()
        .flat_map(|cleanup| cleanup.work_units.iter())
        .map(|unit| {
            (
                unit.id.as_str(),
                unit.task_run_id.as_str(),
                unit.base_commit.as_str(),
                unit.worktree_path.as_str(),
                unit.branch.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    authorized_work_units == current_work_units
}

fn recovery_cleanup_revision(
    issue: &StudioRecoveryIssue,
    scope: &RecoveryCleanupScope,
    resources: &[StudioRecoveryCleanupResource],
) -> String {
    let mut digest = Sha256::new();
    digest.update(issue.id.as_bytes());
    digest.update(scope.project_updated_at.unwrap_or_default().to_le_bytes());
    digest.update(scope.thread_updated_at.unwrap_or_default().to_le_bytes());
    let mut resource_index = 0;
    for cleanup in &scope.runs {
        digest.update(cleanup.run.id.as_bytes());
        digest.update(cleanup.run.updated_at.to_le_bytes());
        for unit in &cleanup.work_units {
            let resource = &resources[resource_index];
            resource_index += 1;
            digest.update(unit.id.as_bytes());
            digest.update(unit.kind().as_str().as_bytes());
            digest.update(unit.worktree_disposition().as_str().as_bytes());
            digest.update(resource.path.as_bytes());
            digest.update(resource.branch.as_bytes());
            digest.update([resource.registration_exists as u8]);
            digest.update([resource.path_exists as u8]);
            digest.update([resource.branch_exists as u8]);
            digest.update(
                resource
                    .branch_head
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes(),
            );
            digest.update([resource.dirty as u8]);
            digest.update(resource.ahead_by.to_le_bytes());
            digest.update(resource.changed_file_count.to_le_bytes());
        }
    }
    format!("{:x}", digest.finalize())
}
