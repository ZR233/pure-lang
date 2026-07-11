mod accept;
mod cleanup;
mod conflict;
mod git;
mod process;
mod recovery;
mod validation;
mod verifier;
pub(super) use conflict::validate_conflict_recovery;
pub(crate) use recovery::MergeRestartRecovery;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[cfg(test)]
pub(crate) use cleanup::MergeCleanupTestBarrier;
pub(crate) use verifier::MergeVerifier;
use verifier::ProductionMergeVerifier;
#[cfg(test)]
pub(crate) use verifier::{MergeVerificationCommand, select_merge_verification_commands};

#[cfg(test)]
pub(crate) use self::accept::MergeCommitTestBarrier;
use self::accept::{merge_commit_message, pending_cleanup, verify_created_merge_commit};
pub(crate) use self::cleanup::cleanup_accepted_delivery;
use self::git::{checked_git, run_git};
use self::validation::{
    ensure_preflight_delivery_identity, validate_final_head, validate_merge_preflight,
};
use super::{
    BeginTaskMerge, CompleteTaskMerge, MergeStatus, MergeVerificationRequest, TaskCoordinator,
    TaskMergeAgentOutput, TaskMergeScope, TaskRunPhase,
};
use crate::AgentSupervisor;
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::turn::{CompileMode, ToolEffect};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskMergeAgentInput {
    agent_id: String,
    expected_head_commit: String,
}

struct MergeAgentRequest<'a> {
    session_id: &'a str,
    agent_id: &'a str,
    expected_head: &'a str,
    call_id: &'a str,
}

