mod accept;
mod barriers;
mod cleanup;
mod conflict;
mod conflict_index;
mod conflict_status;
pub(crate) mod conflict_tools;
mod failure;
mod git;
mod output;
mod process;
mod recovery;
mod scope;
mod validation;
mod verifier;
#[cfg(test)]
#[path = "tests/verifier.rs"]
mod verifier_tests;
pub(super) use conflict::validate_conflict_recovery;
pub(crate) use recovery::MergeRestartRecovery;

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[cfg(test)]
pub(super) use barriers::MergeFailureTestPoint;
#[cfg(test)]
pub(crate) use cleanup::MergeCleanupTestBarrier;
pub(crate) use verifier::MergeVerifier;
pub(crate) use verifier::{ProductionMergeVerifier, select_merge_verification_commands};

#[cfg(test)]
pub(crate) use self::accept::MergeCommitTestBarrier;
use self::accept::{
    MergeCommitProof, merge_commit_message, pending_cleanup, verify_created_merge_commit,
};
pub(crate) use self::cleanup::cleanup_accepted_delivery;
use self::failure::MergeFailureStage;
use self::git::run_git;
use self::output::merged_output;
use self::validation::{validate_final_head, validate_merge_preflight};
use super::{
    BeginTaskMerge, CompleteTaskMerge, MergeStatus, MergeVerificationRequest, TaskCoordinator,
    TaskMergeAgentOutput, TaskMergeScope,
};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::{AgentRuntimeHandle, ToolEffect};

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
}

