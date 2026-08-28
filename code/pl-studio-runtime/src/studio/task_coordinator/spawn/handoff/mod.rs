//! Task executor 的 durable、版本化交接事实与上下文载荷。
//!
//! 职责划分:
//! - 本文件:`TaskExecutorHandoff` 的 ownership/repository/plan/delivery 载荷、
//!   上下文 section 编解码与 owner 校验;
//! - `blueprint`:planner 提交的实施蓝图类型、规范化校验与指纹;
//! - `validation`:蓝图字段规范化 helper 与验证结果映射。

mod blueprint;
mod validation;

pub(crate) use blueprint::*;
pub(crate) use validation::verification_result_map;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::super::{TaskRun, TaskRunStateKind, WorkUnit};

pub(crate) const TASK_EXECUTOR_HANDOFF_SECTION_ID: &str = "studio.task_executor_handoff";
const TASK_EXECUTOR_HANDOFF_VERSION: u32 = 4;
const MAX_EXECUTOR_BLUEPRINT_BYTES: usize = 20 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorOwnership {
    pub(crate) task_run_id: String,
    pub(crate) work_unit_id: String,
    pub(crate) executor_agent_id: String,
    pub(crate) parent_thread_id: String,
    pub(crate) requesting_call_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorRepositoryFacts {
    pub(crate) base_commit: String,
    pub(crate) worktree_path: String,
    pub(crate) branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorConfirmedPlan {
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorDeliveryContract {
    pub(crate) completion_tool: String,
    pub(crate) require_commit: bool,
    pub(crate) require_verification_results: bool,
}

/// Task executor 的 durable、版本化交接事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorHandoff {
    pub(crate) version: u32,
    pub(crate) blueprint_fingerprint: String,
    pub(crate) ownership: TaskExecutorOwnership,
    pub(crate) repository: TaskExecutorRepositoryFacts,
    pub(crate) confirmed_plan: TaskExecutorConfirmedPlan,
    pub(crate) blueprint: TaskExecutorBlueprint,
    pub(crate) delivery: TaskExecutorDeliveryContract,
}

impl TaskExecutorHandoff {
    pub(crate) fn new(
        run: &TaskRun,
        work_unit: &WorkUnit,
        parent_thread_id: String,
        blueprint: TaskExecutorBlueprint,
    ) -> Result<Self> {
        anyhow::ensure!(
            run.kind() == TaskRunStateKind::Working,
            "Task executor allocation requires working state"
        );
        let blueprint_fingerprint = blueprint.fingerprint()?;
        Ok(Self {
            version: TASK_EXECUTOR_HANDOFF_VERSION,
            blueprint_fingerprint,
            ownership: TaskExecutorOwnership {
                task_run_id: run.id.clone(),
                work_unit_id: work_unit.id.clone(),
                executor_agent_id: work_unit.executor_thread_id.clone().unwrap_or_default(),
                parent_thread_id,
                requesting_call_id: work_unit.requested_by_call_id.clone(),
            },
            repository: TaskExecutorRepositoryFacts {
                base_commit: work_unit.base_commit.clone(),
                worktree_path: work_unit.worktree_path.clone(),
                branch: work_unit.branch.clone(),
            },
            confirmed_plan: TaskExecutorConfirmedPlan {
                content: run
                    .plan_content()
                    .context("Task executor allocation requires a frozen plan")?
                    .to_string(),
            },
            blueprint,
            delivery: TaskExecutorDeliveryContract {
                completion_tool: "report_completion".to_string(),
                require_commit: true,
                require_verification_results: true,
            },
        })
    }

    pub(crate) fn to_context_section(&self) -> Result<pl_core::PinnedContextSection> {
        let content = serde_json::to_string_pretty(self)
            .context("failed to serialize Task executor handoff")?;
        pl_core::context_section(
            TASK_EXECUTOR_HANDOFF_SECTION_ID,
            u64::from(self.version),
            "Task executor handoff",
            content,
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub(crate) fn from_context_section(section: &pl_core::PinnedContextSection) -> Result<Self> {
        if section.id.as_str() != TASK_EXECUTOR_HANDOFF_SECTION_ID {
            bail!("unexpected Task executor handoff section id")
        }
        let value: serde_json::Value = serde_json::from_str(&section.content)
            .context("invalid Task executor handoff payload")?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .context("Task executor handoff omitted its version")?;
        if version != u64::from(TASK_EXECUTOR_HANDOFF_VERSION) {
            bail!("unsupported Task executor handoff version {version}")
        }
        let handoff: Self =
            serde_json::from_value(value).context("invalid Task executor handoff payload")?;
        if handoff.blueprint_fingerprint != handoff.blueprint.fingerprint()? {
            bail!("Task executor handoff blueprint fingerprint does not match its content")
        }
        Ok(handoff)
    }

    pub(crate) fn validate_owner(
        &self,
        run: &TaskRun,
        work_unit: &WorkUnit,
        executor_agent_id: &str,
    ) -> Result<()> {
        if self.ownership.task_run_id != run.id
            || self.ownership.work_unit_id != work_unit.id
            || self.ownership.executor_agent_id != executor_agent_id
            || self.ownership.requesting_call_id != work_unit.requested_by_call_id
            || self.ownership.parent_thread_id != run.root_thread_id
            || self.repository.base_commit != work_unit.base_commit
            || self.repository.worktree_path != work_unit.worktree_path
            || self.repository.branch != work_unit.branch
        {
            bail!("Task executor handoff does not match its durable owner")
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_blueprint() -> TaskExecutorBlueprint {
        TaskExecutorBlueprint {
            task_name: " implement transport ".to_string(),
            objective: " use one canonical transport ".to_string(),
            scope: TaskExecutorScope {
                in_scope: vec!["model routing".to_string()],
                out_of_scope: Vec::new(),
                scope_hints: vec!["code\\pl-model".to_string()],
            },
            implementation_steps: vec![TaskExecutorImplementationStep {
                id: "step-1".to_string(),
                instruction: " update routing ".to_string(),
                targets: vec![TaskExecutorTarget {
                    path: "code/pl-model/src/lib.rs".to_string(),
                    symbol: Some(" route ".to_string()),
                }],
                expected_outcome: " one route ".to_string(),
                criterion_ids: vec!["criterion-1".to_string()],
            }],
            acceptance_criteria: vec![TaskExecutorAcceptanceCriterion {
                id: "criterion-1".to_string(),
                requirement: " routing is canonical ".to_string(),
            }],
            dependencies: Vec::new(),
            evidence: Vec::new(),
            verification: TaskExecutorVerificationContract {
                commands: vec![TaskExecutorVerificationCommand {
                    id: "check-1".to_string(),
                    command: " cargo test -p pl-model ".to_string(),
                    cwd: ".".to_string(),
                    purpose: " test routing ".to_string(),
                    expected_outcome: " tests pass ".to_string(),
                    criterion_ids: vec!["criterion-1".to_string()],
                }],
                inspections: Vec::new(),
            },
        }
    }

    fn valid_handoff() -> TaskExecutorHandoff {
        let blueprint = valid_blueprint().normalize_and_validate().unwrap();
        TaskExecutorHandoff {
            version: TASK_EXECUTOR_HANDOFF_VERSION,
            blueprint_fingerprint: blueprint.fingerprint().unwrap(),
            ownership: TaskExecutorOwnership {
                task_run_id: "task-1".to_string(),
                work_unit_id: "work-1".to_string(),
                executor_agent_id: "executor-1".to_string(),
                parent_thread_id: "thread-1".to_string(),
                requesting_call_id: "call-1".to_string(),
            },
            repository: TaskExecutorRepositoryFacts {
                base_commit: "base".to_string(),
                worktree_path: "/tmp/worktree".to_string(),
                branch: "task-work-1".to_string(),
            },
            confirmed_plan: TaskExecutorConfirmedPlan {
                content: "confirmed plan".to_string(),
            },
            blueprint,
            delivery: TaskExecutorDeliveryContract {
                completion_tool: "report_completion".to_string(),
                require_commit: true,
                require_verification_results: true,
            },
        }
    }

    #[test]
    fn blueprint_requires_concrete_steps_and_complete_criterion_coverage() {
        let normalized = valid_blueprint().normalize_and_validate().unwrap();
        assert_eq!(normalized.task_name, "implement transport");
        assert_eq!(normalized.scope.scope_hints, vec!["code/pl-model"]);
        assert_eq!(
            normalized.implementation_steps[0].targets[0]
                .symbol
                .as_deref(),
            Some("route")
        );

        let mut vague = valid_blueprint();
        vague.implementation_steps.clear();
        assert!(vague.normalize_and_validate().is_err());

        let mut uncovered = valid_blueprint();
        uncovered.verification.commands[0].criterion_ids = vec!["unknown".to_string()];
        assert!(uncovered.normalize_and_validate().is_err());
    }

    #[test]
    fn blueprint_rejects_invalid_paths_ids_references_and_context_size() {
        let mut invalid_path = valid_blueprint();
        invalid_path.implementation_steps[0].targets[0].path = "../outside".to_string();
        assert!(invalid_path.normalize_and_validate().is_err());

        let mut invalid_cwd = valid_blueprint();
        invalid_cwd.verification.commands[0].cwd = "/tmp".to_string();
        assert!(invalid_cwd.normalize_and_validate().is_err());

        let mut prose_scope_hint = valid_blueprint();
        prose_scope_hint.scope.scope_hints =
            vec!["code/pl-model is the only allowed implementation area".to_string()];
        assert!(
            prose_scope_hint
                .normalize_and_validate()
                .unwrap_err()
                .to_string()
                .contains("not prose")
        );

        let mut duplicate_id = valid_blueprint();
        duplicate_id.implementation_steps[0].id = "criterion-1".to_string();
        assert!(duplicate_id.normalize_and_validate().is_err());

        let mut duplicate_reference = valid_blueprint();
        duplicate_reference.implementation_steps[0]
            .criterion_ids
            .push("criterion-1".to_string());
        assert!(duplicate_reference.normalize_and_validate().is_err());

        let mut no_command = valid_blueprint();
        no_command.verification.commands.clear();
        assert!(no_command.normalize_and_validate().is_err());

        let mut oversized = valid_blueprint();
        oversized.objective = "x".repeat(MAX_EXECUTOR_BLUEPRINT_BYTES);
        assert!(
            oversized
                .normalize_and_validate()
                .unwrap_err()
                .to_string()
                .contains("context budget")
        );
    }

    #[test]
    fn blueprint_fingerprint_changes_with_steps_acceptance_or_verification() {
        let blueprint = valid_blueprint().normalize_and_validate().unwrap();
        let fingerprint = blueprint.fingerprint().unwrap();
        for changed in [
            {
                let mut changed = blueprint.clone();
                changed.implementation_steps[0]
                    .instruction
                    .push_str(" safely");
                changed
            },
            {
                let mut changed = blueprint.clone();
                changed.acceptance_criteria[0]
                    .requirement
                    .push_str(" always");
                changed
            },
            {
                let mut changed = blueprint.clone();
                changed.verification.commands[0]
                    .expected_outcome
                    .push_str(" cleanly");
                changed
            },
        ] {
            assert_ne!(changed.fingerprint().unwrap(), fingerprint);
        }
    }

    #[test]
    fn handoff_rejects_v1_and_unknown_fields() {
        let handoff = valid_handoff();
        let mut value = serde_json::to_value(&handoff).unwrap();
        value["version"] = serde_json::json!(1);
        let section = pl_core::context_section(
            TASK_EXECUTOR_HANDOFF_SECTION_ID,
            1,
            "Task executor handoff",
            serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
        assert!(
            TaskExecutorHandoff::from_context_section(&section)
                .unwrap_err()
                .to_string()
                .contains("unsupported Task executor handoff version 1")
        );

        let mut value = serde_json::to_value(&handoff).unwrap();
        value["legacyMessage"] = serde_json::json!("drifting duplicate");
        let section = pl_core::context_section(
            TASK_EXECUTOR_HANDOFF_SECTION_ID,
            u64::from(TASK_EXECUTOR_HANDOFF_VERSION),
            "Task executor handoff",
            serde_json::to_string(&value).unwrap(),
        )
        .unwrap();
        assert!(TaskExecutorHandoff::from_context_section(&section).is_err());
    }

    #[test]
    fn verification_results_must_cover_each_command_and_inspection_exactly_once() {
        let mut blueprint = valid_blueprint();
        blueprint.verification.inspections = vec![TaskExecutorVerificationInspection {
            id: "inspect-1".to_string(),
            instruction: "inspect the routing table".to_string(),
            targets: vec![TaskExecutorTarget {
                path: "code/pl-model/src/lib.rs".to_string(),
                symbol: Some("route".to_string()),
            }],
            expected_outcome: "there is one canonical route".to_string(),
            criterion_ids: vec!["criterion-1".to_string()],
        }];
        let blueprint = blueprint.normalize_and_validate().unwrap();

        let complete = verification_result_map(
            &blueprint,
            [("check-1", "passed"), ("inspect-1", "confirmed")],
        )
        .unwrap();
        assert_eq!(complete.len(), 2);
        assert!(
            verification_result_map(&blueprint, [("check-1", "passed")])
                .unwrap_err()
                .to_string()
                .contains("missing checks: inspect-1")
        );
        assert!(
            verification_result_map(
                &blueprint,
                [
                    ("check-1", "passed"),
                    ("check-1", "passed twice"),
                    ("inspect-1", "confirmed"),
                ],
            )
            .unwrap_err()
            .to_string()
            .contains("repeats check `check-1`")
        );
        assert!(
            verification_result_map(
                &blueprint,
                [
                    ("check-1", "passed"),
                    ("inspect-1", "confirmed"),
                    ("unknown", "not in handoff"),
                ],
            )
            .unwrap_err()
            .to_string()
            .contains("unknown check `unknown`")
        );
    }
}
