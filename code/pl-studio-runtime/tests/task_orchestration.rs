mod task_fixture;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use pl_studio_runtime::{
    InteractionResolution, InteractionStatus, PlanConfirmationResolution, StudioMode,
    StudioSubmitPromptOptions, StudioSubmitPromptRequest,
};
use task_fixture::{
    DESIGN_PATH, FEATURE_CONTENT, FEATURE_PATH, PARENT_HISTORY_MARKER, TaskFlowFixture, git_output,
    normalized_text,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offline_task_flow_completes_through_worktree_merge_review_and_continuations() -> Result<()>
{
    tokio::time::timeout(Duration::from_secs(120), run_offline_task_flow())
        .await
        .context("offline Task orchestration integration test timed out")?
}

async fn run_offline_task_flow() -> Result<()> {
    let fixture = TaskFlowFixture::new().await?;
    assert!(!fixture.workspace.join(".git").exists());

    fixture
        .runtime
        .submit_prompt(StudioSubmitPromptRequest {
            session_id: fixture.session_id.clone(),
            prompt: format!(
                "Create the offline task integration fixture and carry it through review. \
                 Unique parent marker: {PARENT_HISTORY_MARKER}"
            ),
            attachment_ids: Vec::new(),
            options: StudioSubmitPromptOptions::default(),
        })
        .await?;
    let confirmation = fixture.wait_for_plan_confirmation().await?;
    fixture.wait_for_no_active_turns().await?;
    assert_eq!(confirmation.status, InteractionStatus::Pending);

    let resolution = fixture
        .runtime
        .resolve_interaction(
            confirmation.interaction_id.clone(),
            InteractionResolution::PlanConfirmation {
                decision: PlanConfirmationResolution::ImplementFreshContext,
                content: None,
                reason: None,
            },
        )
        .await?;
    assert_eq!(resolution.interaction.status, InteractionStatus::Resolved);

    let task = fixture.wait_for_completed_task().await?;
    fixture.wait_for_no_active_turns().await?;
    fixture.assert_script_complete().await?;

    assert_eq!(task.phase, "completed");
    assert_eq!(task.work_units.len(), 1);
    let work_unit = &task.work_units[0];
    assert_eq!(work_unit.status, "merged");
    assert!(work_unit.agent_id.is_some());
    assert!(!Path::new(&work_unit.worktree_path).exists());

    let executor = task
        .agents
        .iter()
        .find(|agent| agent.role == "executor")
        .context("executor outcome is absent")?;
    assert_eq!(executor.status, "completed");
    assert!(executor.head_commit.is_some());
    let reviewer = task
        .agents
        .iter()
        .find(|agent| agent.role == "reviewer")
        .context("reviewer outcome is absent")?;
    assert_eq!(reviewer.status, "completed");

    assert_eq!(task.merges.len(), 1);
    let merge = &task.merges[0];
    assert_eq!(merge.status, "merged");
    let merge_commit = merge
        .merge_commit
        .as_deref()
        .context("accepted merge has no merge commit")?;
    assert_eq!(
        git_output(
            &fixture.workspace,
            &["show", "-s", "--format=%an <%ae>", merge_commit]
        )?,
        "Pure Studio <pure-studio@local>"
    );
    let merge_parents = git_output(
        &fixture.workspace,
        &["show", "-s", "--format=%P", merge_commit],
    )?;
    assert_eq!(merge_parents.split_whitespace().count(), 2);

    assert_eq!(task.reviews.len(), 1);
    let review = &task.reviews[0];
    assert_eq!(review.verdict, "pass");
    assert_eq!(
        review.design_references,
        vec!["design/task-flow.md#Offline Task Flow".to_string()]
    );

    assert_eq!(
        normalized_text(&fixture.workspace.join(FEATURE_PATH))?,
        FEATURE_CONTENT
    );
    let design = normalized_text(&fixture.workspace.join(DESIGN_PATH))?;
    assert!(design.contains("# Offline Task Flow"));
    assert!(design.contains("Implementation status: completed and merged."));
    assert_eq!(
        git_output(&fixture.workspace, &["rev-parse", "HEAD"])?,
        task.expected_head
    );
    assert!(git_output(&fixture.workspace, &["status", "--porcelain"])?.is_empty());

    let persisted_confirmation = fixture
        .store
        .read_interaction(&confirmation.interaction_id)
        .await?
        .context("plan confirmation was not persisted")?;
    assert_eq!(persisted_confirmation.status, InteractionStatus::Resolved);
    assert!(matches!(
        persisted_confirmation.resolution,
        Some(InteractionResolution::PlanConfirmation {
            decision: PlanConfirmationResolution::ImplementFreshContext,
            ..
        })
    ));
    assert!(
        fixture
            .store
            .list_pending_interactions(&fixture.session_id)
            .await?
            .is_empty()
    );
    assert!(fixture.runtime.runtime_snapshot().active_turns.is_empty());

    fixture
        .runtime
        .set_session_mode(&fixture.session_id, StudioMode::Simple)
        .await?;
    let session = fixture
        .store
        .read_session(&fixture.session_id)
        .await?
        .context("task session disappeared")?;
    assert_eq!(session.mode, StudioMode::Simple.label());

    fixture.shutdown().await
}