impl TaskCoordinator {
    pub(crate) fn task_merge_agent_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let session_id = session_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "task_merge_agent",
            "Merge one validated executor delivery into the task branch.",
            strict_tool_input_schema([
                ToolInputSchemaField::required("agentId", serde_json::json!({ "type": "string" })),
                ToolInputSchemaField::required(
                    "expectedHeadCommit",
                    serde_json::json!({ "type": "string" }),
                ),
            ]),
            move |arguments: TaskMergeAgentInput, context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                async move {
                    if context.mode != CompileMode::Task || context.active_subagent.is_some() {
                        bail!("task_merge_agent requires the root Task planner");
                    }
                    let call_id = context
                        .provider_call_id
                        .as_deref()
                        .unwrap_or("task-merge-agent");
                    let output = coordinator
                        .merge_agent(
                            MergeAgentRequest {
                                session_id: &session_id,
                                agent_id: &arguments.agent_id,
                                expected_head: &arguments.expected_head_commit,
                                call_id,
                            },
                            &context.agent_supervisor,
                            &context.event_tx,
                            &ProductionMergeVerifier,
                        )
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(output)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    async fn merge_agent<V: MergeVerifier>(
        &self,
        request: MergeAgentRequest<'_>,
        supervisor: &AgentSupervisor,
        event_tx: &pl_trace::AgentEventSender,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        let agent_id = request.agent_id.trim();
        let caller_expected_head = request.expected_head.trim();
        if agent_id.is_empty() || caller_expected_head.is_empty() {
            bail!("agentId and expectedHeadCommit must not be empty");
        }

        let (scope, mut output) = {
            let guard = self.lock_branch_mutation().await;
            self.ensure_branch_mutation_guard(&guard)?;
            let scope = self
                .load_merge_preflight_scope(request.session_id, agent_id)
                .await?;
            self.ensure_process_lease_owned(&scope.run)?;
            let preflight = validate_merge_preflight(
                &scope.run,
                &scope.lease,
                &scope.work_unit,
                &scope.delivery,
                caller_expected_head,
            )
            .await?;
            let scope = self
                .store
                .begin_task_merge(BeginTaskMerge {
                    session_id: request.session_id.to_string(),
                    agent_id: agent_id.to_string(),
                    expected_head: caller_expected_head.to_string(),
                    pre_index_tree: preflight.pre_index_tree,
                    changed_files: scope.delivery.changed_files.clone(),
                })
                .await?;
            let output = self
                .merge_clean_locked(&scope, &preflight.workspace, verifier)
                .await?;
            (scope, output)
        };

        if output.status == MergeStatus::Conflicted {
            return Ok(output);
        }
        self.pause_before_merge_cleanup().await;
        let cleanup =
            cleanup_accepted_delivery(&scope, supervisor, event_tx, request.call_id).await;
        self.store
            .record_merge_cleanup(&scope.merge.id, cleanup.clone())
            .await?;
        output.cleanup = cleanup;
        Ok(output)
    }

    #[cfg(test)]
    #[expect(
        clippy::too_many_arguments,
        reason = "merge tests keep each injected dependency and identity explicit"
    )]
    pub(crate) async fn merge_agent_with_verifier<V: MergeVerifier>(
        &self,
        session_id: &str,
        agent_id: &str,
        caller_expected_head: &str,
        supervisor: &AgentSupervisor,
        event_tx: &pl_trace::AgentEventSender,
        call_id: &str,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        self.merge_agent(
            MergeAgentRequest {
                session_id,
                agent_id,
                expected_head: caller_expected_head,
                call_id,
            },
            supervisor,
            event_tx,
            verifier,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) fn set_merge_cleanup_barrier(&self, barrier: MergeCleanupTestBarrier) {
        *self
            .merge_cleanup_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    async fn pause_before_merge_cleanup(&self) {
        let barrier = self
            .merge_cleanup_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(barrier) = barrier {
            barrier.pause().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_before_merge_cleanup(&self) {}

    async fn load_merge_preflight_scope(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<TaskMergeScope> {
        let run = self
            .store
            .read_active_task_run_for_session(session_id)
            .await?;
        if !matches!(
            run.phase,
            TaskRunPhase::Implementing | TaskRunPhase::Reworking
        ) {
            bail!("task merge requires phase implementing or reworking");
        }
        let lease = self
            .store
            .read_branch_lease(&run.id)
            .await?
            .context("task branch lease not found")?;
        let outcomes = self
            .store
            .list_agent_outcomes(&run.id)
            .await?
            .into_iter()
            .filter(|outcome| outcome.agent_id == agent_id)
            .collect::<Vec<_>>();
        let outcome = match outcomes.as_slice() {
            [outcome] => outcome.clone(),
            [] => bail!("delivered executor outcome not found for agent"),
            _ => bail!("ambiguous executor outcome for agent"),
        };
        let work_unit_id = outcome
            .work_unit_id
            .as_deref()
            .context("executor outcome has no work unit")?;
        let work_unit = self
            .store
            .read_work_unit(work_unit_id)
            .await?
            .context("executor work unit not found")?;
        let delivery = outcome
            .delivery
            .clone()
            .context("completed executor outcome has no delivery")?;
        ensure_preflight_delivery_identity(&run.id, agent_id, &work_unit, &outcome, &delivery)?;
        Ok(TaskMergeScope {
            #[cfg(test)]
            origin_phase: run.phase,
            run,
            lease,
            work_unit,
            outcome,
            delivery,
            merge: empty_preflight_merge(),
        })
    }

    async fn merge_clean_locked<V: MergeVerifier>(
        &self,
        scope: &TaskMergeScope,
        workspace: &std::path::Path,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        let merge_output = run_git(
            workspace,
            vec![
                "merge".into(),
                "--no-ff".into(),
                "--no-commit".into(),
                scope.work_unit.branch.clone(),
            ],
        )
        .await?;
        if !merge_output.success {
            return self
                .handle_non_clean_merge(scope, workspace, merge_output.stderr_lossy())
                .await;
        }
        self.store
            .mark_task_merge_verifying(&scope.merge.id)
            .await?;
        let verification = match verifier
            .verify(MergeVerificationRequest {
                workspace_root: scope.run.workspace_root.clone(),
                changed_files: scope.delivery.changed_files.clone(),
            })
            .await
        {
            Ok(steps) if steps.iter().all(|step| step.success) => steps,
            Ok(steps) => {
                return self
                    .fail_uncommitted_merge(
                        scope,
                        workspace,
                        steps,
                        "merge verification returned a failed check".to_string(),
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .fail_uncommitted_merge(scope, workspace, Vec::new(), error.to_string())
                    .await;
            }
        };
        let message = merge_commit_message(scope, &verification);
        let commit = run_git(workspace, vec!["commit".into(), "-m".into(), message]).await?;
        if !commit.success {
            return self
                .fail_uncommitted_merge(
                    scope,
                    workspace,
                    verification,
                    format!("merge commit failed: {}", commit.stderr_lossy()),
                )
                .await;
        }
        let merge_commit = checked_git(workspace, vec!["rev-parse".into(), "HEAD".into()]).await?;
        verify_created_merge_commit(scope, workspace, &merge_commit).await?;
        self.pause_after_merge_commit().await;
        let completed = self
            .store
            .complete_task_merge(CompleteTaskMerge {
                merge_id: scope.merge.id.clone(),
                expected_head: scope.run.expected_head.clone(),
                merge_commit: merge_commit.clone(),
                verification_steps: verification.clone(),
            })
            .await;
        if let Err(error) = completed {
            return self
                .compensate_failed_durable_cas(scope, workspace, &merge_commit, verification, error)
                .await;
        }
        let durable_run = self
            .store
            .read_task_run(&scope.run.id)
            .await?
            .context("accepted task run disappeared")?;
        validate_final_head(&durable_run, &merge_commit).await?;
        Ok(TaskMergeAgentOutput {
            merge_id: scope.merge.id.clone(),
            status: MergeStatus::Merged,
            previous_head: scope.run.expected_head.clone(),
            new_head: Some(merge_commit),
            agent_id: scope.outcome.agent_id.clone(),
            source_commit: scope.delivery.head_commit.clone(),
            changed_files: scope.delivery.changed_files.clone(),
            verification,
            cleanup: pending_cleanup(),
            conflict_files: Vec::new(),
        })
    }

    #[cfg(test)]
    pub(crate) fn set_merge_after_commit_barrier(&self, barrier: MergeCommitTestBarrier) {
        *self
            .merge_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(barrier);
    }

    #[cfg(test)]
    async fn pause_after_merge_commit(&self) {
        let barrier = self
            .merge_after_commit_barrier
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(barrier) = barrier {
            barrier.pause().await;
        }
    }

    #[cfg(not(test))]
    async fn pause_after_merge_commit(&self) {}

    async fn handle_non_clean_merge(
        &self,
        scope: &TaskMergeScope,
        workspace: &std::path::Path,
        detail: String,
    ) -> Result<TaskMergeAgentOutput> {
        let merge_head = run_git(
            workspace,
            vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
        )
        .await?;
        if merge_head.success {
            return self.persist_merge_conflict(scope, workspace).await;
        }
        self.fail_uncommitted_merge(scope, workspace, Vec::new(), detail)
            .await
    }
}

fn empty_preflight_merge() -> super::MergeRecord {
    super::MergeRecord {
        id: String::new(),
        task_run_id: String::new(),
        agent_id: String::new(),
        status: MergeStatus::Pending,
        expected_head: String::new(),
        source_commit: String::new(),
        conflict_files: Vec::new(),
        resolution_summary: None,
        verification: None,
        evidence: None,
        attempt: 0,
        created_at: 0,
        updated_at: 0,
    }
}
