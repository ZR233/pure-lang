use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use pl_core::{AgentIdentity, AgentWorkspace, WorkspaceMutability, resolve_workspace_root};

use crate::StudioMode;
use crate::config::StudioRole;
use crate::studio::StudioStore;
use crate::studio::records::{ProjectRecord, ThreadRecord};
use crate::studio::task_coordinator::{
    ReviewScope, TaskRunPhase, TaskRunRecord, WorkCompletionRecord,
};

/// 从 Studio durable owner 解析单个 Agent 的 canonical workspace。
#[derive(Clone)]
pub(super) struct AgentWorkspaceResolver {
    store: StudioStore,
}

impl AgentWorkspaceResolver {
    pub(super) fn new(store: StudioStore) -> Self {
        Self { store }
    }

    pub(super) async fn resolve(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
        project: &ProjectRecord,
        active_task_run: Option<&TaskRunRecord>,
    ) -> Result<AgentWorkspace> {
        let mode = StudioMode::from_label(&thread.mode);
        if identity.parent_id.is_none() {
            return self.resolve_root(mode, project, active_task_run).await;
        }
        match identity.role.as_str() {
            role if role == StudioRole::Executor.key() && mode == StudioMode::Task => {
                self.resolve_executor(identity, thread).await
            }
            role if role == StudioRole::Reviewer.key() && mode == StudioMode::Task => {
                self.resolve_reviewer(identity, thread).await
            }
            role if role == StudioRole::Explorer.key() => {
                let root = project_workspace(project)?;
                Ok(AgentWorkspace::confined(
                    root,
                    WorkspaceMutability::ReadOnly,
                ))
            }
            role => bail!("unsupported Studio child role for workspace resolution: {role}"),
        }
    }

    async fn resolve_root(
        &self,
        mode: StudioMode,
        project: &ProjectRecord,
        active_task_run: Option<&TaskRunRecord>,
    ) -> Result<AgentWorkspace> {
        let root = project_workspace(project)?;
        match mode {
            StudioMode::Simple => Ok(AgentWorkspace::local(root)),
            StudioMode::Task => {
                if let Some(run) = active_task_run {
                    validate_main_workspace(&root, run).await?;
                }
                let mutability =
                    if active_task_run.is_some_and(|run| run.phase == TaskRunPhase::Merging) {
                        WorkspaceMutability::ReadWrite
                    } else {
                        WorkspaceMutability::ReadOnly
                    };
                Ok(AgentWorkspace::confined(root, mutability))
            }
        }
    }

    async fn resolve_executor(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
    ) -> Result<AgentWorkspace> {
        let work_unit = self
            .store
            .find_work_unit_for_executor(identity.id.as_str())
            .await?
            .with_context(|| format!("executor {} has no durable WorkUnit owner", identity.id))?;
        let run = self
            .store
            .read_task_run(&work_unit.task_run_id)
            .await?
            .context("executor WorkUnit task run not found")?;
        ensure!(
            run.root_thread_id == thread.root_thread_id,
            "executor WorkUnit belongs to another TaskRun root"
        );
        let root =
            validate_child_workspace(&work_unit.worktree_path, &work_unit.branch, &run, false)
                .await?;
        Ok(AgentWorkspace::confined(
            root,
            WorkspaceMutability::ReadWrite,
        ))
    }

