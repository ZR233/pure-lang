use std::cmp::Reverse;
use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use pl_core::{
    AgentActivityState, AgentId, AgentLifecycleState, AgentSubmitRequest, AgentTurnSubmitPolicy,
    ConversationRecoveryRequest, ConversationRecoveryResult, ConversationRecoveryTarget,
    MailboxPresentation,
};
use pl_protocol::{ConversationRecoveryMode, ThreadItemContent, ThreadToolCall, TurnState};

use crate::studio::agent_host::root_agent_id;
use crate::studio::task_coordinator::git::fingerprint_repository;
use crate::studio::task_coordinator::{
    TaskRunPhase, TaskRunRecord, ThreadExecutionStatus, WorkUnitRecord, WorkUnitStatus,
};
use crate::studio::{
    StudioTaskRecoveryPreview, StudioTaskRecoveryRequest, StudioTaskRecoveryResult,
    StudioTaskRecoveryTarget, StudioTaskRecoveryTargetKind, StudioTaskRecoveryTurn,
};

use super::StudioRuntime;

struct Candidate {
    target: StudioTaskRecoveryTarget,
    priority: u8,
    updated_at: i64,
}

impl StudioRuntime {
    pub async fn preview_task_recovery(
        &self,
        root_thread_id: &str,
    ) -> Result<StudioTaskRecoveryPreview> {
        self.build_task_recovery_preview(root_thread_id).await
    }

