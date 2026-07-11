use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::git::{changed_files_between, inspect_repository, is_ancestor};
use super::{
    AgentDelivery, AgentOutcomeRecord, AgentOutcomeStatus, AgentWorktreeDelivery, TaskCoordinator,
    TaskRunRecord, UpdateAgentOutcome, WorkUnitRecord, WorkUnitStatus,
};
use crate::tool::{
    RegisteredTool, SubagentContext, ToolExecutionResult, ToolInputSchemaField,
    strict_tool_input_schema,
};
use crate::turn::ToolEffect;
use crate::{AgentToolRegistrar, PureCore};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmitDeliveryInput {
    head_commit: String,
    verification_summary: String,
}

struct DeliveryValidation<'a> {
    run: &'a TaskRunRecord,
    outcome: &'a AgentOutcomeRecord,
    work_unit: Option<&'a WorkUnitRecord>,
    subagent: &'a SubagentContext,
    caller_workspace: &'a Path,
    supplied_head: &'a str,
    verification_summary: &'a str,
}

struct TaskToolRegistrar {
    coordinator: Arc<TaskCoordinator>,
}

impl std::fmt::Debug for TaskToolRegistrar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskToolRegistrar")
            .finish_non_exhaustive()
    }
}

impl AgentToolRegistrar for TaskToolRegistrar {
    fn register_tools<'a>(
        &'a self,
        core: &'a mut PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) -> Pin<Box<dyn Future<Output = pl_protocol::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            core.register_default_tools(workspace_root, workspace_instructions)
                .await;
            core.register_tool(self.coordinator.submit_delivery_tool());
            Ok(())
        })
    }
}

impl TaskCoordinator {
    pub(crate) fn install_tools(self: &Arc<Self>, core: &mut PureCore) {
        core.register_tool(self.submit_delivery_tool());
        core.set_agent_tool_registrar(Arc::new(TaskToolRegistrar {
            coordinator: self.clone(),
        }));
    }

    pub(crate) async fn submit_delivery(
        &self,
        session_id: &str,
        subagent: &SubagentContext,
        caller_workspace: impl AsRef<Path>,
        supplied_head: &str,
        verification_summary: &str,
    ) -> Result<AgentDelivery> {
        let run = self
            .store
            .read_active_task_run_by_session(session_id)
            .await?
            .context("active task run not found for this session")?;
        let outcome = self
            .store
            .read_agent_outcome_by_agent(&run.id, &subagent.id)
            .await?
            .context("agent outcome not found for this task executor")?;
        let work_unit = match outcome.work_unit_id.as_deref() {
            Some(work_unit_id) => self.store.read_work_unit(work_unit_id).await?,
            None => None,
        };

        let result = self
            .validate_delivery(DeliveryValidation {
                run: &run,
                outcome: &outcome,
                work_unit: work_unit.as_ref(),
                subagent,
                caller_workspace: caller_workspace.as_ref(),
                supplied_head,
                verification_summary,
            })
            .await;
        let (work_unit, delivery) = match result {
            Ok(result) => result,
            Err(error) => {
                self.mark_waiting_for_delivery(&outcome, work_unit.as_ref(), error.to_string())
                    .await?;
                return Err(error);
            }
        };

        self.store
            .update_agent_outcome(
                &outcome.id,
                UpdateAgentOutcome {
                    status: AgentOutcomeStatus::Completed,
                    summary: Some(delivery.verification_summary.clone()),
                    error: None,
                    delivery: Some(delivery.clone()),
                    review: None,
                },
            )
            .await?;
        self.store
            .update_work_unit(
                &work_unit.id,
                WorkUnitStatus::Delivered,
                Some(subagent.id.clone()),
            )
            .await?;
        Ok(delivery)
    }

    pub(crate) fn submit_delivery_tool(self: &Arc<Self>) -> RegisteredTool {
        let coordinator = self.clone();
        RegisteredTool::from_typed_tool_input_fallible_execution_result(
            "submit_delivery",
            "Validate and persist a committed executor delivery.",
            strict_tool_input_schema([
                ToolInputSchemaField::required(
                    "headCommit",
                    serde_json::json!({ "type": "string" }),
                ),
                ToolInputSchemaField::required(
                    "verificationSummary",
                    serde_json::json!({ "type": "string" }),
                ),
            ]),
            move |arguments: SubmitDeliveryInput, input, context| {
                let coordinator = coordinator.clone();
                async move {
                    let subagent = context
                        .active_subagent
                        .as_ref()
                        .context("submit_delivery requires an active executor subagent")?;
                    let delivery = coordinator
                        .submit_delivery(
                            &input.session_id,
                            subagent,
                            &context.workspace_root,
                            &arguments.head_commit,
                            &arguments.verification_summary,
                        )
                        .await?;
                    ToolExecutionResult::<serde_json::Value>::json(delivery)
                        .map_err(anyhow::Error::from)
                }
            },
        )
        .with_effect(ToolEffect::BranchControl)
    }

