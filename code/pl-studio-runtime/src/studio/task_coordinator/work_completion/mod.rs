mod state;

pub(crate) use state::*;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Deserialize;

use super::spawn::{TaskExecutorBlueprint, verification_result_map};
use super::{
    AgentDelivery, AgentWorktreeDelivery, DeliveryScope, TaskCoordinator, TaskRun,
    WorkUnitStateKind,
};
use crate::agent::worktree::git_compatible_path;
use crate::tool::{
    FunctionToolDefinition, NamespaceDescriptor, RegisteredTool, SubagentContext, ToolEntry,
    ToolExecutionResult, ToolSourceId, ToolSourceMetadata,
};
use crate::turn::ToolEffect;
use crate::{AgentProgressStage, AgentRuntimeHandle, AgentSnapshot, TurnEngine};

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum CompletionResultKindInput {
    Delivery,
    NoDelivery,
}

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionResultInput {
    /// Selects delivery or noDelivery validation.
    kind: CompletionResultKindInput,
    /// Full Git commit id or an unambiguous abbreviation of at least 7 hex characters.
    /// Required for delivery and forbidden for noDelivery.
    head_commit: Option<String>,
    /// Caller-declared normalized repository-relative paths. Required for delivery and forbidden
    /// for noDelivery.
    changed_files: Vec<String>,
    /// Successful outcomes for every command and inspection in the durable handoff.
    #[schemars(length(min = 1))]
    verification_results: Vec<VerificationResultInput>,
}

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerificationResultInput {
    check_id: String,
    summary: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ValidatedCompletionResultInput {
    Delivery {
        head_commit: String,
        changed_files: Vec<String>,
        verification_results: Vec<VerificationResultInput>,
    },
    NoDelivery {
        verification_results: Vec<VerificationResultInput>,
    },
}

impl TryFrom<CompletionResultInput> for ValidatedCompletionResultInput {
    type Error = anyhow::Error;

    fn try_from(input: CompletionResultInput) -> Result<Self> {
        match (input.kind, input.head_commit, input.changed_files) {
            (CompletionResultKindInput::Delivery, Some(head_commit), changed_files) => {
                Ok(Self::Delivery {
                    head_commit,
                    changed_files,
                    verification_results: input.verification_results,
                })
            }
            (CompletionResultKindInput::Delivery, None, _) => {
                bail!("delivery requires headCommit")
            }
            (CompletionResultKindInput::NoDelivery, Some(_), _) => {
                bail!("noDelivery must not include headCommit")
            }
            (CompletionResultKindInput::NoDelivery, None, changed_files) => {
                if !changed_files.is_empty() {
                    bail!("noDelivery must not include changedFiles")
                }
                Ok(Self::NoDelivery {
                    verification_results: input.verification_results,
                })
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CompletionValidation<'a> {
    scope: &'a DeliveryScope,
    subagent: &'a SubagentContext,
    caller_workspace: &'a Path,
    verification_summary: &'a str,
}

impl TaskCoordinator {
    pub(crate) fn install_tools(
        self: &Arc<Self>,
        core: &mut TurnEngine,
        thread_id: &str,
        runtime: AgentRuntimeHandle,
        snapshot: &AgentSnapshot,
        active_task_run: Option<&TaskRun>,
    ) {
        if active_task_run.is_none() {
            return;
        }
        // Task 协调工具统一发布到 task 来源（task 命名空间，延迟加载）。
        let source = ToolSourceId::task();
        let metadata = || {
            ToolSourceMetadata::new(source.clone()).with_namespace(NamespaceDescriptor::new(
                "task",
                "Task coordination, review, delivery, and completion tools.",
            ))
        };
        let mut entries = Vec::new();
        if snapshot.identity.parent_id.is_none() {
            // planner 复用框架统一的 send_message（parent→direct-child）调度子代理；
            // 不再注册 Task 专用 send_message。
            entries.push(ToolEntry::new(
                self.task_spawn_executor_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_transition_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_record_merge_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_request_delivery_review_tool(thread_id, runtime.clone()),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_status_tool(thread_id, Some(runtime.clone())),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.read_work_unit_handoff_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.read_review_round_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.read_review_file_coverage_tool(thread_id),
                metadata(),
            ));
            entries.push(ToolEntry::new(
                self.task_stop_tool(thread_id, runtime.clone()),
                metadata(),
            ));
        } else {
            match snapshot.identity.role.as_str() {
                "executor" => {
                    entries.push(ToolEntry::new(
                        self.report_completion_tool(runtime),
                        metadata(),
                    ));
                }
                "reviewer" => {
                    entries.push(ToolEntry::new(
                        self.read_review_file_coverage_tool(thread_id),
                        metadata(),
                    ));
                    entries.push(ToolEntry::new(
                        self.review_exit_tool(thread_id, Some(runtime)),
                        metadata(),
                    ));
                }
                "explorer" | "planner" => {}
                _ => {}
            }
        }
        let _ = core.register_source_tools(source, entries);
    }

    pub(crate) fn report_completion_tool(
        self: &Arc<Self>,
        runtime: AgentRuntimeHandle,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        FunctionToolDefinition::<CompletionResultInput>::new(
            "report_completion",
            "Report caller-declared executor delivery facts with changedFiles and verificationResults, then end the current turn for delivery review.",
        )
        .registered(move |result: CompletionResultInput, context| {
                let coordinator = coordinator.clone();
                let runtime = runtime.clone();
                async move {
                    let subagent = context
                        .active_subagent
                        .as_ref()
                        .context("report_completion requires an active executor")?;
                    let completion = coordinator
                        .report_completion(subagent, context.workspace.root(), result)
                        .await?;
                    if let Err(error) = runtime
                        .report_progress(
                            pl_core::ThreadId::new(subagent.id.clone())?,
                            AgentProgressStage::ReadyForReview,
                            format!(
                                "completion revision {} is ready for delivery review",
                                completion.revision
                            ),
                            "wait for the planner to request an independent reviewer".to_string(),
                            /* detail */ None,
                        )
                        .await
                    {
                        tracing::warn!(
                            agent_id = %subagent.id,
                            completion_id = %completion.id,
                            completion_revision = completion.revision,
                            error_bytes = error.to_string().len(),
                            "completion was committed but its directory progress projection failed"
                        );
                    }
                    let mut output =
                        ToolExecutionResult::<serde_json::Value>::json(completion)
                            .map_err(anyhow::Error::from)?;
                    output.ends_turn = true;
                    Ok::<_, anyhow::Error>(output)
                }
            })
        .with_effect(ToolEffect::BranchControl)
    }

    async fn report_completion(
        &self,
        subagent: &SubagentContext,
        caller_workspace: &Path,
        result: CompletionResultInput,
    ) -> Result<WorkCompletionRecord> {
        let canonical_caller = git_compatible_path(
            std::fs::canonicalize(caller_workspace)
                .context("failed to resolve caller workspace path")?,
        );
        let scope = self
            .store
            .resolve_active_completion_scope(&subagent.id, &canonical_caller.to_string_lossy())
            .await?
            .context("active completion scope not found for this executor worktree")?;
        ensure_completion_scope_is_open(&scope)?;
        let (_, handoff) = self
            .store
            .read_work_unit_handoff(&scope.work_unit.id)
            .await?
            .context("Task executor handoff is missing")?;
        handoff.validate_owner(&scope.run, &scope.work_unit, &subagent.id)?;
        match ValidatedCompletionResultInput::try_from(result)? {
            ValidatedCompletionResultInput::Delivery {
                head_commit,
                changed_files,
                verification_results,
            } => {
                let verification_summary =
                    validated_verification_summary(&handoff.blueprint, verification_results)?;
                let delivery = self.validate_delivery(
                    CompletionValidation {
                        scope: &scope,
                        subagent,
                        caller_workspace,
                        verification_summary: &verification_summary,
                    },
                    &head_commit,
                    changed_files,
                )?;
                self.store
                    .create_work_completion(
                        &scope.work_unit.id,
                        WorkCompletionContent::delivery(
                            delivery.head_commit.clone(),
                            delivery.changed_files.clone(),
                        )
                        .context("validated delivery has an empty head commit")?,
                        delivery.verification_summary.as_str(),
                    )
                    .await
            }
            ValidatedCompletionResultInput::NoDelivery {
                verification_results,
            } => {
                let verification_summary =
                    validated_verification_summary(&handoff.blueprint, verification_results)?;
                let verification_summary = validate_common(CompletionValidation {
                    scope: &scope,
                    subagent,
                    caller_workspace,
                    verification_summary: &verification_summary,
                })?;
                self.store
                    .create_work_completion(
                        &scope.work_unit.id,
                        WorkCompletionContent::no_delivery(),
                        verification_summary,
                    )
                    .await
            }
        }
    }

    fn validate_delivery(
        &self,
        validation: CompletionValidation<'_>,
        supplied_head: &str,
        changed_files: Vec<String>,
    ) -> Result<AgentDelivery> {
        let verification_summary = validate_common(validation)?;
        let supplied_head = supplied_head.trim();
        if supplied_head.is_empty() {
            bail!("headCommit must not be empty");
        }
        let base_commit = validation.scope.work_unit.base_commit.as_str();
        let changed_files = normalize_changed_files(changed_files)?;
        Ok(AgentDelivery {
            worktree: AgentWorktreeDelivery {
                path: validation.scope.work_unit.worktree_path.clone(),
                branch: validation.scope.work_unit.branch.clone(),
            },
            base_commit: base_commit.to_string(),
            head_commit: supplied_head.to_string(),
            changed_files,
            verification_summary: verification_summary.to_string(),
        })
    }
}

fn validate_common(validation: CompletionValidation<'_>) -> Result<&str> {
    let CompletionValidation {
        scope,
        subagent,
        caller_workspace,
        verification_summary,
    } = validation;
    if subagent.role != "executor" {
        bail!("report_completion may only be called by the assigned executor");
    }
    if !is_direct_task_child(subagent, &scope.run.root_thread_id)
        || scope.work_unit.task_run_id != scope.run.id
        || scope.work_unit.executor_thread_id.as_deref() != Some(subagent.id.as_str())
    {
        bail!("executor does not own this work unit");
    }
    let verification_summary = verification_summary.trim();
    if verification_summary.is_empty() {
        bail!("verificationSummary must not be empty");
    }
    if normalized_path(caller_workspace)
        != normalized_path(Path::new(&scope.work_unit.worktree_path))
    {
        bail!("caller workspace does not match the assigned executor worktree");
    }
    Ok(verification_summary)
}

fn normalize_changed_files(changed_files: Vec<String>) -> Result<Vec<String>> {
    if changed_files.is_empty() {
        bail!("delivery requires changedFiles")
    }
    let mut normalized = Vec::with_capacity(changed_files.len());
    for path in changed_files {
        let path = path.trim().replace('\\', "/");
        if path.is_empty()
            || path.starts_with('/')
            || path.contains(":/")
            || path
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
        {
            bail!("changedFiles must contain normalized repository-relative paths")
        }
        normalized.push(path);
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn is_direct_task_child(subagent: &SubagentContext, root_thread_id: &str) -> bool {
    let root_agent_id = crate::studio::agent_host::root_agent_id(root_thread_id);
    subagent.depth == 1 && subagent.parent_id.as_deref() == Some(root_agent_id.as_str())
}

fn ensure_completion_scope_is_open(scope: &DeliveryScope) -> Result<()> {
    if scope.work_unit.kind() != WorkUnitStateKind::Running {
        bail!("work unit is not accepting a completion");
    }
    Ok(())
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

fn validated_verification_summary(
    blueprint: &TaskExecutorBlueprint,
    mut results: Vec<VerificationResultInput>,
) -> Result<String> {
    for result in &mut results {
        result.check_id = result.check_id.trim().to_string();
        result.summary = result.summary.trim().to_string();
        if result.check_id.is_empty() || result.summary.is_empty() {
            bail!("verificationResults require non-empty checkId and summary")
        }
    }
    let by_id = verification_result_map(
        blueprint,
        results
            .iter()
            .map(|result| (result.check_id.as_str(), result.summary.clone())),
    )?;
    let lines = blueprint
        .verification_ids()
        .map(|id| {
            by_id
                .get(id)
                .map(|summary| format!("{id}: {summary}"))
                .context("validated verification result is missing")
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::deserialize_tool_input;

    #[test]
    fn completion_input_schema_is_a_flat_object() {
        let schema =
            FunctionToolDefinition::<CompletionResultInput>::new("report_completion", "test")
                .input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema.get("oneOf").is_none());
        let properties = schema["properties"].as_object().expect("input properties");
        assert!(properties.contains_key("kind"));
        assert!(properties.contains_key("headCommit"));
        assert!(properties.contains_key("changedFiles"));
        assert!(properties.contains_key("verificationResults"));
        assert!(!properties.contains_key("result"));
        let required = schema["required"].as_array().expect("required fields");
        assert!(required.iter().any(|field| field == "kind"));
        assert!(required.iter().any(|field| field == "verificationResults"));
        assert!(required.iter().any(|field| field == "changedFiles"));
        assert!(!required.iter().any(|field| field == "headCommit"));
    }

    #[test]
    fn completion_input_deserializes_top_level_scalar_fields() {
        let delivery = deserialize_tool_input::<CompletionResultInput>(
            "report_completion",
            serde_json::json!({
                "kind": "delivery",
                "headCommit": "0123456789abcdef",
                "changedFiles": ["src/lib.rs"],
                "verificationResults": [{"checkId": "check-1", "summary": "tests passed"}]
            }),
        )
        .expect("flat delivery input");
        assert_eq!(
            delivery,
            CompletionResultInput {
                kind: CompletionResultKindInput::Delivery,
                head_commit: Some("0123456789abcdef".to_string()),
                changed_files: vec!["src/lib.rs".to_string()],
                verification_results: vec![VerificationResultInput {
                    check_id: "check-1".to_string(),
                    summary: "tests passed".to_string(),
                }],
            }
        );

        let no_delivery = deserialize_tool_input::<CompletionResultInput>(
            "report_completion",
            serde_json::json!({
                "kind": "noDelivery",
                "changedFiles": [],
                "verificationResults": [{
                    "checkId": "check-1",
                    "summary": "no repository change required"
                }]
            }),
        )
        .expect("flat no-delivery input");
        assert_eq!(
            no_delivery,
            CompletionResultInput {
                kind: CompletionResultKindInput::NoDelivery,
                head_commit: None,
                changed_files: Vec::new(),
                verification_results: vec![VerificationResultInput {
                    check_id: "check-1".to_string(),
                    summary: "no repository change required".to_string(),
                }],
            }
        );
    }

    #[test]
    fn completion_input_rejects_invalid_head_commit_combinations() {
        let missing_delivery_head =
            ValidatedCompletionResultInput::try_from(CompletionResultInput {
                kind: CompletionResultKindInput::Delivery,
                head_commit: None,
                changed_files: vec!["src/lib.rs".to_string()],
                verification_results: vec![],
            })
            .expect_err("delivery without headCommit must fail");
        assert_eq!(
            missing_delivery_head.to_string(),
            "delivery requires headCommit"
        );

        let unexpected_no_delivery_head =
            ValidatedCompletionResultInput::try_from(CompletionResultInput {
                kind: CompletionResultKindInput::NoDelivery,
                head_commit: Some("0123456789abcdef".to_string()),
                changed_files: Vec::new(),
                verification_results: vec![],
            })
            .expect_err("noDelivery with headCommit must fail");
        assert_eq!(
            unexpected_no_delivery_head.to_string(),
            "noDelivery must not include headCommit"
        );
    }

    #[test]
    fn completion_verification_results_are_exact_and_stably_ordered() {
        let blueprint: TaskExecutorBlueprint = serde_json::from_value(serde_json::json!({
            "taskName": "implement transport",
            "objective": "use one canonical transport",
            "scope": {
                "inScope": ["model routing"],
                "outOfScope": [],
                "scopeHints": ["code/pl-model"]
            },
            "implementationSteps": [{
                "id": "step-1",
                "instruction": "update routing",
                "targets": [{"path": "code/pl-model/src/lib.rs"}],
                "expectedOutcome": "one canonical route",
                "criterionIds": ["criterion-1"]
            }],
            "acceptanceCriteria": [{
                "id": "criterion-1",
                "requirement": "routing is canonical"
            }],
            "dependencies": [],
            "evidence": [],
            "verification": {
                "commands": [{
                    "id": "check-command",
                    "command": "cargo test -p pl-model",
                    "cwd": ".",
                    "purpose": "test routing",
                    "expectedOutcome": "tests pass",
                    "criterionIds": ["criterion-1"]
                }],
                "inspections": [{
                    "id": "check-inspection",
                    "instruction": "inspect the final routing table",
                    "targets": [{"path": "code/pl-model/src/lib.rs"}],
                    "expectedOutcome": "one canonical route remains",
                    "criterionIds": ["criterion-1"]
                }]
            }
        }))
        .unwrap();
        let blueprint = blueprint.normalize_and_validate().unwrap();
        let summary = validated_verification_summary(
            &blueprint,
            vec![
                VerificationResultInput {
                    check_id: "check-inspection".to_string(),
                    summary: "confirmed".to_string(),
                },
                VerificationResultInput {
                    check_id: "check-command".to_string(),
                    summary: "passed".to_string(),
                },
            ],
        )
        .unwrap();
        assert_eq!(
            summary,
            "check-command: passed\ncheck-inspection: confirmed"
        );

        for (results, expected) in [
            (
                vec![VerificationResultInput {
                    check_id: "check-command".to_string(),
                    summary: "passed".to_string(),
                }],
                "missing checks: check-inspection",
            ),
            (
                vec![
                    VerificationResultInput {
                        check_id: "check-command".to_string(),
                        summary: "passed".to_string(),
                    },
                    VerificationResultInput {
                        check_id: "check-command".to_string(),
                        summary: "passed twice".to_string(),
                    },
                ],
                "repeats check `check-command`",
            ),
            (
                vec![VerificationResultInput {
                    check_id: "unknown".to_string(),
                    summary: "passed".to_string(),
                }],
                "unknown check `unknown`",
            ),
        ] {
            assert!(
                validated_verification_summary(&blueprint, results)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
    }
}