    pub async fn apply_task_recovery(
        &self,
        request: StudioTaskRecoveryRequest,
    ) -> Result<StudioTaskRecoveryResult> {
        if request.recovery_id.trim().is_empty() {
            bail!("Task recovery id must not be empty");
        }
        if request.root_thread_id != request.preview.root_thread_id {
            bail!("Task recovery root Thread does not match its preview");
        }
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let branch_guard = self.task_coordinator.lock_branch_mutation().await;
        self.task_coordinator
            .ensure_branch_mutation_guard(&branch_guard)?;

        let target = request
            .preview
            .targets
            .iter()
            .find(|target| target.thread_id == request.target_thread_id)
            .cloned()
            .context("Task recovery target is not present in the preview")?;
        validate_turn_suffix(&target, &request.turn_ids)?;
        if !target.available_modes.contains(&request.mode) {
            bail!("Task recovery mode is not available for the selected target");
        }

        let existing_recovery = self
            .store
            .conversation_recovery_state(&request.target_thread_id)
            .await?
            .last_recovery
            .filter(|record| record.recovery_id == request.recovery_id);
        if let Some(record) = existing_recovery.as_ref() {
            if record.mode != request.mode || record.target_turn_ids != request.turn_ids {
                bail!("Task recovery id was already used for a different target");
            }
            self.validate_task_recovery_facts(&request.preview, &target, true)
                .await?;
        } else {
            let current = self
                .build_task_recovery_preview(&request.root_thread_id)
                .await?;
            if current.preview_token != request.preview.preview_token {
                bail!("Task recovery preview is stale; generate a new preview");
            }
        }

        let runtime = self.agent_framework().await?.handle();
        let input_hashes = selected_input_hashes(
            &self.store,
            &request.target_thread_id,
            &request.turn_ids,
            request.mode,
        )
        .await?;
        let target_agent_id = AgentId::new(request.target_thread_id.clone())?;
        let recovery = if let Some(record) = existing_recovery {
            ConversationRecoveryResult {
                recovery_id: record.recovery_id,
                mode: record.mode,
                facts: pl_core::ConversationRecoveryFacts {
                    recovery_revision: record.revision,
                    before_transcript_hash: record.before_transcript_hash,
                    after_transcript_hash: record.after_transcript_hash,
                },
                runtime_revision: record.runtime_revision,
                thread_revision: record.thread_revision,
                removed_item_count: record.removed_item_count,
                removed_input_count: record.removed_input_count,
            }
        } else {
            let preview = runtime
                .preview_conversation_recovery(
                    target_agent_id.clone(),
                    ConversationRecoveryTarget {
                        mode: request.mode,
                        turn_ids: request.turn_ids.clone(),
                        input_hashes,
                    },
                )
                .await
                .map_err(anyhow::Error::msg)?;
            runtime
                .recover_conversation(
                    target_agent_id,
                    ConversationRecoveryRequest {
                        recovery_id: request.recovery_id.clone(),
                        preview,
                    },
                )
                .await
                .map_err(anyhow::Error::msg)?
        };

        let main_after = fingerprint_repository(
            &request.preview.main_git_fingerprint.workspace_root,
            &request.preview.main_git_fingerprint.base_commit,
            &request.preview.main_git_fingerprint.expected_head,
        )
        .await?;
        let target_after = fingerprint_repository(
            &target.git_fingerprint.workspace_root,
            &target.git_fingerprint.base_commit,
            &target.git_fingerprint.expected_head,
        )
        .await?;
        if main_after != request.preview.main_git_fingerprint
            || target_after != target.git_fingerprint
        {
            bail!("Git/worktree fingerprint changed during conversation recovery");
        }

        let phase = TaskRunPhase::from_str(&request.preview.phase)
            .context("Task recovery preview has an invalid phase")?;
        let stop_cleared = if request.preview.stop_requested {
            self.store
                .clear_task_stop_for_recovery(
                    &request.preview.run_id,
                    request.preview.task_generation,
                    phase,
                    &request.preview.expected_head,
                )
                .await?
        } else {
            false
        };
        let resume_mail_id = format!(
            "task-recovery:{}:{}",
            request.preview.run_id, recovery.facts.recovery_revision
        );
        let resume_turn_id = runtime
            .submit(
                root_agent_id(&request.root_thread_id),
                AgentSubmitRequest::start(
                    root_agent_id(&request.root_thread_id),
                    "Conversation recovery 已提交。先读取 task_status 与 list_agents，核对当前 Task、WorkUnit 和工作区事实；如目标是 executor，只向同一 executor Thread follow-up，禁止创建重复 WorkUnit。",
                )
                .with_presentation(MailboxPresentation::Hidden)
                .with_mail_id(resume_mail_id)
                .with_turn_policy(AgentTurnSubmitPolicy::StartOrQueue),
            )
            .await
            .map_err(anyhow::Error::msg)?;

        Ok(StudioTaskRecoveryResult {
            recovery_id: recovery.recovery_id,
            run_id: request.preview.run_id,
            work_unit_id: target.work_unit_id,
            root_thread_id: request.root_thread_id,
            target_thread_id: request.target_thread_id,
            mode: recovery.mode,
            recovery_revision: recovery.facts.recovery_revision,
            runtime_revision: recovery.runtime_revision,
            thread_revision: recovery.thread_revision,
            before_transcript_hash: recovery.facts.before_transcript_hash,
            after_transcript_hash: recovery.facts.after_transcript_hash,
            removed_item_count: recovery.removed_item_count,
            removed_input_count: recovery.removed_input_count,
            stop_cleared,
            resume_turn_id: resume_turn_id.to_string(),
            git_fingerprint: target_after,
        })
    }

    async fn build_task_recovery_preview(
        &self,
        root_thread_id: &str,
    ) -> Result<StudioTaskRecoveryPreview> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(root_thread_id)
            .await?;
        ensure_recoverable_phase(run.phase)?;
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("Task recovery requires the durable branch lease")?;
        if lease.branch != run.branch
            || lease.git_common_dir != run.git_common_dir
            || lease.expected_head != run.expected_head
        {
            bail!("Task recovery branch lease does not match the TaskRun");
        }
        let runtime = self.agent_framework().await?.handle();
        ensure_task_tree_idle(&runtime, root_thread_id).await?;