    async fn resolve_reviewer(
        &self,
        identity: &AgentIdentity,
        thread: &ThreadRecord,
    ) -> Result<AgentWorkspace> {
        let round = self
            .store
            .find_review_round_for_reviewer(identity.id.as_str())
            .await?
            .with_context(|| {
                format!("reviewer {} has no durable ReviewRound owner", identity.id)
            })?;
        let run = self
            .store
            .read_task_run(&round.task_run_id)
            .await?
            .context("review round TaskRun not found")?;
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
                let completion = self
                    .store
                    .read_work_completion(completion_id)
                    .await?
                    .context("delivery review completion not found")?;
                validate_delivery_target(&round, &completion)?;
                let root = validate_child_workspace(
                    &completion.worktree_path,
                    &completion.branch,
                    &run,
                    true,
                )
                .await?;
                let snapshot =
                    crate::studio::task_coordinator::git::inspect_repository(&root, true).await?;
                let expected_head = completion
                    .head_commit
                    .as_deref()
                    .unwrap_or(completion.base_commit.as_str());
                ensure!(
                    snapshot.head == expected_head && round.reviewed_head == expected_head,
                    "delivery review worktree HEAD no longer matches its Completion revision"
                );
                root
            }
            ReviewScope::Integrated => {
                let root = validate_main_workspace(Path::new(&run.workspace_root), &run).await?;
                let snapshot =
                    crate::studio::task_coordinator::git::inspect_repository(&root, true).await?;
                ensure!(
                    snapshot.head == run.expected_head && round.reviewed_head == run.expected_head,
                    "integrated review no longer matches the TaskRun HEAD"
                );
                root
            }
        };
        Ok(AgentWorkspace::confined(
            root,
            WorkspaceMutability::ReadOnly,
        ))
    }
}

fn project_workspace(project: &ProjectRecord) -> Result<PathBuf> {
    resolve_workspace_root(Path::new(&project.path)).map_err(anyhow::Error::from)
}

async fn validate_main_workspace(root: &Path, run: &TaskRunRecord) -> Result<PathBuf> {
    let root = resolve_workspace_root(root).map_err(anyhow::Error::from)?;
    ensure!(
        same_path(&root, Path::new(&run.workspace_root)),
        "TaskRun main workspace does not match the project workspace"
    );
    validate_repository_identity(&root, &run.git_common_dir, &run.branch, false).await?;
    Ok(root)
}

async fn validate_child_workspace(
    stored_path: &str,
    expected_branch: &str,
    run: &TaskRunRecord,
    require_clean: bool,
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
    validate_repository_identity(&root, &run.git_common_dir, expected_branch, require_clean)
        .await?;
    Ok(root)
}