impl TaskCoordinator {
    pub(crate) fn task_merge_agent_tool(
        self: &Arc<Self>,
        session_id: impl Into<String>,
        runtime: AgentRuntimeHandle,
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
            move |arguments: TaskMergeAgentInput, _context| {
                let coordinator = coordinator.clone();
                let session_id = session_id.clone();
                let runtime = runtime.clone();
                async move {
                    let output = coordinator
                        .merge_agent(
                            MergeAgentRequest {
                                session_id: &session_id,
                                agent_id: &arguments.agent_id,
                                expected_head: &arguments.expected_head_commit,
                            },
                            Some(&runtime),
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
        runtime: Option<&AgentRuntimeHandle>,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        let agent_id = request.agent_id.trim();
        let caller_expected_head = request.expected_head.trim();
        if agent_id.is_empty() || caller_expected_head.is_empty() {
            bail!("agentId and expectedHeadCommit must not be empty");
        }
        if let Some(scope) = self
            .store
            .find_accepted_merge_scope(request.session_id, agent_id, caller_expected_head)
            .await?
        {
            self.ensure_process_lease_owned(&scope.run)?;
            self.validate_accepted_cleanup_replay(&scope).await?;
            let output = merged_output(&scope)?;
            return self
                .finish_accepted_delivery_cleanup(&scope, output, runtime)
                .await;
        }

        let (scope, output) = {
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
        self.finish_accepted_delivery_cleanup(&scope, output, runtime)
            .await
    }

    #[cfg(test)]
    #[expect(
        clippy::too_many_arguments,
        reason = "merge tests keep each injected dependency and identity explicit"
    )]
    pub(crate) async fn merge_agent_with_verifier<V: MergeVerifier, R>(
        &self,
        session_id: &str,
        agent_id: &str,
        caller_expected_head: &str,
        _runtime_marker: &R,
        _event_marker: &pl_trace::AgentEventSender,
        _call_id: &str,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        self.merge_agent(
            MergeAgentRequest {
                session_id,
                agent_id,
                expected_head: caller_expected_head,
            },
            None,
            verifier,
        )
        .await
    }

    async fn merge_clean_locked<V: MergeVerifier>(
        &self,
        scope: &TaskMergeScope,
        workspace: &std::path::Path,
        verifier: &V,
    ) -> Result<TaskMergeAgentOutput> {
        let merge_output = match run_git(
            workspace,
            vec![
                "merge".into(),
                "--no-ff".into(),
                "--no-commit".into(),
                scope.work_unit.branch.clone(),
            ],
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        Vec::new(),
                        error.context("git merge runner failed"),
                        MergeFailureStage::BeforeCommit,
                    )
                    .await;
            }
        };
        if !merge_output.success {
            return self
                .handle_non_clean_merge(scope, workspace, merge_output.stderr_lossy())
                .await;
        }
        if let Err(error) = self.mark_task_merge_verifying(scope).await {
            return self
                .handle_merge_stage_failure(
                    scope,
                    workspace,
                    Vec::new(),
                    error.context("mark task merge verifying"),
                    MergeFailureStage::BeforeCommit,
                )
                .await;
        }
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
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        steps,
                        anyhow::anyhow!("merge verification returned a failed check"),
                        MergeFailureStage::BeforeCommit,
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        Vec::new(),
                        error.context("merge verifier runner failed"),
                        MergeFailureStage::BeforeCommit,
                    )
                    .await;
            }
        };
        let expected_tree = match self.read_merge_index_tree(workspace).await {
            Ok(tree) => tree,
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        verification,
                        error.context("capture verified merge index tree"),
                        MergeFailureStage::BeforeCommit,
                    )
                    .await;
            }
        };
        let message = merge_commit_message(scope, &verification);
        let commit = match self.run_merge_commit(workspace, message).await {
            Ok(commit) => commit,
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        verification,
                        error.context("create merge commit"),
                        MergeFailureStage::CommitAttempted {
                            expected_tree: Some(expected_tree),
                        },
                    )
                    .await;
            }
        };
        if !commit.success {
            return self
                .handle_merge_stage_failure(
                    scope,
                    workspace,
                    verification,
                    anyhow::anyhow!("merge commit failed: {}", commit.stderr_lossy()),
                    MergeFailureStage::CommitAttempted {
                        expected_tree: Some(expected_tree),
                    },
                )
                .await;
        }
        let merge_commit = match self.read_post_commit_head(workspace).await {
            Ok(head) => head,
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        verification,
                        error.context("read merge commit HEAD"),
                        MergeFailureStage::CommitAttempted {
                            expected_tree: Some(expected_tree),
                        },
                    )
                    .await;
            }
        };
        let proof = MergeCommitProof {
            commit: merge_commit.clone(),
            expected_tree,
        };
        self.pause_before_merge_proof().await;
        if let Err(error) = verify_created_merge_commit(scope, workspace, &proof).await {
            return self
                .handle_merge_stage_failure(
                    scope,
                    workspace,
                    verification,
                    error.context("merge commit proof failed"),
                    MergeFailureStage::CommitAttempted {
                        expected_tree: Some(proof.expected_tree.clone()),
                    },
                )
                .await;
        }
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
                .handle_merge_stage_failure(
                    scope,
                    workspace,
                    verification,
                    error.context("durable merge CAS failed"),
                    MergeFailureStage::CommitAttempted {
                        expected_tree: Some(proof.expected_tree),
                    },
                )
                .await;
        }
        self.pause_after_merge_acceptance().await;
        let durable_run = match self.read_accepted_task_run(&scope.run.id).await {
            Ok(Some(run)) => run,
            Ok(None) => {
                return self
                    .block_accepted_scope_failure(
                        scope,
                        anyhow::anyhow!("accepted task run disappeared after merge CAS"),
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .block_accepted_scope_failure(
                        scope,
                        error.context("read accepted task run after merge CAS"),
                    )
                    .await;
            }
        };
        if let Err(error) = validate_final_head(&durable_run, &merge_commit).await {
            return self.block_accepted_scope_failure(scope, error).await;
        }
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

    async fn block_accepted_scope_failure(
        &self,
        scope: &TaskMergeScope,
        error: anyhow::Error,
    ) -> Result<TaskMergeAgentOutput> {
        let reason = format!("accepted merge final scope validation failed: {error:#}");
        let block = self
            .store
            .block_accepted_merge(
                &scope.merge.id,
                &reason,
                super::MergeCleanupEvidence {
                    status: "deferred".to_string(),
                    detail: Some(reason.clone()),
                },
            )
            .await;
        self.release_owned_process_lease(&scope.run.id);
        match block {
            Ok(_) => Err(error).context(reason),
            Err(block_error) => Err(error).context(format!(
                "{reason}; accepted-merge block persistence also failed: {block_error:#}"
            )),
        }
    }

    async fn handle_non_clean_merge(
        &self,
        scope: &TaskMergeScope,
        workspace: &std::path::Path,
        detail: String,
    ) -> Result<TaskMergeAgentOutput> {
        let merge_head = match run_git(
            workspace,
            vec!["rev-parse".into(), "--verify".into(), "MERGE_HEAD".into()],
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                return self
                    .handle_merge_stage_failure(
                        scope,
                        workspace,
                        Vec::new(),
                        error.context("inspect MERGE_HEAD after non-clean merge"),
                        MergeFailureStage::Conflict,
                    )
                    .await;
            }
        };
        if merge_head.success {
            return self.persist_merge_conflict(scope, workspace).await;
        }
        self.handle_merge_stage_failure(
            scope,
            workspace,
            Vec::new(),
            anyhow::anyhow!(detail),
            MergeFailureStage::BeforeCommit,
        )
        .await
    }
}