        let work_units = self.store.list_work_units(&run.id).await?;
        let completions = self.store.list_work_completions(&run.id).await?;
        let reviews = self.store.list_review_rounds(&run.id).await?;
        let merges = self.store.list_merge_records(&run.id).await?;
        let main_git_fingerprint =
            fingerprint_repository(&run.workspace_root, &run.base_commit, &run.expected_head)
                .await?;
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
            run_id: run.id,
            task_generation: run.task_generation,
            phase: run.phase.as_str().to_string(),
            expected_head: run.expected_head,
            stop_requested: run.stop_requested,
            branch_lease_id: lease.id,
            branch_lease_branch: lease.branch,
            branch_lease_git_common_dir: lease.git_common_dir,
            branch_lease_expected_head: lease.expected_head,
            recommended_thread_id,
            targets,
            main_git_fingerprint,
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
        run: &TaskRunRecord,
        thread_id: &str,
        kind: StudioTaskRecoveryTargetKind,
        unit: Option<&WorkUnitRecord>,
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
                    .filter_map(|item| match &item.content {
                        ThreadItemContent::ToolCall { tool } => Some(tool_summary(tool)),
                        ThreadItemContent::UserMessage { .. }
                        | ThreadItemContent::AgentMessage { .. }
                        | ThreadItemContent::Reasoning { .. }
                        | ThreadItemContent::Plan { .. }
                        | ThreadItemContent::File { .. }
                        | ThreadItemContent::ContextCompaction { .. } => None,
                    })
                    .collect::<Vec<_>>();
                Ok(StudioTaskRecoveryTurn {
                    turn_id: history.turn.id.clone(),
                    status: turn_status(&history.turn.state).to_string(),
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
        let agent_id = AgentId::new(thread_id.to_string())?;
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
                    Some(unit.continuation_revision),
                    unit.branch.clone(),
                    unit.worktree_path.clone(),
                    unit.base_commit.clone(),
                ),
                None => (
                    None,
                    None,
                    None,
                    run.branch.clone(),
                    run.workspace_root.clone(),
                    run.base_commit.clone(),
                ),
            };
        let git_fingerprint =
            fingerprint_repository(&worktree_path, &base_commit, &run.expected_head).await?;
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
                turns,
                default_turn_ids,
                available_modes,
                git_fingerprint,
            },
        }))
    }

    async fn validate_task_recovery_facts(
        &self,
        preview: &StudioTaskRecoveryPreview,
        target: &StudioTaskRecoveryTarget,
        allow_cleared_stop: bool,
    ) -> Result<()> {
        let run = self
            .store
            .read_active_task_run_for_root_thread(&preview.root_thread_id)
            .await?;
        if run.id != preview.run_id
            || run.task_generation != preview.task_generation
            || run.phase.as_str() != preview.phase
            || run.expected_head != preview.expected_head
            || (!allow_cleared_stop && run.stop_requested != preview.stop_requested)
            || (allow_cleared_stop && run.stop_requested && !preview.stop_requested)
        {
            bail!("Task recovery facts changed after conversation recovery");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("Task recovery branch lease disappeared")?;
        if lease.id != preview.branch_lease_id
            || lease.branch != preview.branch_lease_branch
            || lease.git_common_dir != preview.branch_lease_git_common_dir
            || lease.expected_head != preview.branch_lease_expected_head
        {
            bail!("Task recovery branch lease changed");
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
                let unit = self
                    .store
                    .list_work_units(&run.id)
                    .await?
                    .into_iter()
                    .find(|unit| unit.id == work_unit_id)
                    .context("Task recovery WorkUnit disappeared")?;
                if unit.executor_thread_id.as_deref() != Some(target.thread_id.as_str())
                    || Some(unit.attempt) != target.attempt
                    || Some(unit.continuation_revision) != target.continuation_revision
                    || unit.branch != target.branch
                    || unit.worktree_path != target.worktree_path
                    || unit.base_commit != target.git_fingerprint.base_commit
                {
                    bail!("Task recovery WorkUnit identity or continuation changed");
                }
            }
        }
        let main = fingerprint_repository(
            &preview.main_git_fingerprint.workspace_root,
            &preview.main_git_fingerprint.base_commit,
            &preview.main_git_fingerprint.expected_head,
        )
        .await?;
        let target_git = fingerprint_repository(
            &target.git_fingerprint.workspace_root,
            &target.git_fingerprint.base_commit,
            &target.git_fingerprint.expected_head,
        )
        .await?;
        if main != preview.main_git_fingerprint || target_git != target.git_fingerprint {
            bail!("Task recovery Git fingerprint changed");
        }
        let completions = self.store.list_work_completions(&run.id).await?;
        let reviews = self.store.list_review_rounds(&run.id).await?;
        let merges = self.store.list_merge_records(&run.id).await?;
        if record_fingerprint(&completions)? != preview.completion_revision_fingerprint
            || record_fingerprint(&reviews)? != preview.review_revision_fingerprint
            || record_fingerprint(&merges)? != preview.merge_revision_fingerprint
        {
            bail!("Task Completion/Review/Merge facts changed");
        }
        Ok(())
    }
}

