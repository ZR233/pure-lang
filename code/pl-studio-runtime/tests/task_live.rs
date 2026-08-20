#[path = "task_fixture/git.rs"]
mod git;
#[path = "task_fixture/live.rs"]
mod live_fixture;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use git::git_output;
use live_fixture::{LIVE_VERIFY_MARKER, LiveTaskFixture, command_output, normalized_text};
use pl_studio_runtime::{
    InteractionResolution, InteractionStatus, PlanConfirmationResolution,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest,
};

const LIVE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "uses the installed Studio model configuration and incurs real model usage"]
async fn installed_config_task_mode_builds_headless_shooter() -> Result<()> {
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
    let config_unchanged = fixture.assert_config_unchanged();

    result?;
    shutdown?;
    config_unchanged
}

async fn run_live_task_flow(fixture: &LiveTaskFixture) -> Result<()> {
    fixture
        .runtime
        .submit_prompt(StudioSubmitPromptRequest {
            thread_id: fixture.thread_id.clone(),
            prompt: live_task_prompt(),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;

    let confirmation = fixture.wait_for_plan_confirmation().await?;
    fixture.wait_for_no_active_turns().await?;
    if confirmation.status != InteractionStatus::Pending {
        bail!(
            "plan confirmation was not pending: {:?}",
            confirmation.status
        );
    }
    let resolution = fixture
        .runtime
        .resolve_interaction(
            confirmation.interaction_id,
            InteractionResolution::PlanConfirmation {
                decision: PlanConfirmationResolution::ImplementFreshContext,
                content: None,
                reason: None,
            },
        )
        .await?;
    if resolution.interaction.status != InteractionStatus::Resolved {
        bail!(
            "plan confirmation did not resolve: {:?}",
            resolution.interaction.status
        );
    }

    let task = fixture.wait_for_completed_task().await?;
    fixture.wait_for_no_active_turns().await?;
    assert_task_invariants(fixture, &task).await?;
    assert_generated_project(fixture)?;
    Ok(())
}

async fn assert_task_invariants(
    fixture: &LiveTaskFixture,
    task: &pl_studio_runtime::StudioTaskRuntime,
) -> Result<()> {
    if task.phase != "completed" {
        bail!("Task phase is `{}` instead of `completed`", task.phase);
    }
    if task.work_units.is_empty() {
        bail!("Task did not create an executor work unit");
    }
    for unit in &task.work_units {
        if unit.status != "merged" {
            bail!(
                "work unit `{}` finished with status `{}`",
                unit.id,
                unit.status
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
        if executor.status != "closed" {
            bail!(
                "executor Thread `{executor_thread_id}` finished with status `{}`",
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
    if !scope_hints
        .iter()
        .any(|hints| hints.as_slice() == ["game-core.mjs"])
    {
        bail!("Task did not preserve the requested focused scopeHints: {scope_hints:?}");
    }
    if !task.completions.iter().any(|completion| {
        completion
            .changed_files
            .iter()
            .any(|path| path != "game-core.mjs" && !path.starts_with("design/"))
    }) {
        bail!("executor completion did not prove that scopeHints are non-authoritative");
    }

    let recorded_merges = fixture.successful_task_record_merge_arguments().await?;
    if recorded_merges.len() != task.merges.len() {
        bail!(
            "task_record_merge call count {} does not match durable merge count {}",
            recorded_merges.len(),
            task.merges.len()
        );
    }
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
            &[
                "merge-base",
                "--is-ancestor",
                resulting_head,
                &task.expected_head,
            ],
        )?;
    }

    if task.completions.iter().any(|completion| {
        completion.kind == "delivery"
            && completion.status == "approved"
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
        } => {
            if task.work_units.len() != 1
                || task
                    .reviews
                    .iter()
                    .any(|review| review.scope == "integrated")
            {
                bail!("single-executor equivalent task unexpectedly created integrated review");
            }
        }
        pl_studio_runtime::StudioIntegratedReviewGate::SatisfiedByReview {
            review_round_id,
            reviewed_head,
        } => {
            let review = task
                .reviews
                .iter()
                .find(|review| review.id == *review_round_id)
                .context("satisfied review gate references a missing round")?;
            if review.scope != "integrated"
                || review.verdict != "pass"
                || review.reviewed_head != *reviewed_head
                || reviewed_head != &task.expected_head
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
            .any(|reference| reference.path == "design/shooter.md")
    }) {
        bail!("not every review cited design/shooter.md");
    }
    if !task.completions.iter().all(|completion| {
        completion.status != "approved"
            || task.reviews.iter().any(|review| {
                review.scope == "delivery"
                    && review.completion_id.as_deref() == Some(completion.id.as_str())
                    && review.completion_revision == Some(completion.revision)
                    && review.reviewed_head == completion.head_commit.as_deref().unwrap_or_default()
                    && review.verdict == "pass"
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
    if git_output(&fixture.workspace, &["rev-parse", "HEAD"])? != task.expected_head {
        bail!("workspace HEAD does not match Task expectedHead");
    }
    if !git_output(&fixture.workspace, &["status", "--porcelain"])?.is_empty() {
        bail!("workspace Git tree is dirty");
    }
    git_output(
        &fixture.workspace,
        &["cat-file", "-e", "HEAD:design/shooter.md"],
    )
    .context("design/shooter.md was not committed at workspace HEAD")?;
    Ok(())
}

fn assert_generated_project(fixture: &LiveTaskFixture) -> Result<()> {
    for path in [
        "index.html",
        "styles.css",
        "game-core.mjs",
        "game.js",
        "verify.mjs",
        "design/shooter.md",
    ] {
        if !fixture.workspace.join(path).is_file() {
            bail!("required generated file is missing: {path}");
        }
    }

    let html = normalized_text(&fixture.workspace.join("index.html"))?;
    let html_lower = html.to_ascii_lowercase();
    if !html_lower.contains("<canvas") {
        bail!("index.html does not contain a canvas");
    }
    if !html.contains("styles.css") {
        bail!("index.html does not reference styles.css");
    }
    if !html.contains("game.js") {
        bail!("index.html does not reference game.js");
    }

    run_node(fixture, &["--check", "game-core.mjs"])?;
    run_node(fixture, &["--check", "game.js"])?;
    let verification = run_node(fixture, &["verify.mjs"])?;
    if !verification
        .lines()
        .any(|line| line.trim() == LIVE_VERIFY_MARKER)
    {
        bail!(
            "verify.mjs did not output the fixed success marker `{LIVE_VERIFY_MARKER}`\n\
             output:\n{verification}"
        );
    }
    Ok(())
}

fn run_node(fixture: &LiveTaskFixture, args: &[&str]) -> Result<String> {
    command_output(Some(&fixture.workspace), "node", args)
}

fn live_task_prompt() -> String {
    format!(
        r#"Build and fully deliver a dependency-free static Web airplane shooter in this temporary Git workspace.

Required files:
- index.html
- styles.css
- game-core.mjs
- game.js
- verify.mjs
- design/shooter.md

Required behavior:
- keyboard movement constrained to the canvas bounds
- shooting
- enemy spawning and movement
- projectile/enemy collision
- score updates
- game-over state
- restart that resets gameplay state

Use only browser APIs and Node.js built-ins; do not add package dependencies or require a browser build step.
Keep deterministic gameplay rules in game-core.mjs so verify.mjs can import them in Node.
verify.mjs must use node:assert to verify movement boundaries, shooting, collision, scoring, and restart, then print exactly this success marker on its own line:
{LIVE_VERIFY_MARKER}

In Task mode, update and commit design/shooter.md, then spawn one executor with a self-contained structured implementation blueprint. The blueprint must contain at least two concrete ordered implementation steps, repository targets, stable acceptance criteria, and command or inspection checks that cover every criterion. Use scopeHints exactly ["game-core.mjs"]. scopeHints are planning and review-focus hints only, not write authorization: that executor must deliver all required non-design files, including files outside the hint. Review every completion, close each approved executor, integrate it with ordinary Git in the Planner workspace, and call task_record_merge with the exact Completion revision and before/after HEADs. Synchronize the final design, read the shared integrated review gate, and do not create an integrated reviewer when it reports the single-executor equivalent exemption. If it reports required, request integrated review and repair failures through the normal Task workflow. Only call task_complete when the gate permits it. The final Git worktree must be clean."#
    )
}
