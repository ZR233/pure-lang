use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::tool::{SubagentContext, Tool, ToolRegistry};
use crate::{
    AgentKernel, AgentKernelToolRequest, CompileMode, CoreAgentProfile, PureCoreBuilder,
    StudioStore, ToolEffect, TurnExecutionProfile,
};

#[tokio::test]
async fn clean_committed_delivery_persists_exact_receipt_and_completes_records() {
    let fixture = DeliveryFixture::new("delivery-success", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(
        fixture.worktree.join("src/lib.rs"),
        "pub fn delivered() {}\n",
    )
    .unwrap();
    git(&fixture.worktree, &["add", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture
        .coordinator
        .submit_delivery(
            &fixture.subagent,
            &fixture.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .unwrap();

    assert_eq!(
        delivery,
        AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: std::fs::canonicalize(&fixture.worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: fixture.branch.clone(),
            },
            base_commit: fixture.base_commit.clone(),
            head_commit: head,
            changed_files: vec!["src/lib.rs".to_string()],
            verification_summary: "cargo test passed".to_string(),
        }
    );
    let outcome = fixture.outcome().await;
    let work_unit = fixture.work_unit().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.delivery, Some(delivery));
    assert_eq!(work_unit.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn repeated_successful_delivery_does_not_reopen_completed_records() {
    let fixture = DeliveryFixture::new("repeat-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot be submitted again");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_retry_after_success_does_not_downgrade_or_record_error() {
    let fixture = DeliveryFixture::new("invalid-retry-after-success", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let delivered = fixture.submit(&head).await.unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "dirty retry\n").unwrap();

    let error = fixture
        .submit(&head)
        .await
        .expect_err("completed delivery cannot enter waiting state");

    assert!(error.to_string().contains("already finalized"));
    let outcome = fixture.outcome().await;
    assert_eq!(outcome.status, AgentOutcomeStatus::Completed);
    assert_eq!(outcome.error, None);
    assert_eq!(outcome.delivery, Some(delivered));
    assert_eq!(fixture.work_unit().await.status, WorkUnitStatus::Delivered);
    fixture.cleanup();
}

#[tokio::test]
async fn dirty_tracked_and_untracked_deliveries_wait_for_retry() {
    for (name, untracked) in [("dirty-tracked", false), ("dirty-untracked", true)] {
        let fixture = DeliveryFixture::new(name, vec!["src/**"]).await;
        std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
        std::fs::write(fixture.worktree.join("src/lib.rs"), "committed\n").unwrap();
        git(&fixture.worktree, &["add", "src/lib.rs"]);
        git(&fixture.worktree, &["commit", "-m", "deliver"]);
        let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
        let dirty_path = if untracked {
            "src/untracked.rs"
        } else {
            "src/lib.rs"
        };
        std::fs::write(fixture.worktree.join(dirty_path), "dirty\n").unwrap();

        let error = fixture.submit(&head).await.expect_err("dirty delivery");

        assert!(error.to_string().contains("clean working tree"));
        fixture.assert_waiting().await;
        fixture.cleanup();
    }
}

#[tokio::test]
async fn invalid_head_and_scope_deliveries_wait_for_retry() {
    let unchanged = DeliveryFixture::new("unchanged-head", vec!["src/**"]).await;
    let error = unchanged
        .submit(&unchanged.base_commit)
        .await
        .expect_err("unchanged HEAD");
    assert!(error.to_string().contains("must advance beyond base"));
    unchanged.assert_waiting().await;
    unchanged.cleanup();

    let mismatch = DeliveryFixture::new("head-mismatch", vec!["src/**"]).await;
    mismatch.commit_file("src/lib.rs");
    let error = mismatch
        .submit("0000000000000000000000000000000000000000")
        .await
        .expect_err("supplied HEAD mismatch");
    assert!(error.to_string().contains("does not match worktree HEAD"));
    mismatch.assert_waiting().await;
    mismatch.cleanup();

    let out_of_scope = DeliveryFixture::new("out-of-scope", vec!["src/**"]).await;
    out_of_scope.commit_file("design/notes.md");
    let head = git_output(&out_of_scope.worktree, &["rev-parse", "HEAD"]);
    let error = out_of_scope
        .submit(&head)
        .await
        .expect_err("out-of-scope delivery");
    assert!(error.to_string().contains("outside ownedPaths"));
    out_of_scope.assert_waiting().await;
    out_of_scope.cleanup();
}

#[tokio::test]
async fn delivery_head_must_descend_from_the_assigned_base() {
    let fixture = DeliveryFixture::new("diverged-head", vec!["src/**"]).await;
    std::fs::remove_file(fixture.worktree.join("README.md")).unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "diverged\n").unwrap();
    git(&fixture.worktree, &["add", "-A"]);
    let tree = git_output(&fixture.worktree, &["write-tree"]);
    let head = git_output(
        &fixture.worktree,
        &["commit-tree", &tree, "-m", "diverged delivery"],
    );
    git(&fixture.worktree, &["reset", "--hard", &head]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("delivery must descend from base");

    assert!(error.to_string().contains("descend from base"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_uses_work_unit_base_after_task_expected_head_advances() {
    let fixture = DeliveryFixture::new("stable-work-unit-base", vec!["src/**"]).await;
    std::fs::write(fixture.repository.join("other.txt"), "other delivery\n").unwrap();
    git(&fixture.repository, &["add", "other.txt"]);
    git(
        &fixture.repository,
        &["commit", "-m", "merge other delivery"],
    );
    let advanced_head = git_output(&fixture.repository, &["rev-parse", "HEAD"]);
    assert!(
        fixture
            .store
            .compare_and_set_task_head(&fixture.task_run_id, &fixture.base_commit, &advanced_head,)
            .await
            .unwrap()
    );
    fixture.commit_file("src/lib.rs");
    let executor_head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&executor_head).await.unwrap();

    assert_eq!(delivery.base_commit, fixture.base_commit);
    assert_eq!(delivery.changed_files, vec!["src/lib.rs".to_string()]);
    fixture.cleanup();
}

#[tokio::test]
async fn delivery_rejects_main_workspace_and_other_worktree_from_same_repository() {
    let main_workspace = DeliveryFixture::new("reject-main-workspace", vec!["src/**"]).await;
    let main_head = git_output(&main_workspace.repository, &["rev-parse", "HEAD"]);
    let error = main_workspace
        .coordinator
        .submit_delivery(
            &main_workspace.subagent,
            &main_workspace.repository,
            &main_head,
            "cargo test passed",
        )
        .await
        .expect_err("planner workspace is not the assigned executor worktree");
    assert!(error.to_string().contains("assigned worktree"));
    main_workspace.assert_waiting().await;
    main_workspace.cleanup();

    let other_worktree = DeliveryFixture::new("reject-other-worktree", vec!["src/**"]).await;
    let other_path = other_worktree.repository.with_extension("other-worktree");
    let other_path_text = other_path.to_string_lossy().to_string();
    git(
        &other_worktree.repository,
        &[
            "worktree",
            "add",
            "-b",
            "unassigned-worktree",
            &other_path_text,
            &other_worktree.base_commit,
        ],
    );
    std::fs::create_dir_all(other_path.join("src")).unwrap();
    std::fs::write(other_path.join("src/lib.rs"), "unassigned\n").unwrap();
    git(&other_path, &["add", "src/lib.rs"]);
    git(&other_path, &["commit", "-m", "unassigned delivery"]);
    let other_head = git_output(&other_path, &["rev-parse", "HEAD"]);
    let error = other_worktree
        .coordinator
        .submit_delivery(
            &other_worktree.subagent,
            &other_path,
            &other_head,
            "cargo test passed",
        )
        .await
        .expect_err("other worktree is not assigned");
    assert!(error.to_string().contains("assigned worktree"));
    other_worktree.assert_waiting().await;
    let _ = Command::new("git")
        .arg("-C")
        .arg(&other_worktree.repository)
        .args(["worktree", "remove", "--force", &other_path_text])
        .output();
    remove_repository(other_path);
    other_worktree.cleanup();

    let subdirectory = DeliveryFixture::new("reject-worktree-subdirectory", vec!["src/**"]).await;
    subdirectory.commit_file("src/lib.rs");
    let head = git_output(&subdirectory.worktree, &["rev-parse", "HEAD"]);
    let error = subdirectory
        .coordinator
        .submit_delivery(
            &subdirectory.subagent,
            subdirectory.worktree.join("src"),
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("caller path must be the assigned worktree root");
    assert!(error.to_string().contains("assigned worktree"));
    subdirectory.assert_waiting().await;
    subdirectory.cleanup();
}

#[tokio::test]
async fn rename_from_outside_owned_paths_is_rejected() {
    let fixture = DeliveryFixture::new("rename-outside-owned-paths", vec!["src/**"]).await;
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    git(&fixture.worktree, &["mv", "README.md", "src/README.md"]);
    git(
        &fixture.worktree,
        &["commit", "-m", "rename into owned path"],
    );
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let error = fixture
        .submit(&head)
        .await
        .expect_err("rename source is outside owned paths");

    assert!(error.to_string().contains("README.md"));
    fixture.assert_waiting().await;
    fixture.cleanup();
}

#[tokio::test]
async fn invalid_owned_path_wrong_role_and_attempt_four_wait_for_retry() {
    for (name, owned_path) in [
        ("traversal-owned-path", "../escape/**"),
        ("absolute-owned-path", "C:/escape/**"),
    ] {
        let invalid_path = DeliveryFixture::new(name, vec![owned_path]).await;
        invalid_path.commit_file("src/lib.rs");
        let head = git_output(&invalid_path.worktree, &["rev-parse", "HEAD"]);
        let error = invalid_path
            .submit(&head)
            .await
            .expect_err("invalid owned path");
        assert!(error.to_string().contains("invalid owned path"));
        invalid_path.assert_waiting().await;
        invalid_path.cleanup();
    }

    let wrong_role = DeliveryFixture::new("wrong-role", vec!["src/**"]).await;
    wrong_role.commit_file("src/lib.rs");
    let head = git_output(&wrong_role.worktree, &["rev-parse", "HEAD"]);
    let mut explorer = wrong_role.subagent.clone();
    explorer.role = "explorer".to_string();
    let error = wrong_role
        .coordinator
        .submit_delivery(&explorer, &wrong_role.worktree, &head, "cargo test passed")
        .await
        .expect_err("wrong role");
    assert!(error.to_string().contains("executor"));
    wrong_role.assert_waiting().await;
    wrong_role.cleanup();

    let attempt_four = DeliveryFixture::new_with_attempt("attempt-four", vec!["src/**"], 4).await;
    attempt_four.commit_file("src/lib.rs");
    let head = git_output(&attempt_four.worktree, &["rev-parse", "HEAD"]);
    let error = attempt_four.submit(&head).await.expect_err("attempt four");
    assert!(error.to_string().contains("attempt must be within 1..=3"));
    attempt_four.assert_waiting().await;
    attempt_four.cleanup();
}

#[tokio::test]
async fn wrong_owner_missing_work_unit_and_empty_summary_are_actionable() {
    let wrong_owner = DeliveryFixture::new("wrong-owner", vec!["src/**"]).await;
    wrong_owner.commit_file("src/lib.rs");
    let head = git_output(&wrong_owner.worktree, &["rev-parse", "HEAD"]);
    let mut other_owner = wrong_owner.subagent.clone();
    other_owner.parent_id = Some("other-planner".to_string());
    let error = wrong_owner
        .coordinator
        .submit_delivery(
            &other_owner,
            &wrong_owner.worktree,
            &head,
            "cargo test passed",
        )
        .await
        .expect_err("wrong owner");
    assert!(error.to_string().contains("does not own this task outcome"));
    wrong_owner.assert_waiting().await;
    wrong_owner.cleanup();

    let missing = DeliveryFixture::new_without_work_unit("missing-work-unit", vec!["src/**"]).await;
    missing.commit_file("src/lib.rs");
    let head = git_output(&missing.worktree, &["rev-parse", "HEAD"]);
    let error = missing.submit(&head).await.expect_err("missing work unit");
    assert!(error.to_string().contains("no work unit"));
    assert_eq!(missing.outcome().await.status, AgentOutcomeStatus::Running);
    missing.cleanup();

    let empty_summary = DeliveryFixture::new("empty-summary", vec!["src/**"]).await;
    empty_summary.commit_file("src/lib.rs");
    let head = git_output(&empty_summary.worktree, &["rev-parse", "HEAD"]);
    let error = empty_summary
        .coordinator
        .submit_delivery(
            &empty_summary.subagent,
            &empty_summary.worktree,
            &head,
            "  ",
        )
        .await
        .expect_err("empty verification summary");
    assert!(error.to_string().contains("verificationSummary"));
    empty_summary.assert_waiting().await;
    empty_summary.cleanup();
}

#[tokio::test]
async fn exact_and_backslash_directory_owned_paths_are_normalized() {
    let fixture = DeliveryFixture::new("owned-path-shapes", vec!["README.md", r"src\**"]).await;
    std::fs::write(fixture.worktree.join("README.md"), "updated\n").unwrap();
    std::fs::create_dir_all(fixture.worktree.join("src")).unwrap();
    std::fs::write(fixture.worktree.join("src/lib.rs"), "delivered\n").unwrap();
    git(&fixture.worktree, &["add", "README.md", "src/lib.rs"]);
    git(&fixture.worktree, &["commit", "-m", "deliver"]);
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);

    let delivery = fixture.submit(&head).await.unwrap();

    assert_eq!(
        delivery.changed_files,
        vec!["README.md".to_string(), "src/lib.rs".to_string()]
    );
    fixture.cleanup();
}

#[tokio::test]
async fn submit_delivery_tool_has_typed_schema_branch_effect_and_role_visibility() {
    let coordinator = Arc::new(TaskCoordinator::new(
        StudioStore::open_memory().await.unwrap(),
    ));
    let tool = coordinator.submit_delivery_tool();

    assert_eq!(tool.name(), "submit_delivery");
    assert_eq!(tool.effect(), Some(ToolEffect::BranchControl));
    assert_eq!(
        tool.input_schema(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "headCommit": { "type": "string" },
                "verificationSummary": { "type": "string" }
            },
            "required": ["headCommit", "verificationSummary"],
            "additionalProperties": false
        })
    );

    let mut registry = ToolRegistry::new();
    registry.register(tool);
    let visible = |profile| {
        registry
            .schemas_for_profile(profile)
            .into_iter()
            .map(|schema| schema.name().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "executor"
        )),
        vec!["submit_delivery".to_string()]
    );
    assert!(visible(TurnExecutionProfile::root(CompileMode::Task)).is_empty());
    assert!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "explorer"
        ))
        .is_empty()
    );
    assert!(
        visible(TurnExecutionProfile::for_subagent(
            CompileMode::Task,
            "reviewer"
        ))
        .is_empty()
    );
}

#[tokio::test]
async fn child_dispatch_resolves_delivery_without_task_session_in_tool_input() {
    let fixture = DeliveryFixture::new("delivery-tool-handler", vec!["src/**"]).await;
    fixture.commit_file("src/lib.rs");
    let head = git_output(&fixture.worktree, &["rev-parse", "HEAD"]);
    let tool = fixture.coordinator.submit_delivery_tool();
    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let kernel = AgentKernel::builder(
        PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None)).unwrap(),
    )
    .with_profile(CoreAgentProfile::host_provided(fixture.worktree.clone()))
    .with_registered_tool(tool)
    .with_subagent_context(fixture.subagent.clone())
    .build()
    .await;

    let output = kernel
        .execute_tool(
            AgentKernelToolRequest::new(
                "submit_delivery",
                serde_json::json!({
                    "headCommit": head,
                    "verificationSummary": "cargo test passed"
                }),
                "child-turn-not-task-session",
                "call-submit",
                event_tx,
            )
            .with_mode(CompileMode::Task),
        )
        .await
        .unwrap();
    let delivery: AgentDelivery = serde_json::from_str(&output.description).unwrap();

    assert_eq!(delivery, fixture.outcome().await.delivery.unwrap());
    fixture.cleanup();
}

