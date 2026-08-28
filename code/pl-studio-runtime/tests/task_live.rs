#[path = "task_fixture/git.rs"]
mod git;
#[path = "task_fixture/live.rs"]
mod live_fixture;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use git::git_output;
use live_fixture::{LIVE_TASK_PROMPT, LIVE_VERIFY_MARKER, LiveTaskFixture, command_output};
use pl_studio_runtime::{
    InteractionResolution, InteractionStatus, PlanConfirmationResolution,
    PlanConfirmationResolutionPayload, StudioHostKind, StudioReviewScope, StudioRuntime,
    StudioRuntimeOptions, StudioStore, StudioSubmitPromptOptions, StudioSubmitPromptRequest,
    StudioTaskCompletionContent, StudioTaskCompletionState, StudioTaskOutcome,
    StudioTaskReviewState, StudioTaskState, StudioTaskWorkUnitState,
    StudioWorkUnitCompletionOutcome, ThreadStatus,
};

const LIVE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio model configuration and incurs real model usage"]
async fn installed_config_task_mode_delivers_two_rust_workstreams_and_recovers() -> Result<()> {
    let fixture = LiveTaskFixture::new().await?;
    let result = tokio::time::timeout(LIVE_TIMEOUT, run_live_task_flow(&fixture))
        .await
        .context("live Task integration test exceeded the 30 minute timeout")
        .and_then(|result| result);
    if let Err(error) = &result {
        eprintln!(
            "live Task integration failed: {error:#}\n{}",
            fixture.diagnostics().await
        );
    }

    let shutdown = tokio::time::timeout(Duration::from_secs(30), fixture.shutdown())
        .await
        .context("Studio runtime shutdown timed out")
        .and_then(|result| result);
    let recovery = if result.is_ok() && shutdown.is_ok() {
        assert_reopened_activation(&fixture).await
    } else {
        Ok(())
    };
    let config_unchanged = fixture.assert_config_unchanged();

    result?;
    shutdown?;
    recovery?;
    config_unchanged
}

async fn run_live_task_flow(fixture: &LiveTaskFixture) -> Result<()> {
    fixture
        .runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: fixture.thread_id.clone(),
            input: pl_protocol::studio::StudioPromptInput {
                text: LIVE_TASK_PROMPT.trim().to_string(),
                attachment_draft_ids: Vec::new(),
            },
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;

    let confirmation = fixture.wait_for_plan_confirmation().await?;
    fixture.wait_for_no_active_turns().await?;
    if confirmation.status() != InteractionStatus::Pending {
        bail!(
            "plan confirmation was not pending: {:?}",
            confirmation.status()
        );
    }
    let resolution = fixture
        .runtime
        .resolve_interaction(
            confirmation.interaction_id,
            InteractionResolution::PlanConfirmation(PlanConfirmationResolutionPayload {
                decision: PlanConfirmationResolution::Confirm,
                content: None,
                reason: None,
            }),
        )
        .await?;
    if resolution.interaction.status() != InteractionStatus::Resolved {
        bail!(
            "plan confirmation did not resolve: {:?}",
            resolution.interaction.status()
        );
    }

    let task = fixture.wait_for_completed_task().await?;
    fixture.wait_for_no_active_turns().await?;
    assert_task_invariants(fixture, &task).await?;
    assert_delivered_fixture(fixture)?;
    write_delivery_artifacts(fixture, &task)?;
    Ok(())
}

