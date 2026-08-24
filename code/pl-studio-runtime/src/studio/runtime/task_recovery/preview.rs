//! 恢复候选收集与无状态 preview 快照生成。

use std::cmp::Reverse;
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use pl_core::{ConversationRecoveryTarget, ThreadId};
use pl_protocol::{ConversationRecoveryMode, ThreadItemState, TurnState};

use crate::studio::agent_host::root_agent_id;
use crate::studio::task_coordinator::{TaskRun, TaskRunStateKind, WorkUnit, WorkUnitStateKind};
use crate::studio::{
    StudioTaskRecoveryPreview, StudioTaskRecoveryTarget, StudioTaskRecoveryTargetKind,
    StudioTaskRecoveryTurn,
};

use super::StudioRuntime;
use super::facts::{
    flatten_input_hashes, preview_token, record_fingerprint, recovery_state_from_task_kind,
    tool_summary,
};

struct Candidate {
    target: StudioTaskRecoveryTarget,
    priority: u8,
    updated_at: i64,
}

impl StudioRuntime {
    pub(super) async fn build_task_recovery_preview(
        &self,
        root_thread_id: &str,
    ) -> Result<StudioTaskRecoveryPreview> {
        // 预览基于内存 owner 事实；未驻留聚合先显式冷激活（design/20 §20.3）。
        let aggregate = self
            .task_runtime
            .activate(root_thread_id)
            .await?
            .with_context(|| {
                format!("active task run not found for root Thread {root_thread_id}")
            })?;
        let run = aggregate.facts.run.clone();
        anyhow::ensure!(
            !run.kind().is_terminal(),
            "active task run not found for this root Thread"
        );
        ensure_recoverable_phase(run.kind())?;
        let runtime = self.agent_framework().await?.handle();
        ensure_task_tree_idle(&runtime, root_thread_id).await?;

        let work_units = aggregate.facts.work_units;
        let completions = aggregate.facts.completions;
        let reviews = aggregate.facts.reviews;
        let merges = aggregate.facts.merges;
        let mut candidates = Vec::new();
        for unit in work_units.iter().filter(|unit| eligible_executor(unit)) {
            let Some(thread_id) = unit.executor_thread_id.as_deref() else {
                continue;
            };
            if let Some(candidate) = self
                .recovery_candidate(
                    &runtime,
                    &run,
                    thread_id,
                    StudioTaskRecoveryTargetKind::Executor,
                    Some(unit),
                )
                .await?
            {
                candidates.push(candidate);
            }
        }
        if let Some(candidate) = self
            .recovery_candidate(
                &runtime,
                &run,
                root_thread_id,
                StudioTaskRecoveryTargetKind::Planner,
                None,
            )
            .await?
        {
            candidates.push(candidate);
        }
        candidates.sort_by(|left, right| {
            (
                left.priority,
                Reverse(left.updated_at),
                &left.target.thread_id,
            )
                .cmp(&(
                    right.priority,
                    Reverse(right.updated_at),
                    &right.target.thread_id,
                ))
        });
        let recommended_thread_id = candidates
            .first()
            .map(|candidate| candidate.target.thread_id.clone())
            .context("Task has no eligible conversation recovery target")?;
        let targets = candidates
            .into_iter()
            .map(|candidate| candidate.target)
            .collect::<Vec<_>>();
        let mut preview = StudioTaskRecoveryPreview {
            preview_token: String::new(),
            root_thread_id: root_thread_id.to_string(),
            run_id: run.id.clone(),
            revision: run.revision,
            task_generation: run.generation(),
            state: recovery_state_from_task_kind(run.kind()),
            recommended_thread_id,
            targets,
            completion_revision_fingerprint: record_fingerprint(&completions)?,
            review_revision_fingerprint: record_fingerprint(&reviews)?,
            merge_revision_fingerprint: record_fingerprint(&merges)?,
        };
        preview.preview_token = preview_token(&preview)?;
        Ok(preview)
    }