fn tool_summary(tool: &ThreadToolCall) -> String {
    let outcome = if tool.timed_out {
        Some("timed out".to_string())
    } else if tool.denial_reason.is_some() {
        Some("denied".to_string())
    } else {
        tool.exit_code.map(|code| format!("exit {code}"))
    };
    match outcome {
        Some(outcome) => format!("{} ({outcome})", tool.name),
        None => tool.name.clone(),
    }
}

async fn ensure_task_tree_idle(
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
        if snapshot.lifecycle == AgentLifecycleState::Active
            && (snapshot.activity != AgentActivityState::Idle
                || snapshot.active_turn_id.is_some()
                || snapshot.pending_inputs != 0)
        {
            bail!("Task recovery requires the complete Task agent tree to be paused");
        }
    }
    Ok(())
}

fn belongs_to_root(
    parents: &BTreeMap<AgentId, Option<AgentId>>,
    agent: &AgentId,
    root: &AgentId,
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

fn ensure_recoverable_phase(phase: TaskRunPhase) -> Result<()> {
    if matches!(
        phase,
        TaskRunPhase::Planning
            | TaskRunPhase::PendingConfirmation
            | TaskRunPhase::DesignUpdating
            | TaskRunPhase::Implementing
            | TaskRunPhase::Reworking
    ) {
        Ok(())
    } else {
        bail!(
            "Task phase {} must use Retry/Reconcile instead of conversation recovery",
            phase.as_str()
        )
    }
}

fn eligible_executor(unit: &WorkUnitRecord) -> bool {
    unit.executor_thread_id.is_some()
        && matches!(
            unit.status,
            WorkUnitStatus::Running
                | WorkUnitStatus::AwaitingCompletion
                | WorkUnitStatus::ChangesRequested
                | WorkUnitStatus::NeedsAttention
        )
        && matches!(
            unit.execution_status,
            ThreadExecutionStatus::Running
                | ThreadExecutionStatus::Completed
                | ThreadExecutionStatus::BudgetLimited
                | ThreadExecutionStatus::Failed
                | ThreadExecutionStatus::Cancelled
        )
}

fn terminal_turn(state: &TurnState) -> bool {
    matches!(
        state,
        TurnState::Completed | TurnState::Failed { .. } | TurnState::Interrupted { .. }
    )
}

fn failed_turn(state: &TurnState) -> bool {
    matches!(
        state,
        TurnState::Failed { .. } | TurnState::Interrupted { .. }
    )
}

fn turn_status(state: &TurnState) -> &'static str {
    match state {
        TurnState::Queued => "queued",
        TurnState::InProgress { .. } => "inProgress",
        TurnState::Completed => "completed",
        TurnState::Failed { .. } => "failed",
        TurnState::Interrupted { .. } => "interrupted",
    }
}

