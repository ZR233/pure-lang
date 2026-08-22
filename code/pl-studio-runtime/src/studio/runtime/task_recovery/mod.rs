//! Task 会话恢复的 Studio runtime 入口:preview 生成与 apply 提交。
//!
//! 按域拆分:`preview` 收集恢复候选并生成无状态快照,`validation` 复核 apply
//! 前后的 durable 事实,`facts` 提供 fingerprint/状态/token 投影 helper。

mod facts;
mod preview;
mod validation;

use anyhow::{Context, Result, bail};
use pl_core::{
    AgentSubmitRequest, AgentTurnSubmitPolicy, ConversationRecoveryRequest,
    ConversationRecoveryTarget, MailboxPresentation, ThreadId,
};

use crate::studio::agent_host::root_agent_id;
use crate::studio::{
    StudioTaskRecoveryPreview, StudioTaskRecoveryRequest, StudioTaskRecoveryResult,
};

use super::StudioRuntime;
use facts::{selected_input_hashes, task_kind_from_recovery_state};
use validation::validate_turn_suffix;

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
        let target_agent_id = ThreadId::new(request.target_thread_id.clone())?;
        let recovery = if let Some(record) = existing_recovery {
            record.into()
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

        let phase = task_kind_from_recovery_state(request.preview.state);
        let stop_cleared = if request.preview.stop_requested {
            self.store
                .clear_task_stop_for_recovery(
                    &request.preview.run_id,
                    request.preview.task_generation,
                    phase,
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
        })
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        ConversationRecoveryMode, SucceededThreadTool, ThreadToolInvocation, ThreadToolItem,
        ThreadToolOutput, ThreadToolState,
    };

    use super::facts::tool_summary;
    use super::preview::ensure_recoverable_phase;
    use crate::studio::task_coordinator::TaskRunStateKind;
    use crate::studio::{
        StudioTaskRecoveryTarget, StudioTaskRecoveryTargetKind, StudioTaskRecoveryTurn,
    };

    use super::*;

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
                    state: pl_protocol::studio::StudioTaskRecoveryTurnState::Failed,
                    updated_at: 1,
                    item_count: 1,
                    input_count: 1,
                    tool_count: 0,
                    tool_summaries: Vec::new(),
                })
                .collect(),
            default_turn_ids: vec![turn_ids.last().unwrap().to_string()],
            available_modes: vec![ConversationRecoveryMode::RewindTail],
            base_commit: Some("base".to_string()),
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
            TaskRunStateKind::Merging,
            TaskRunStateKind::Reviewing,
            TaskRunStateKind::Stopping,
            TaskRunStateKind::Completed,
            TaskRunStateKind::Blocked,
            TaskRunStateKind::Failed,
            TaskRunStateKind::Cancelled,
        ] {
            assert!(ensure_recoverable_phase(phase).is_err(), "{phase:?}");
        }
    }

    #[test]
    fn tool_summary_reports_side_effect_outcome_without_arguments() {
        let tool = ThreadToolItem::new(
            ThreadToolInvocation::new(
                "call-1".to_string(),
                "shell_command".to_string(),
                r#"{"command":"secret"}"#.to_string(),
            )
            .with_working_directory(Some("workspace".to_string())),
            ThreadToolState::Succeeded(SucceededThreadTool::new(
                1,
                ThreadToolOutput::new(String::new(), Vec::new(), Some(1)),
            )),
        );

        assert_eq!(tool_summary(&tool), "shell_command (exit 1)");
    }
}