async fn assert_task_invariants(
    fixture: &LiveTaskFixture,
    task: &pl_studio_runtime::StudioTaskRuntime,
) -> Result<()> {
    match &task.state {
        StudioTaskState::Completed(completed) => match &completed.outcome {
            StudioTaskOutcome::Succeeded { .. } => {}
            StudioTaskOutcome::Failed { summary, .. } => {
                bail!("Task completed with a failed outcome: {summary}")
            }
        },
        state => bail!("Task state is `{state:?}` instead of `completed`"),
    }
    if task.work_units.len() != 2 {
        bail!(
            "Task must create exactly two independent executor work units, got {}",
            task.work_units.len()
        );
    }
    if task.completions.len() != 2 {
        bail!(
            "Task must persist exactly two executor Completions, got {}",
            task.completions.len()
        );
    }
    for unit in &task.work_units {
        if !matches!(
            &unit.state,
            StudioTaskWorkUnitState::Completed(completed)
                if matches!(completed.outcome, StudioWorkUnitCompletionOutcome::Merged { .. })
        ) {
            bail!(
                "work unit `{}` finished with state `{:?}`",
                unit.id,
                unit.state
            );
        }
        if unit.implementation_step_count < 2
            || unit.acceptance_criterion_count == 0
            || unit.verification_count == 0
            || unit.blueprint_fingerprint.is_none()
            || unit.objective.as_deref().is_none_or(str::is_empty)
        {
            bail!(
                "work unit `{}` did not preserve a concrete covered implementation blueprint",
                unit.id
            );
        }
        if Path::new(&unit.worktree_path).exists() {
            bail!(
                "merged work unit `{}` retained worktree `{}`",
                unit.id,
                unit.worktree_path
            );
        }
        let executor_thread_id = unit
            .agent_id
            .as_deref()
            .context("merged work unit does not reference an executor Thread")?;
        let executor = fixture
            .store
            .read_thread(executor_thread_id)
            .await?
            .context("merged work unit references a missing executor Thread")?;
        if executor.status != ThreadStatus::Closed {
            bail!(
                "executor Thread `{executor_thread_id}` finished with status `{:?}`",
                executor.status
            );
        }
        if !task.merges.iter().any(|merge| {
            merge.work_unit_id == unit.id && merge.executor_agent_id == executor_thread_id
        }) {
            bail!("executor Thread `{executor_thread_id}` has no merged delivery");
        }
    }

    let scope_hints = fixture.successful_executor_scope_hints().await?;
    if scope_hints.is_empty() {
        bail!("no successful task_spawn_executor call was recorded");
    }
    for hints in &scope_hints {
        if hints.iter().any(|path| path.trim().is_empty()) {
            bail!("task_spawn_executor recorded an invalid scopeHints entry: {hints:?}");
        }
    }
    let expected_scope_hints = [
        ["src/normalize.rs", "tests/normalize.rs"],
        ["src/validate.rs", "tests/validate.rs"],
    ];
    for expected in expected_scope_hints {
        if !scope_hints.iter().any(|hints| {
            let mut actual = hints.iter().map(String::as_str).collect::<Vec<_>>();
            actual.sort_unstable();
            actual == expected
        }) {
            bail!("Task did not preserve executor scopeHints {expected:?}: {scope_hints:?}");
        }
    }
    for completion in &task.completions {
        let StudioTaskCompletionContent::Delivery(delivery) = &completion.content else {
            bail!("Task executor reported no delivery: {}", completion.id);
        };
        let mut changed = delivery
            .changed_files
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        changed.sort_unstable();
        if changed.as_slice() != expected_scope_hints[0]
            && changed.as_slice() != expected_scope_hints[1]
        {
            bail!(
                "executor completion `{}` escaped its exact two-file scope: {:?}",
                completion.id,
                delivery.changed_files
            );
        }
    }

    let recorded_merges = fixture.successful_task_record_merge_arguments().await?;
    if recorded_merges.len() != task.merges.len() {
        bail!(
            "task_record_merge call count {} does not match durable merge count {}",
            recorded_merges.len(),
            task.merges.len()
        );
    }
    let expected_head = task
        .merges
        .last()
        .context("completed delivery Task has no durable merge")?
        .resulting_head
        .as_str();
    for arguments in &recorded_merges {
        let executor_agent_id = arguments["executorAgentId"]
            .as_str()
            .context("task_record_merge has no executorAgentId")?;
        let completion_revision = arguments["completionRevision"]
            .as_u64()
            .context("task_record_merge has no completionRevision")?;
        let expected_previous_head = arguments["expectedPreviousHead"]
            .as_str()
            .context("task_record_merge has no expectedPreviousHead")?;
        let resulting_head = arguments["resultingHead"]
            .as_str()
            .context("task_record_merge has no resultingHead")?;
        let method = arguments["method"]
            .as_str()
            .context("task_record_merge has no method")?;
        let summary = arguments["summary"]
            .as_str()
            .context("task_record_merge has no summary")?;
        if expected_previous_head == resulting_head || summary.trim().is_empty() {
            bail!("task_record_merge did not describe a real Git integration");
        }
        if !matches!(
            method,
            "merge" | "cherryPick" | "squash" | "rebase" | "manual"
        ) {
            bail!("task_record_merge used unsupported method `{method}`");
        }
        if !task.completions.iter().any(|completion| {
            completion.executor_agent_id == executor_agent_id
                && u64::from(completion.revision) == completion_revision
        }) {
            bail!("task_record_merge does not match a durable Completion revision");
        }
        if !task.merges.iter().any(|merge| {
            merge.executor_agent_id == executor_agent_id && merge.resulting_head == resulting_head
        }) {
            bail!("task_record_merge does not match the durable MergeRecord projection");
        }
        git_output(
            &fixture.workspace,
            &[
                "merge-base",
                "--is-ancestor",
                expected_previous_head,
                resulting_head,
            ],
        )?;
        git_output(
            &fixture.workspace,
            &["merge-base", "--is-ancestor", resulting_head, expected_head],
        )?;
    }

    if task.completions.iter().any(|completion| {
        matches!(
            &completion.content,
            StudioTaskCompletionContent::Delivery(_)
        ) && matches!(&completion.state, StudioTaskCompletionState::Approved(_))
            && !task.merges.iter().any(|merge| {
                merge.completion_id == completion.id
                    && merge.completion_revision == completion.revision
            })
    }) {
        bail!("Task contains an approved but unmerged delivery");
    }

    match &task.integrated_review_gate {
        pl_studio_runtime::StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
            ..
        } => bail!("two-workstream Task incorrectly used the integrated-review exemption"),
        pl_studio_runtime::StudioIntegratedReviewGate::SatisfiedByReview {
            review_round_id,
            reviewed_head,
        } => {
            let review = task
                .reviews
                .iter()
                .find(|review| review.id == *review_round_id)
                .context("satisfied review gate references a missing round")?;
            if review.scope != StudioReviewScope::Integrated
                || !matches!(&review.state, StudioTaskReviewState::Passed { .. })
                || review.reviewed_head != *reviewed_head
                || reviewed_head != expected_head
            {
                bail!("integrated review gate does not match a passing current review");
            }
        }
        other => bail!("completed delivery task has invalid review gate: {other:?}"),
    }
    if !task.reviews.iter().all(|review| {
        review
            .design_references
            .iter()
            .any(|reference| reference.path == "design/task-workflows.md")
    }) {
        bail!("not every review cited design/task-workflows.md");
    }
    if !task.completions.iter().all(|completion| {
        !matches!(&completion.state, StudioTaskCompletionState::Approved(_))
            || task.reviews.iter().any(|review| {
                let completion_head = match &completion.content {
                    StudioTaskCompletionContent::Delivery(value) => value.head_commit.as_str(),
                    StudioTaskCompletionContent::NoDelivery(_) => "",
                };
                review.scope == StudioReviewScope::Delivery
                    && review.completion_id.as_deref() == Some(completion.id.as_str())
                    && review.completion_revision == Some(completion.revision)
                    && review.reviewed_head == completion_head
                    && matches!(&review.state, StudioTaskReviewState::Passed { .. })
            })
    }) {
        bail!("an approved completion has no matching passing delivery review");
    }

    if !fixture
        .store
        .list_pending_interactions(&fixture.thread_id)
        .await?
        .is_empty()
    {
        bail!("Task retained pending interactions");
    }
    if !fixture
        .runtime
        .runtime_snapshot()
        .await?
        .active_turns
        .is_empty()
    {
        bail!("Task retained active turns");
    }
    if git_output(&fixture.workspace, &["rev-parse", "HEAD"])? != expected_head {
        bail!("workspace HEAD does not match the final durable merge head");
    }
    if !git_output(&fixture.workspace, &["status", "--porcelain"])?.is_empty() {
        bail!("workspace Git tree is dirty");
    }
    git_output(
        &fixture.workspace,
        &["cat-file", "-e", "HEAD:design/task-workflows.md"],
    )
    .context("design/task-workflows.md was not committed at workspace HEAD")?;
    Ok(())
}