struct DeliveryFixture {
    coordinator: Arc<TaskCoordinator>,
    store: StudioStore,
    task_run_id: String,
    work_unit_id: String,
    outcome_id: String,
    repository: PathBuf,
    worktree: PathBuf,
    branch: String,
    base_commit: String,
    subagent: SubagentContext,
}

impl DeliveryFixture {
    async fn new(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, true).await
    }

    async fn new_with_attempt(name: &str, owned_paths: Vec<&str>, attempt: u32) -> Self {
        Self::new_configured(name, owned_paths, attempt, true).await
    }

    async fn new_without_work_unit(name: &str, owned_paths: Vec<&str>) -> Self {
        Self::new_configured(name, owned_paths, 1, false).await
    }

    async fn new_configured(
        name: &str,
        owned_paths: Vec<&str>,
        attempt: u32,
        link_work_unit: bool,
    ) -> Self {
        let repository = init_repository(name);
        let worktree = repository.with_extension("executor-worktree");
        let store = task_store(&repository).await;
        let session = task_session(&store, &repository).await;
        let coordinator = Arc::new(TaskCoordinator::new(store.clone()));
        let run = coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap();
        let branch = format!("pure-task-{}-agent-1", run.id);
        let worktree_text = worktree.to_string_lossy().to_string();
        git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &worktree_text,
                &run.base_commit,
            ],
        );
        let work_unit = store
            .create_work_unit(CreateWorkUnit {
                task_run_id: run.id.clone(),
                title: "Implement delivery".to_string(),
                owned_paths: owned_paths.into_iter().map(str::to_string).collect(),
                base_commit: run.expected_head.clone(),
                worktree_path: std::fs::canonicalize(&worktree)
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                branch: branch.clone(),
                attempt,
            })
            .await
            .unwrap();
        let work_unit = store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Running,
                Some("agent-1".to_string()),
            )
            .await
            .unwrap();
        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: Some("root".to_string()),
            agent_path: Some("root/agent-1".to_string()),
            role: "executor".to_string(),
            task: "Implement delivery".to_string(),
            depth: 1,
        };
        let task_run_id = run.id.clone();
        let outcome = store
            .create_agent_outcome(CreateAgentOutcome {
                task_run_id: run.id,
                work_unit_id: link_work_unit.then(|| work_unit.id.clone()),
                agent_id: subagent.id.clone(),
                owner_path: "root".to_string(),
                initiated_by: "planner".to_string(),
                requested_by_call_id: "call-spawn".to_string(),
                role: subagent.role.clone(),
                status: AgentOutcomeStatus::Running,
                attempt,
            })
            .await
            .unwrap();
        Self {
            coordinator,
            store,
            task_run_id,
            work_unit_id: work_unit.id,
            outcome_id: outcome.id,
            repository,
            worktree,
            branch,
            base_commit: run.base_commit,
            subagent,
        }
    }

    fn commit_file(&self, relative_path: &str) {
        let path = self.worktree.join(relative_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "delivered\n").unwrap();
        git(&self.worktree, &["add", relative_path]);
        git(&self.worktree, &["commit", "-m", "deliver"]);
    }

    async fn submit(&self, head: &str) -> anyhow::Result<AgentDelivery> {
        self.coordinator
            .submit_delivery(&self.subagent, &self.worktree, head, "cargo test passed")
            .await
    }

    async fn outcome(&self) -> AgentOutcomeRecord {
        self.store
            .list_agent_outcomes(&self.task_run_id)
            .await
            .unwrap()
            .into_iter()
            .find(|outcome| outcome.id == self.outcome_id)
            .unwrap()
    }

    async fn work_unit(&self) -> WorkUnitRecord {
        self.store
            .read_work_unit(&self.work_unit_id)
            .await
            .unwrap()
            .unwrap()
    }

    async fn assert_waiting(&self) {
        assert_eq!(
            self.outcome().await.status,
            AgentOutcomeStatus::WaitingForDelivery
        );
        assert_eq!(
            self.work_unit().await.status,
            WorkUnitStatus::WaitingForDelivery
        );
    }

    fn cleanup(self) {
        drop(self.coordinator);
        let worktree_text = self.worktree.to_string_lossy().to_string();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(["worktree", "remove", "--force", &worktree_text])
            .output();
        remove_repository(self.worktree);
        remove_repository(self.repository);
    }
}