    async fn recovery_candidate(
        &self,
        runtime: &pl_core::AgentRuntimeHandle,
        run: &TaskRun,
        thread_id: &str,
        kind: StudioTaskRecoveryTargetKind,
        unit: Option<&WorkUnit>,
    ) -> Result<Option<Candidate>> {
        let page = self.store.list_thread_turns(thread_id, None, 8).await?;
        let mut histories = page
            .turns
            .into_iter()
            .filter(|history| terminal_turn(&history.turn.state))
            .collect::<Vec<_>>();
        histories.reverse();
        if histories.is_empty() {
            return Ok(None);
        }
        let all_turn_ids = histories
            .iter()
            .map(|history| history.turn.id.clone())
            .collect::<Vec<_>>();
        let inputs = self
            .store
            .conversation_turn_inputs(thread_id, &all_turn_ids)
            .await?;
        let turns = histories
            .iter()
            .map(|history| {
                let tool_summaries = history
                    .items
                    .iter()
                    .filter_map(|item| match item.state() {
                        ThreadItemState::Tool(tool) => Some(tool_summary(tool)),
                        ThreadItemState::Text(_)
                        | ThreadItemState::Thinking(_)
                        | ThreadItemState::Agent(_)
                        | ThreadItemState::Turn(_)
                        | ThreadItemState::Inference(_)
                        | ThreadItemState::Plan(_)
                        | ThreadItemState::File(_)
                        | ThreadItemState::ContextCompaction(_) => None,
                    })
                    .collect::<Vec<_>>();
                Ok(StudioTaskRecoveryTurn {
                    turn_id: history.turn.id.clone(),
                    state: turn_state(&history.turn.state)?,
                    updated_at: history.turn.updated_at,
                    item_count: u64::try_from(history.items.len())?,
                    input_count: u64::try_from(
                        inputs
                            .get(&history.turn.id)
                            .map_or(0, |inputs| inputs.hashes.len()),
                    )?,
                    tool_count: u64::try_from(tool_summaries.len())?,
                    tool_summaries,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let failure_index = histories
            .iter()
            .rposition(|history| failed_turn(&history.turn.state));
        let default_start = failure_index.unwrap_or(histories.len().saturating_sub(1));
        let default_turn_ids = histories[default_start..]
            .iter()
            .map(|history| history.turn.id.clone())
            .collect::<Vec<_>>();
        let default_input_hashes = flatten_input_hashes(&inputs, &default_turn_ids, false)?;
        let agent_id = ThreadId::new(thread_id.to_string())?;
        let rewind_preview = if default_input_hashes.is_empty() {
            None
        } else {
            runtime
                .preview_conversation_recovery(
                    agent_id.clone(),
                    ConversationRecoveryTarget {
                        mode: ConversationRecoveryMode::RewindTail,
                        turn_ids: default_turn_ids.clone(),
                        input_hashes: default_input_hashes.clone(),
                    },
                )
                .await
                .ok()
        };
        let rebuild_preview = runtime
            .preview_conversation_recovery(
                agent_id,
                ConversationRecoveryTarget {
                    mode: ConversationRecoveryMode::RebuildThread,
                    turn_ids: default_turn_ids.clone(),
                    input_hashes: default_input_hashes,
                },
            )
            .await
            .ok();
        let mut available_modes = Vec::new();
        if rewind_preview.is_some() {
            available_modes.push(ConversationRecoveryMode::RewindTail);
        }
        if rebuild_preview.is_some() {
            available_modes.push(ConversationRecoveryMode::RebuildThread);
        }
        let Some(revision_preview) = rewind_preview.as_ref().or(rebuild_preview.as_ref()) else {
            return Ok(None);
        };
        let (work_unit_id, attempt, continuation_revision, branch, worktree_path, base_commit) =
            match unit {
                Some(unit) => (
                    Some(unit.id.clone()),
                    Some(unit.attempt),
                    Some(unit.continuation_revision()),
                    unit.branch.clone(),
                    unit.worktree_path.clone(),
                    Some(unit.base_commit.clone()),
                ),
                None => (
                    None,
                    None,
                    None,
                    String::new(),
                    run.workspace_root.clone(),
                    None,
                ),
            };
        let has_failure = failure_index.is_some();
        let priority = match (kind, has_failure) {
            (StudioTaskRecoveryTargetKind::Executor, true) => 0,
            (StudioTaskRecoveryTargetKind::Planner, true) => 1,
            (
                StudioTaskRecoveryTargetKind::Executor | StudioTaskRecoveryTargetKind::Planner,
                false,
            ) => 2,
        };
        Ok(Some(Candidate {
            updated_at: histories
                .last()
                .map_or(0, |history| history.turn.updated_at),
            priority,
            target: StudioTaskRecoveryTarget {
                thread_id: thread_id.to_string(),
                kind,
                work_unit_id,
                attempt,
                continuation_revision,
                expected_runtime_revision: revision_preview.expected_runtime_revision,
                expected_thread_revision: revision_preview.expected_thread_revision,
                branch,
                worktree_path,
                base_commit,
                turns,
                default_turn_ids,
                available_modes,
            },
        }))
    }
}

pub(super) async fn ensure_task_tree_idle(
    runtime: &pl_core::AgentRuntimeHandle,
    root_thread_id: &str,
) -> Result<()> {
    let snapshots = runtime.list().await.map_err(anyhow::Error::msg)?;
    let parents = snapshots
        .iter()
        .map(|snapshot| {
            (
                snapshot.identity.id.clone(),
                snapshot.identity.parent_id.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let root = root_agent_id(root_thread_id);
    for snapshot in snapshots {
        if !belongs_to_root(&parents, &snapshot.identity.id, &root) {
            continue;
        }
        if snapshot.state.is_operational()
            && (!snapshot.state.is_idle()
                || snapshot.active_turn_id().is_some()
                || snapshot.pending_inputs != 0)
        {
            bail!("Task recovery requires the complete Task agent tree to be paused");
        }
    }
    Ok(())
}

pub(super) fn belongs_to_root(
    parents: &BTreeMap<ThreadId, Option<ThreadId>>,
    agent: &ThreadId,
    root: &ThreadId,
) -> bool {
    let mut current = Some(agent.clone());
    while let Some(agent) = current {
        if &agent == root {
            return true;
        }
        current = parents.get(&agent).cloned().flatten();
    }
    false
}
pub(super) fn ensure_recoverable_phase(phase: TaskRunStateKind) -> Result<()> {
    if !phase.is_terminal() {
        Ok(())
    } else {
        bail!(
            "Task phase {} must use Retry/Reconcile instead of conversation recovery",
            phase.as_str()
        )
    }
}
fn eligible_executor(unit: &WorkUnit) -> bool {
    unit.executor_thread_id.is_some()
        && matches!(
            unit.kind(),
            WorkUnitStateKind::Running
                | WorkUnitStateKind::WaitingReview
                | WorkUnitStateKind::ChangesRequired
                | WorkUnitStateKind::Paused
        )
}
fn terminal_turn(state: &TurnState) -> bool {
    matches!(
        state,
        TurnState::Completed(_)
            | TurnState::Cancelled(_)
            | TurnState::Failed(_)
            | TurnState::BudgetLimited(_)
    )
}
fn failed_turn(state: &TurnState) -> bool {
    matches!(state, TurnState::Failed(_) | TurnState::Cancelled(_))
}
fn turn_state(state: &TurnState) -> Result<pl_protocol::studio::StudioTaskRecoveryTurnState> {
    match state {
        TurnState::Completed(_) => Ok(pl_protocol::studio::StudioTaskRecoveryTurnState::Completed),
        TurnState::Cancelled(_) => Ok(pl_protocol::studio::StudioTaskRecoveryTurnState::Cancelled),
        TurnState::Failed(_) => Ok(pl_protocol::studio::StudioTaskRecoveryTurnState::Failed),
        TurnState::BudgetLimited(_) => {
            Ok(pl_protocol::studio::StudioTaskRecoveryTurnState::BudgetLimited)
        }
        TurnState::Queued(_) | TurnState::Running(_) => Err(anyhow::anyhow!(
            "task recovery preview received a non-terminal Turn"
        )),
    }
}
