//! apply 前后的 durable 事实复核。

use anyhow::{Context, Result, bail};

use crate::studio::{
    StudioTaskRecoveryPreview, StudioTaskRecoveryTarget, StudioTaskRecoveryTargetKind,
};

use super::StudioRuntime;
use super::facts::{record_fingerprint, recovery_state_from_task_kind};

impl StudioRuntime {
    pub(super) async fn validate_task_recovery_facts(
        &self,
        preview: &StudioTaskRecoveryPreview,
        target: &StudioTaskRecoveryTarget,
        _allow_generation_change: bool,
    ) -> Result<()> {
        let aggregate = self
            .task_runtime
            .activate(&preview.root_thread_id)
            .await?
            .context("Task recovery owner is not available")?;
        let run = &aggregate.facts.run;
        if run.id != preview.run_id
            || run.revision != preview.revision
            || run.generation() != preview.task_generation
            || recovery_state_from_task_kind(run.kind()) != preview.state
        {
            bail!("Task recovery facts changed after conversation recovery");
        }
        match target.kind {
            StudioTaskRecoveryTargetKind::Planner => {
                if target.work_unit_id.is_some()
                    || target.attempt.is_some()
                    || target.continuation_revision.is_some()
                    || target.thread_id != preview.root_thread_id
                {
                    bail!("Planner recovery target identity changed");
                }
            }
            StudioTaskRecoveryTargetKind::Executor => {
                let work_unit_id = target
                    .work_unit_id
                    .as_deref()
                    .context("Executor recovery target has no WorkUnit")?;
                let unit = aggregate
                    .facts
                    .work_units
                    .iter()
                    .find(|unit| unit.id == work_unit_id)
                    .context("Task recovery WorkUnit disappeared")?;
                if unit.executor_thread_id.as_deref() != Some(target.thread_id.as_str())
                    || Some(unit.attempt) != target.attempt
                    || Some(unit.continuation_revision()) != target.continuation_revision
                    || unit.branch != target.branch
                    || unit.worktree_path != target.worktree_path
                    || target.base_commit.as_deref() != Some(unit.base_commit.as_str())
                {
                    bail!("Task recovery WorkUnit identity or continuation changed");
                }
            }
        }
        if record_fingerprint(&aggregate.facts.completions)?
            != preview.completion_revision_fingerprint
            || record_fingerprint(&aggregate.facts.reviews)? != preview.review_revision_fingerprint
            || record_fingerprint(&aggregate.facts.merges)? != preview.merge_revision_fingerprint
        {
            bail!("Task Completion/Review/Merge facts changed");
        }
        Ok(())
    }
}

pub(super) fn validate_turn_suffix(
    target: &StudioTaskRecoveryTarget,
    selected: &[String],
) -> Result<()> {
    if selected.is_empty() || selected.len() > 8 {
        bail!("Task recovery must select a suffix of 1 to 8 complete Turns");
    }
    let start = target.turns.len().saturating_sub(selected.len());
    let suffix = target.turns[start..]
        .iter()
        .map(|turn| turn.turn_id.as_str())
        .collect::<Vec<_>>();
    if suffix != selected.iter().map(String::as_str).collect::<Vec<_>>() {
        bail!("Task recovery Turn selection must be a continuous suffix");
    }
    Ok(())
}