fn assert_delivered_fixture(fixture: &LiveTaskFixture) -> Result<()> {
    for path in [
        "src/normalize.rs",
        "src/validate.rs",
        "tests/normalize.rs",
        "tests/validate.rs",
        "design/task-workflows.md",
    ] {
        if !fixture.workspace.join(path).is_file() {
            bail!("required delivered file is missing: {path}");
        }
    }

    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-fixtures/task-live/workspace");
    for protected in [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "src/bin/fixture_verify.rs",
        "README.md",
        "AGENTS.md",
        ".gitignore",
        "docs/product-contract.md",
        "skills/task-fixture-rust/SKILL.md",
    ] {
        let expected = std::fs::read(fixture_root.join(protected))?;
        let actual = std::fs::read(fixture.workspace.join(protected))?;
        if actual != expected {
            bail!("Task modified protected fixture file `{protected}`");
        }
    }

    let tests = command_output(Some(&fixture.workspace), "cargo", &["test"])?;
    let verification = command_output(
        Some(&fixture.workspace),
        "cargo",
        &["run", "--quiet", "--bin", "fixture_verify"],
    )?;
    if !verification
        .lines()
        .any(|line| line.trim() == LIVE_VERIFY_MARKER)
    {
        bail!(
            "fixture verifier did not output the fixed success marker `{LIVE_VERIFY_MARKER}`\n\
             output:\n{verification}"
        );
    }
    std::fs::write(fixture.artifact_dir.join("cargo-test.stdout.txt"), tests)?;
    std::fs::write(
        fixture.artifact_dir.join("fixture-verify.stdout.txt"),
        format!("{verification}\n"),
    )?;
    Ok(())
}