    async fn validate_delivery<'a>(
        &self,
        validation: DeliveryValidation<'a>,
    ) -> Result<(&'a WorkUnitRecord, AgentDelivery)> {
        let DeliveryValidation {
            run,
            outcome,
            work_unit,
            subagent,
            caller_workspace,
            supplied_head,
            verification_summary,
        } = validation;
        if subagent.role != "executor" || outcome.role != "executor" {
            bail!("submit_delivery may only be called by the assigned executor");
        }
        let owner_path = subagent
            .parent_id
            .as_deref()
            .context("executor has no owner path")?;
        if outcome.owner_path != owner_path || outcome.task_run_id != run.id {
            bail!("executor does not own this task outcome");
        }
        let work_unit = work_unit.context("executor outcome has no work unit")?;
        if work_unit.task_run_id != run.id
            || work_unit.agent_id.as_deref() != Some(subagent.id.as_str())
        {
            bail!("executor does not own this work unit");
        }
        if !(1..=3).contains(&outcome.attempt)
            || !(1..=3).contains(&work_unit.attempt)
            || outcome.attempt != work_unit.attempt
        {
            bail!("delivery attempt must be within 1..=3 and match the work unit");
        }
        let verification_summary = verification_summary.trim();
        if verification_summary.is_empty() {
            bail!("verificationSummary must not be empty");
        }
        let supplied_head = supplied_head.trim();
        if supplied_head.is_empty() {
            bail!("headCommit must not be empty");
        }

        let snapshot = inspect_repository(caller_workspace, true).await?;
        if normalized_path(&snapshot.git_common_dir)
            != normalized_path(Path::new(&run.git_common_dir))
        {
            bail!("executor worktree does not belong to the active task repository");
        }
        if normalized_path(caller_workspace) != normalized_path(Path::new(&work_unit.worktree_path))
            || snapshot.branch != work_unit.branch
        {
            bail!(
                "caller workspace and branch do not match the assigned worktree {} on branch {}",
                work_unit.worktree_path,
                work_unit.branch
            );
        }
        if snapshot.head != supplied_head {
            bail!(
                "supplied headCommit {supplied_head} does not match worktree HEAD {}",
                snapshot.head
            );
        }
        let base_commit = work_unit.base_commit.as_str();
        if snapshot.head == base_commit {
            bail!("delivery HEAD must advance beyond base commit {base_commit}");
        }
        if !is_ancestor(&snapshot.workspace_root, base_commit, &snapshot.head).await? {
            bail!("delivery HEAD must descend from base commit {base_commit}");
        }
        let changed_files =
            changed_files_between(&snapshot.workspace_root, base_commit, &snapshot.head).await?;
        validate_owned_paths(&work_unit.owned_paths, &changed_files)?;

        Ok((
            work_unit,
            AgentDelivery {
                worktree: AgentWorktreeDelivery {
                    path: snapshot.workspace_root.to_string_lossy().to_string(),
                    branch: snapshot.branch,
                },
                base_commit: base_commit.to_string(),
                head_commit: snapshot.head,
                changed_files,
                verification_summary: verification_summary.to_string(),
            },
        ))
    }

    async fn mark_waiting_for_delivery(
        &self,
        outcome: &AgentOutcomeRecord,
        work_unit: Option<&WorkUnitRecord>,
        error: String,
    ) -> Result<()> {
        self.store
            .update_agent_outcome(
                &outcome.id,
                UpdateAgentOutcome {
                    status: AgentOutcomeStatus::WaitingForDelivery,
                    summary: outcome.summary.clone(),
                    error: Some(error),
                    delivery: outcome.delivery.clone(),
                    review: outcome.review.clone(),
                },
            )
            .await?;
        if let Some(work_unit) = work_unit {
            self.store
                .update_work_unit(
                    &work_unit.id,
                    WorkUnitStatus::WaitingForDelivery,
                    work_unit.agent_id.clone(),
                )
                .await?;
        }
        Ok(())
    }
}

fn validate_owned_paths(owned_paths: &[String], changed_files: &[String]) -> Result<()> {
    let owned_paths = owned_paths
        .iter()
        .map(|path| normalize_owned_path(path))
        .collect::<Result<Vec<_>>>()?;
    for changed_file in changed_files {
        if !owned_paths.iter().any(|owned| owned.matches(changed_file)) {
            bail!("changed file `{changed_file}` is outside ownedPaths");
        }
    }
    Ok(())
}

#[derive(Debug)]
enum OwnedPath {
    Exact(String),
    Directory(String),
}

impl OwnedPath {
    fn matches(&self, changed_file: &str) -> bool {
        match self {
            Self::Exact(path) => changed_file == path,
            Self::Directory(path) => changed_file.starts_with(&format!("{path}/")),
        }
    }
}

fn normalize_owned_path(path: &str) -> Result<OwnedPath> {
    let normalized = path.trim().replace('\\', "/");
    let (path, directory) = normalized
        .strip_suffix("/**")
        .map_or((normalized.as_str(), false), |path| (path, true));
    if path.is_empty()
        || path.starts_with('/')
        || path.as_bytes().get(1) == Some(&b':')
        || Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("invalid owned path `{normalized}`: use a relative normalized path");
    }
    Ok(if directory {
        OwnedPath::Directory(path.to_string())
    } else {
        OwnedPath::Exact(path.to_string())
    })
}

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}