async fn validate_repository_identity(
    root: &Path,
    expected_common_dir: &str,
    expected_branch: &str,
    require_clean: bool,
) -> Result<()> {
    let snapshot =
        crate::studio::task_coordinator::git::inspect_repository(root, require_clean).await?;
    ensure!(
        same_path(&snapshot.workspace_root, root),
        "Git top-level does not match the canonical Agent workspace"
    );
    ensure!(
        same_path(&snapshot.git_common_dir, Path::new(expected_common_dir)),
        "Agent workspace Git common directory does not match its TaskRun"
    );
    ensure!(
        snapshot.branch == expected_branch,
        "Agent workspace branch does not match its durable owner"
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pl_core::{AgentId, WorkspaceBoundary};

    use super::*;
    use crate::studio::agent_host::resources::StudioAgentResources;
    use crate::studio::records::{ThreadKind, ThreadVisibility};
    use crate::studio::task_coordinator::{
        AgentDelivery, AgentWorktreeDelivery, CreateWorkUnit, TaskCoordinator, WorkCompletionKind,
        WorkUnitStatus,
    };

    #[tokio::test]
    async fn executor_workspace_uses_durable_owner_without_in_memory_resources() {
        let fixture = ResolverFixture::new("durable-executor").await;
        let identity = fixture.child_identity(&fixture.executor_agent_id, StudioRole::Executor);
        let thread = fixture.child_thread(&fixture.executor_agent_id, StudioRole::Executor);
        let resources = StudioAgentResources::default();
        assert!(resources.get(&identity.id).await.is_none());

        let workspace = fixture
            .resolver()
            .resolve(&identity, &thread, &fixture.project, None)
            .await
            .unwrap();

        assert!(same_path(workspace.root(), &fixture.worktree));
        assert_eq!(workspace.boundary(), WorkspaceBoundary::Confined);
        assert_eq!(workspace.mutability(), WorkspaceMutability::ReadWrite);
        fixture.cleanup();
    }

    #[tokio::test]
    async fn delivery_reviewer_uses_executor_worktree_read_only_and_requires_clean_state() {
        let fixture = ResolverFixture::new("delivery-review-workspace").await;
        std::fs::write(fixture.worktree.join("sentinel.txt"), "executor\n").unwrap();
        git(&fixture.worktree, &["add", "sentinel.txt"]);
        git(&fixture.worktree, &["commit", "-m", "executor delivery"]);
        let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        fixture
            .store
            .create_work_completion(
                &fixture.work_unit_id,
                WorkCompletionKind::Delivery,
                Some(&AgentDelivery {
                    worktree: AgentWorktreeDelivery {
                        path: canonical_text(&fixture.worktree),
                        branch: fixture.branch.clone(),
                    },
                    base_commit: fixture.run.base_commit.clone(),
                    head_commit: head,
                    changed_files: vec!["sentinel.txt".to_string()],
                    verification_summary: "focused checks passed".to_string(),
                }),
                "focused checks passed",
            )
            .await
            .unwrap();
        let round = fixture
            .store
            .begin_delivery_review(
                &fixture.root_thread.id,
                &fixture.executor_agent_id,
                "call-delivery-review",
            )
            .await
            .unwrap();
        let reviewer_agent_id = "reviewer-agent";
        fixture
            .store
            .authorize_reviewer_spawn(
                &fixture.root_thread.id,
                "call-delivery-review",
                reviewer_agent_id,
            )
            .await
            .unwrap();
        fixture
            .store
            .activate_reviewer(&round.id, reviewer_agent_id)
            .await
            .unwrap();
        let identity = fixture.child_identity(reviewer_agent_id, StudioRole::Reviewer);
        let thread = fixture.child_thread(reviewer_agent_id, StudioRole::Reviewer);

        let workspace = fixture
            .resolver()
            .resolve(&identity, &thread, &fixture.project, None)
            .await
            .unwrap();

        assert!(same_path(workspace.root(), &fixture.worktree));
        assert_eq!(workspace.mutability(), WorkspaceMutability::ReadOnly);
        assert_eq!(
            std::fs::read_to_string(workspace.root().join("sentinel.txt")).unwrap(),
            "executor\n"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.repository.join("sentinel.txt")).unwrap(),
            "main\n"
        );

        std::fs::write(fixture.worktree.join("dirty.txt"), "dirty\n").unwrap();
        assert!(
            fixture
                .resolver()
                .resolve(&identity, &thread, &fixture.project, None)
                .await
                .is_err(),
            "delivery reviewer must reject a Completion worktree that drifted dirty"
        );
        fixture.cleanup();
    }

    #[tokio::test]
    async fn task_root_is_writable_only_during_planner_merge_phase() {
        let fixture = ResolverFixture::new("planner-mutability").await;
        let identity = AgentIdentity {
            id: AgentId::new(fixture.root_thread.id.clone()).unwrap(),
            parent_id: None,
            role: StudioRole::Planner.id(),
            depth: 0,
        };

        let implementing = fixture
            .resolver()
            .resolve(
                &identity,
                &fixture.root_thread,
                &fixture.project,
                Some(&fixture.run),
            )
            .await
            .unwrap();
        assert_eq!(implementing.mutability(), WorkspaceMutability::ReadOnly);

        let merging = fixture
            .store
            .transition_task_run(&fixture.run.id, TaskRunPhase::Merging, None)
            .await
            .unwrap();
        let workspace = fixture
            .resolver()
            .resolve(
                &identity,
                &fixture.root_thread,
                &fixture.project,
                Some(&merging),
            )
            .await
            .unwrap();
        assert_eq!(workspace.boundary(), WorkspaceBoundary::Confined);
        assert_eq!(workspace.mutability(), WorkspaceMutability::ReadWrite);

        let explorer_id = "explorer-agent";
        let explorer = fixture.child_identity(explorer_id, StudioRole::Explorer);
        let explorer_thread = fixture.child_thread(explorer_id, StudioRole::Explorer);
        let explorer_workspace = fixture
            .resolver()
            .resolve(
                &explorer,
                &explorer_thread,
                &fixture.project,
                Some(&merging),
            )
            .await
            .unwrap();
        assert!(same_path(explorer_workspace.root(), &fixture.repository));
        assert_eq!(
            explorer_workspace.mutability(),
            WorkspaceMutability::ReadOnly
        );
        fixture.cleanup();
    }

    #[tokio::test]
    async fn child_workspace_resolution_fails_closed_for_invalid_durable_owners() {
        let fixture = ResolverFixture::new("invalid-child-owner").await;

        let unknown_id = "unknown-executor";
        let error = fixture
            .resolver()
            .resolve(
                &fixture.child_identity(unknown_id, StudioRole::Executor),
                &fixture.child_thread(unknown_id, StudioRole::Executor),
                &fixture.project,
                None,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no durable WorkUnit owner"));

        git(&fixture.worktree, &["switch", "-c", "pure-task-drifted"]);
        let error = fixture
            .resolve_executor(&fixture.executor_agent_id)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("branch does not match"));

        let missing_id = "missing-workspace";
        let missing_path = fixture
            .repository
            .join(".pure/worktrees")
            .join(&fixture.run.id)
            .join(missing_id);
        fixture
            .allocate_executor(
                missing_id,
                &missing_path,
                &format!("pure-task-{missing_id}"),
            )
            .await;
        assert!(fixture.resolve_executor(missing_id).await.is_err());

        let foreign_id = "foreign-common-dir";
        let foreign_path = fixture
            .repository
            .join(".pure/worktrees")
            .join(&fixture.run.id)
            .join(foreign_id);
        let foreign_branch = format!("pure-task-{foreign_id}");
        init_repository_at(&foreign_path, &foreign_branch);
        fixture
            .allocate_executor(foreign_id, &foreign_path, &foreign_branch)
            .await;
        let error = fixture.resolve_executor(foreign_id).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("common directory does not match")
        );

        let external_id = "external-workspace";
        let external_path = temporary_project("external-workspace");
        let external_branch = format!("pure-task-{external_id}");
        init_repository_at(&external_path, &external_branch);
        fixture
            .allocate_executor(external_id, &external_path, &external_branch)
            .await;
        let error = fixture.resolve_executor(external_id).await.unwrap_err();
        assert!(error.to_string().contains("outside .pure/worktrees"));
        remove_repository(external_path);

        fixture.cleanup();
    }

    struct ResolverFixture {
        store: StudioStore,
        coordinator: Arc<TaskCoordinator>,
        project: ProjectRecord,
        root_thread: ThreadRecord,
        run: TaskRunRecord,
        repository: PathBuf,
        worktree: PathBuf,
        branch: String,
        work_unit_id: String,
        executor_agent_id: String,
    }

    impl ResolverFixture {
        async fn new(name: &str) -> Self {
            let repository = temporary_project(name);
            init_repository_at(&repository, "main");
            let store = StudioStore::open_memory().await.unwrap();
            let project = store.upsert_project(&repository).await.unwrap();
            let root_thread = store
                .create_thread(&project.id, "Task", StudioMode::Task)
                .await
                .unwrap();
            let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
            let run = coordinator
                .start_confirmed_task(&root_thread.id, "plan", &repository)
                .await
                .unwrap();
            let run = store
                .transition_task_run(&run.id, TaskRunPhase::Implementing, None)
                .await
                .unwrap();
            let executor_agent_id = "executor-agent".to_string();
            let worktree = crate::agent::worktree::git_compatible_path(
                repository
                    .join(".pure/worktrees")
                    .join(&run.id)
                    .join(&executor_agent_id),
            );
            std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
            let branch = format!("pure-task-{}-{executor_agent_id}", run.id);
            let worktree_text = worktree.to_string_lossy().to_string();
            git(
                &repository,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &branch,
                    &worktree_text,
                    &run.expected_head,
                ],
            );
            let worktree = std::fs::canonicalize(worktree).unwrap();
            let work_unit = store
                .create_work_unit(CreateWorkUnit {
                    task_run_id: run.id.clone(),
                    title: "executor delivery".to_string(),
                    scope_hints: vec!["src".to_string()],
                    base_commit: run.expected_head.clone(),
                    worktree_path: canonical_text(&worktree),
                    branch: branch.clone(),
                    attempt: 1,
                })
                .await
                .unwrap();
            let work_unit = store
                .update_work_unit(
                    &work_unit.id,
                    WorkUnitStatus::Running,
                    Some(executor_agent_id.clone()),
                )
                .await
                .unwrap();
            store
                .activate_executor(&work_unit.id, &executor_agent_id)
                .await
                .unwrap();
            Self {
                store,
                coordinator,
                project,
                root_thread,
                run,
                repository,
                worktree,
                branch,
                work_unit_id: work_unit.id,
                executor_agent_id,
            }
        }

        fn resolver(&self) -> AgentWorkspaceResolver {
            AgentWorkspaceResolver::new(self.store.clone())
        }

        fn child_identity(&self, agent_id: &str, role: StudioRole) -> AgentIdentity {
            AgentIdentity {
                id: AgentId::new(agent_id).unwrap(),
                parent_id: Some(AgentId::new(self.root_thread.id.clone()).unwrap()),
                role: role.id(),
                depth: 1,
            }
        }

        fn child_thread(&self, agent_id: &str, role: StudioRole) -> ThreadRecord {
            ThreadRecord {
                id: agent_id.to_string(),
                project_id: self.root_thread.project_id.clone(),
                title: role.display_name().to_string(),
                mode: self.root_thread.mode.clone(),
                created_at: self.root_thread.created_at,
                updated_at: self.root_thread.updated_at,
                visibility: ThreadVisibility::Active,
                parent_thread_id: Some(self.root_thread.id.clone()),
                root_thread_id: self.root_thread.root_thread_id.clone(),
                thread_kind: ThreadKind::Agent,
                agent_path: agent_id.to_string(),
                role: role.key().to_string(),
                status: "running".to_string(),
                summary: None,
                error: None,
                runtime_updated_at: None,
            }
        }

        async fn allocate_executor(&self, agent_id: &str, path: &Path, branch: &str) {
            let work_unit = self
                .store
                .create_work_unit(CreateWorkUnit {
                    task_run_id: self.run.id.clone(),
                    title: agent_id.to_string(),
                    scope_hints: Vec::new(),
                    base_commit: self.run.expected_head.clone(),
                    worktree_path: path.to_string_lossy().to_string(),
                    branch: branch.to_string(),
                    attempt: 1,
                })
                .await
                .unwrap();
            let work_unit = self
                .store
                .update_work_unit(
                    &work_unit.id,
                    WorkUnitStatus::Running,
                    Some(agent_id.to_string()),
                )
                .await
                .unwrap();
            self.store
                .activate_executor(&work_unit.id, agent_id)
                .await
                .unwrap();
        }

        async fn resolve_executor(&self, agent_id: &str) -> Result<AgentWorkspace> {
            self.resolver()
                .resolve(
                    &self.child_identity(agent_id, StudioRole::Executor),
                    &self.child_thread(agent_id, StudioRole::Executor),
                    &self.project,
                    None,
                )
                .await
        }

        fn cleanup(self) {
            self.coordinator.suspend();
            let worktree = self.worktree.to_string_lossy().to_string();
            let _ = Command::new("git")
                .arg("-C")
                .arg(&self.repository)
                .args(["worktree", "remove", "--force", &worktree])
                .output();
            remove_repository(self.repository);
        }
    }

    fn init_repository_at(path: &Path, branch: &str) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init"]);
        git(path, &["checkout", "-b", branch]);
        git(path, &["config", "user.email", "pure@example.invalid"]);
        git(path, &["config", "user.name", "Pure Test"]);
        git(path, &["config", "core.autocrlf", "false"]);
        git(path, &["config", "commit.gpgSign", "false"]);
        std::fs::write(path.join("sentinel.txt"), "main\n").unwrap();
        git(path, &["add", "sentinel.txt"]);
        git(path, &["commit", "-m", "initial"]);
    }

    fn temporary_project(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "pure-agent-workspace-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn canonical_text(path: &Path) -> String {
        std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    fn git(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git_output(repository: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn remove_repository(path: PathBuf) {
        let _ = std::fs::remove_dir_all(path);
    }
}