#[tokio::test]
async fn task_start_requires_clean_named_branch() {
    let repository = init_repository("start-guards");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store);

    std::fs::write(repository.join("dirty.txt"), "dirty").unwrap();
    let dirty = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("dirty repository must be rejected");
    assert!(dirty.to_string().contains("clean working tree"));

    std::fs::remove_file(repository.join("dirty.txt")).unwrap();
    git(&repository, &["checkout", "--detach"]);
    let detached = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .expect_err("detached HEAD must be rejected");
    assert!(detached.to_string().contains("detached HEAD"));

    remove_repository(repository);
}

#[tokio::test]
async fn task_head_drift_blocks_the_persisted_run() {
    let repository = init_repository("head-drift");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let coordinator = TaskCoordinator::new(store.clone());
    let run = coordinator
        .start_confirmed_task(&session.id, "plan", &repository)
        .await
        .unwrap();

    let competing = TaskCoordinator::new(store.clone());
    let lease_error = competing
        .start_confirmed_task(&session.id, "another plan", &repository)
        .await
        .expect_err("one process may not own the branch twice");
    assert!(lease_error.to_string().contains("already owned"));

    std::fs::write(repository.join("external.txt"), "external").unwrap();
    git(&repository, &["add", "external.txt"]);
    git(&repository, &["commit", "-m", "external change"]);

    assert!(!coordinator.verify_expected_head(&run.id).await.unwrap());
    let blocked = store.read_task_run(&run.id).await.unwrap().unwrap();
    assert_eq!(blocked.phase, TaskRunPhase::Blocked);
    assert!(
        blocked
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("HEAD drifted")
    );

    coordinator
        .finish_task(
            &run.id,
            TaskRunPhase::Cancelled,
            Some("stopped".to_string()),
        )
        .await
        .unwrap();
    assert!(store.read_branch_lease(&run.id).await.unwrap().is_none());
    remove_repository(repository);
}