fn validate_turn_suffix(target: &StudioTaskRecoveryTarget, selected: &[String]) -> Result<()> {
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

async fn selected_input_hashes(
    store: &crate::studio::StudioStore,
    thread_id: &str,
    turn_ids: &[String],
    mode: ConversationRecoveryMode,
) -> Result<Vec<String>> {
    let inputs = store.conversation_turn_inputs(thread_id, turn_ids).await?;
    flatten_input_hashes(
        &inputs,
        turn_ids,
        matches!(mode, ConversationRecoveryMode::RewindTail),
    )
}

fn flatten_input_hashes(
    inputs: &BTreeMap<String, crate::studio::store::conversation_recovery::ConversationTurnInputs>,
    turn_ids: &[String],
    require_every_turn: bool,
) -> Result<Vec<String>> {
    let mut hashes = Vec::new();
    for turn_id in turn_ids {
        let turn_inputs = inputs.get(turn_id);
        if require_every_turn && turn_inputs.is_none_or(|inputs| inputs.hashes.is_empty()) {
            bail!("Selected Turn has no precisely matched consumed mailbox input");
        }
        if let Some(turn_inputs) = turn_inputs {
            hashes.extend(turn_inputs.hashes.clone());
        }
    }
    Ok(hashes)
}

fn record_fingerprint(value: &impl serde::Serialize) -> Result<String> {
    Ok(pl_core::canonical_json_hash(&serde_json::to_value(value)?))
}

fn preview_token(preview: &StudioTaskRecoveryPreview) -> Result<String> {
    let mut value = serde_json::to_value(preview)?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "previewToken".to_string(),
            serde_json::Value::String(String::new()),
        );
    }
    Ok(pl_core::canonical_json_hash(&value))
}

#[cfg(test)]
mod tests {
    use pl_protocol::ThreadToolCall;

    use super::*;
    use crate::studio::StudioTaskGitFingerprint;

    fn target(turn_ids: &[&str]) -> StudioTaskRecoveryTarget {
        StudioTaskRecoveryTarget {
            thread_id: "executor-1".to_string(),
            kind: StudioTaskRecoveryTargetKind::Executor,
            work_unit_id: Some("work-1".to_string()),
            attempt: Some(1),
            continuation_revision: Some(3),
            expected_runtime_revision: 7,
            expected_thread_revision: 11,
            branch: "task/work-1".to_string(),
            worktree_path: "worktree".to_string(),
            turns: turn_ids
                .iter()
                .map(|turn_id| StudioTaskRecoveryTurn {
                    turn_id: (*turn_id).to_string(),
                    status: "failed".to_string(),
                    updated_at: 1,
                    item_count: 1,
                    input_count: 1,
                    tool_count: 0,
                    tool_summaries: Vec::new(),
                })
                .collect(),
            default_turn_ids: vec![turn_ids.last().unwrap().to_string()],
            available_modes: vec![ConversationRecoveryMode::RewindTail],
            git_fingerprint: StudioTaskGitFingerprint {
                workspace_root: "worktree".to_string(),
                git_common_dir: ".git".to_string(),
                branch: "task/work-1".to_string(),
                head: "head".to_string(),
                base_commit: "base".to_string(),
                expected_head: "head".to_string(),
                operation: "none".to_string(),
                index_diff_hash: "index".to_string(),
                working_tree_diff_hash: "working".to_string(),
                untracked_content_hash: "untracked".to_string(),
            },
        }
    }

    #[test]
    fn turn_selection_accepts_only_a_contiguous_suffix() {
        let target = target(&["turn-1", "turn-2", "turn-3"]);

        validate_turn_suffix(&target, &["turn-2".to_string(), "turn-3".to_string()]).unwrap();
        let error = validate_turn_suffix(&target, &["turn-1".to_string(), "turn-3".to_string()])
            .unwrap_err();

        assert!(error.to_string().contains("continuous suffix"));
    }

    #[test]
    fn recovery_phase_excludes_review_merge_and_terminal_states() {
        for phase in [
            TaskRunPhase::Merging,
            TaskRunPhase::Reviewing,
            TaskRunPhase::Stopping,
            TaskRunPhase::Completed,
            TaskRunPhase::Blocked,
            TaskRunPhase::Failed,
            TaskRunPhase::Cancelled,
        ] {
            assert!(ensure_recoverable_phase(phase).is_err(), "{phase:?}");
        }
    }

    #[test]
    fn tool_summary_reports_side_effect_outcome_without_arguments() {
        let tool = ThreadToolCall {
            tool_call_id: "call-1".to_string(),
            call_id: None,
            provider_item_id: None,
            name: "shell_command".to_string(),
            arguments: r#"{"command":"secret"}"#.to_string(),
            result: None,
            output_artifacts: Vec::new(),
            exit_code: Some(1),
            timed_out: false,
            working_directory: Some("workspace".to_string()),
            denial_reason: None,
        };

        assert_eq!(tool_summary(&tool), "shell_command (exit 1)");
    }
}
