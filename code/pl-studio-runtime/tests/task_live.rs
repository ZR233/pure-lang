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
            session_id: fixture.session_id.clone(),
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

    let interrupted_executor_id = fixture.wait_for_running_executor().await?;
    fixture
        .runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: fixture.session_id.clone(),
            prompt: live_interrupt_prompt(&interrupted_executor_id),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    let interrupt_target = fixture.wait_for_successful_interrupt_target().await?;
    if interrupt_target != interrupted_executor_id {
        bail!("send_input did not interrupt the expected executor `{interrupted_executor_id}`");
    }

    let task = fixture.wait_for_completed_task().await?;
    fixture.wait_for_no_active_turns().await?;
    assert_task_invariants(fixture, &task, &interrupted_executor_id).await?;
    assert_generated_project(fixture)?;
    Ok(())
}

async fn assert_task_invariants(
    fixture: &LiveTaskFixture,
    task: &pl_studio_runtime::StudioTaskRuntime,
    interrupted_executor_id: &str,
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
        if Path::new(&unit.worktree_path).exists() {
            bail!(
                "merged work unit `{}` retained worktree `{}`",
                unit.id,
                unit.worktree_path
            );
        }
    }

    let owned_paths = fixture.successful_executor_owned_paths().await?;
    if owned_paths.is_empty() {
        bail!("no successful task_spawn_executor call was recorded");
    }
    for paths in &owned_paths {
        if paths.is_empty() || paths.iter().any(|path| path.trim().is_empty()) {
            bail!("task_spawn_executor recorded empty ownedPaths: {paths:?}");
        }
    }

    let completed_executors = task
        .agents
        .iter()
        .filter(|agent| agent.role == "executor" && agent.status == "completed")
        .collect::<Vec<_>>();
    if completed_executors.is_empty() {
        bail!("Task has no completed executor");
    }
    for executor in completed_executors {
        if !task
            .merges
            .iter()
            .any(|merge| merge.agent_id == executor.agent_id && merge.status == "merged")
        {
            bail!(
                "completed executor `{}` has no merged delivery",
                executor.agent_id
            );
        }
    }
    if !task.agents.iter().any(|agent| {
        agent.agent_id == interrupted_executor_id
            && agent.role == "executor"
            && agent.status == "completed"
    }) {
        bail!("interrupted executor `{interrupted_executor_id}` did not complete its delivery");
    }
    if !task
        .merges
        .iter()
        .any(|merge| merge.agent_id == interrupted_executor_id && merge.status == "merged")
    {
        bail!("interrupted executor `{interrupted_executor_id}` did not produce a merged delivery");
    }
    if task.merges.iter().any(|merge| merge.status != "merged") {
        bail!("Task contains an unmerged delivery: {:#?}", task.merges);
    }

    let review = task.reviews.last().context("Task has no reviewer result")?;
    if review.verdict != "pass" {
        bail!("latest reviewer verdict is `{}`", review.verdict);
    }
    if review.head_commit != task.expected_head {
        bail!(
            "latest reviewer checked `{}` instead of expected HEAD `{}`",
            review.head_commit,
            task.expected_head
        );
    }
    if !review
        .design_references
        .iter()
        .any(|reference| reference.starts_with("design/shooter.md#"))
    {
        bail!(
            "latest reviewer did not cite design/shooter.md: {:?}",
            review.design_references
        );
    }

    if !fixture
        .store
        .list_pending_interactions(&fixture.session_id)
        .await?
        .is_empty()
    {
        bail!("Task retained pending interactions");
    }
    if !fixture.runtime.runtime_snapshot().active_turns.is_empty() {
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

fn live_interrupt_prompt(executor_id: &str) -> String {
    format!(
        "这是 headless shooter 的中断续轮验收控制输入。只调用一次 send_input 工具：\
         target 必须是 `{executor_id}`，delivery 必须是 `interruptThenStart`，message 要求该 \
         executor 在 queued turn 中继续现有实现、完成实际验证、commit，并以 submit_delivery \
         交付。不要创建新 executor，不要 merge，不要用文字代替工具调用。工具成功后只返回简短确认。"
    )
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

In Task mode, update and commit design/shooter.md, spawn at least one executor with explicit non-empty ownedPaths, merge every successful delivery, request review against the current design and HEAD, repair any review failures through the normal Task workflow, and only call task_complete after the reviewer passes. The final Git worktree must be clean."#
    )
}