fn write_delivery_artifacts(
    fixture: &LiveTaskFixture,
    task: &pl_studio_runtime::StudioTaskRuntime,
) -> Result<()> {
    std::fs::write(
        fixture.artifact_dir.join("task-runtime.json"),
        serde_json::to_vec_pretty(task)?,
    )?;
    std::fs::write(
        fixture.artifact_dir.join("git-head.txt"),
        format!(
            "{}\n",
            git_output(&fixture.workspace, &["rev-parse", "HEAD"])?
        ),
    )?;
    std::fs::write(
        fixture.artifact_dir.join("git-log.txt"),
        git_output(
            &fixture.workspace,
            &["log", "--oneline", "--decorate", "--all"],
        )?,
    )?;
    std::fs::write(
        fixture.artifact_dir.join("git-status.txt"),
        git_output(&fixture.workspace, &["status", "--porcelain=v1"])?,
    )?;
    Ok(())
}

async fn assert_reopened_activation(fixture: &LiveTaskFixture) -> Result<()> {
    let expected_task: pl_studio_runtime::StudioTaskRuntime = serde_json::from_slice(
        &std::fs::read(fixture.artifact_dir.join("task-runtime.json"))?,
    )?;
    let reopened = StudioRuntime::with_options(StudioRuntimeOptions {
        studio_home: Some(fixture.studio_home.clone()),
        host: StudioHostKind::Desktop,
    })
    .await
    .map_err(anyhow::Error::new)?;
    reopened.start_runtime().await?;
    let project = reopened.open_project(&fixture.workspace).await?;
    reopened.activate_project(&project.id).await?;
    let snapshot = reopened.thread_snapshot(&fixture.thread_id).await?;
    let reopened_store =
        StudioStore::open(fixture.studio_home.join("studio/studio.sqlite")).await?;
    let restored_task = reopened
        .thread_task_view(&fixture.thread_id)
        .await?
        .context("reopened Task projection is missing")?;
    if restored_task != expected_task {
        std::fs::write(
            fixture.artifact_dir.join("task-runtime-reopened.json"),
            serde_json::to_vec_pretty(&restored_task)?,
        )?;
        bail!("reopened Task projection differs from the durable completed Task");
    }
    if snapshot.items.is_empty() || snapshot.revision == 0 {
        bail!("reopened activation did not materialize the hot Timeline window");
    }
    if snapshot.active_turn.is_some() {
        bail!("reopened completed Task retained an active Turn");
    }
    if !reopened_store
        .list_pending_interactions(&fixture.thread_id)
        .await?
        .is_empty()
    {
        bail!("reopened completed Task retained a pending interaction");
    }
    let recovery = serde_json::json!({
        "threadId": fixture.thread_id,
        "taskRunId": restored_task.run_id,
        "taskRevision": restored_task.revision,
        "timelineItems": snapshot.items.len(),
        "timelineRevision": snapshot.revision,
        "activeTurn": snapshot.active_turn,
        "status": "restoredHot",
    });
    std::fs::write(
        fixture.artifact_dir.join("shutdown-reopen.json"),
        serde_json::to_vec_pretty(&recovery)?,
    )?;
    reopened.shutdown_runtime().await?;
    Ok(())
}