#[tokio::test]
async fn coordinator_recovers_active_task_after_restart() {
    let repository = init_repository("recovery");
    let store = task_store(&repository).await;
    let session = task_session(&store, &repository).await;
    let run = {
        let coordinator = TaskCoordinator::new(store.clone());
        coordinator
            .start_confirmed_task(&session.id, "plan", &repository)
            .await
            .unwrap()
    };

    let recovered_coordinator = TaskCoordinator::new(store.clone());
    let recovered = recovered_coordinator.recover_active_tasks().await.unwrap();

    assert_eq!(recovered, vec![run.clone()]);
    assert!(
        recovered_coordinator
            .verify_expected_head(&run.id)
            .await
            .unwrap()
    );
    recovered_coordinator
        .finish_task(&run.id, TaskRunPhase::Cancelled, None)
        .await
        .unwrap();
    remove_repository(repository);
}

async fn task_store(repository: &Path) -> StudioStore {
    let store = StudioStore::open_memory().await.unwrap();
    store.upsert_project(repository).await.unwrap();
    store
}

async fn task_session(store: &StudioStore, repository: &Path) -> crate::studio::SessionRecord {
    let project = store.upsert_project(repository).await.unwrap();
    store
        .create_session(&project.id, "Task", CompileMode::Task)
        .await
        .unwrap()
}

fn init_repository(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "pure-task-coordinator-{name}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init"]);
    git(&path, &["checkout", "-b", "main"]);
    git(&path, &["config", "user.email", "pure@example.invalid"]);
    git(&path, &["config", "user.name", "Pure Test"]);
    std::fs::write(path.join("README.md"), "initial\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-m", "initial"]);
    path
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
