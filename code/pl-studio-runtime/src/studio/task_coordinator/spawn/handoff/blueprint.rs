//! Planner 提交的实施蓝图类型、规范化校验与指纹。

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::normalize_scope_hints;
use super::MAX_EXECUTOR_BLUEPRINT_BYTES;
use super::validation::*;

/// Planner 可以随 executor allocation 一起提交的结构化依赖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorDependency {
    pub(crate) kind: String,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// 已完成探索的稳定定位证据;正文留在原文件或 child transcript。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorEvidence {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) line: Option<u32>,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
    #[serde(default)]
    pub(crate) content_hash: Option<String>,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// 实施或只读检查预计触及的仓库位置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorTarget {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) symbol: Option<String>,
}

/// 一个 WorkUnit 的范围说明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(inline)]
pub(crate) struct TaskExecutorScope {
    pub(crate) in_scope: Vec<String>,
    pub(crate) out_of_scope: Vec<String>,
    /// Normalized repository-relative path prefixes covered by implementation targets.
    /// This field is structural conflict metadata, not free-form prose.
    pub(crate) scope_hints: Vec<String>,
}

/// 可按顺序执行并独立核验的实施步骤。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorImplementationStep {
    pub(crate) id: String,
    pub(crate) instruction: String,
    pub(crate) targets: Vec<TaskExecutorTarget>,
    pub(crate) expected_outcome: String,
    pub(crate) criterion_ids: Vec<String>,
}

/// 蓝图必须满足的单条验收条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorAcceptanceCriterion {
    pub(crate) id: String,
    pub(crate) requirement: String,
}

/// Planner 在 allocation 前核对的单条项目验证命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorVerificationCommand {
    pub(crate) id: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) purpose: String,
    pub(crate) expected_outcome: String,
    pub(crate) criterion_ids: Vec<String>,
}

/// 不依赖 shell 命令的显式验收检查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorVerificationInspection {
    pub(crate) id: String,
    pub(crate) instruction: String,
    pub(crate) targets: Vec<TaskExecutorTarget>,
    pub(crate) expected_outcome: String,
    pub(crate) criterion_ids: Vec<String>,
}

/// Fresh executor 不依赖 planner transcript 也能恢复的验证契约。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(inline)]
pub(crate) struct TaskExecutorVerificationContract {
    pub(crate) commands: Vec<TaskExecutorVerificationCommand>,
    pub(crate) inspections: Vec<TaskExecutorVerificationInspection>,
}

/// Planner 为一个 WorkUnit 提交的完整、自包含实施蓝图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorBlueprint {
    pub(crate) task_name: String,
    pub(crate) objective: String,
    pub(crate) scope: TaskExecutorScope,
    pub(crate) implementation_steps: Vec<TaskExecutorImplementationStep>,
    pub(crate) acceptance_criteria: Vec<TaskExecutorAcceptanceCriterion>,
    pub(crate) dependencies: Vec<TaskExecutorDependency>,
    pub(crate) evidence: Vec<TaskExecutorEvidence>,
    pub(crate) verification: TaskExecutorVerificationContract,
}

impl TaskExecutorBlueprint {
    #[cfg(test)]
    pub(crate) fn for_test(task_name: &str, scope_hints: Vec<String>) -> Self {
        let target = scope_hints
            .first()
            .cloned()
            .unwrap_or_else(|| "src".to_string());
        Self {
            task_name: task_name.to_string(),
            objective: format!("Deliver {task_name}"),
            scope: TaskExecutorScope {
                in_scope: vec![target.clone()],
                out_of_scope: Vec::new(),
                scope_hints,
            },
            implementation_steps: vec![TaskExecutorImplementationStep {
                id: "step-1".to_string(),
                instruction: format!("Implement {task_name}"),
                targets: vec![TaskExecutorTarget {
                    path: target,
                    symbol: None,
                }],
                expected_outcome: "The requested behavior is implemented".to_string(),
                criterion_ids: vec!["criterion-1".to_string()],
            }],
            acceptance_criteria: vec![TaskExecutorAcceptanceCriterion {
                id: "criterion-1".to_string(),
                requirement: "The implementation passes its focused tests".to_string(),
            }],
            dependencies: Vec::new(),
            evidence: Vec::new(),
            verification: TaskExecutorVerificationContract {
                commands: vec![TaskExecutorVerificationCommand {
                    id: "verify-1".to_string(),
                    command: "cargo test".to_string(),
                    cwd: ".".to_string(),
                    purpose: "Run the focused test suite".to_string(),
                    expected_outcome: "Tests pass".to_string(),
                    criterion_ids: vec!["criterion-1".to_string()],
                }],
                inspections: Vec::new(),
            },
        }
    }

    pub(crate) fn normalize_and_validate(mut self) -> Result<Self> {
        self.task_name = normalize_required(&self.task_name, "taskName")?;
        self.objective = normalize_required(&self.objective, "objective")?;
        self.scope.in_scope = normalize_required_list(self.scope.in_scope, "scope.inScope", true)?;
        self.scope.out_of_scope =
            normalize_required_list(self.scope.out_of_scope, "scope.outOfScope", false)?;
        self.scope.scope_hints = normalize_scope_hints(&self.scope.scope_hints)?;
        if self.scope.scope_hints.is_empty() {
            bail!("scope.scopeHints must not be empty")
        }
        if self.implementation_steps.is_empty() {
            bail!("implementationSteps must not be empty")
        }
        if self.acceptance_criteria.is_empty() {
            bail!("acceptanceCriteria must not be empty")
        }
        if self.verification.commands.is_empty() {
            bail!("verification.commands must not be empty")
        }

        let mut all_ids = BTreeSet::new();
        let mut criterion_ids = BTreeSet::new();
        for criterion in &mut self.acceptance_criteria {
            criterion.id = normalize_identifier(&criterion.id, "acceptanceCriteria.id")?;
            criterion.requirement =
                normalize_required(&criterion.requirement, "acceptanceCriteria.requirement")?;
            register_id(&mut all_ids, &criterion.id)?;
            criterion_ids.insert(criterion.id.clone());
        }

        let mut step_coverage = BTreeSet::new();
        for step in &mut self.implementation_steps {
            step.id = normalize_identifier(&step.id, "implementationSteps.id")?;
            register_id(&mut all_ids, &step.id)?;
            step.instruction =
                normalize_required(&step.instruction, "implementationSteps.instruction")?;
            step.expected_outcome = normalize_required(
                &step.expected_outcome,
                "implementationSteps.expectedOutcome",
            )?;
            normalize_targets(&mut step.targets, "implementationSteps.targets")?;
            normalize_references(
                &mut step.criterion_ids,
                &criterion_ids,
                "implementationSteps.criterionIds",
            )?;
            step_coverage.extend(step.criterion_ids.iter().cloned());
        }
        for hint in &self.scope.scope_hints {
            if !self
                .implementation_steps
                .iter()
                .flat_map(|step| &step.targets)
                .any(|target| scope_hint_covers_target(hint, &target.path))
            {
                bail!(
                    "scope.scopeHints entry `{hint}` does not cover any implementation target; use repository-relative path prefixes, not prose"
                )
            }
        }

        let mut verification_coverage = BTreeSet::new();
        for command in &mut self.verification.commands {
            command.id = normalize_identifier(&command.id, "verification.commands.id")?;
            register_id(&mut all_ids, &command.id)?;
            command.command =
                normalize_required(&command.command, "verification.commands.command")?;
            command.purpose =
                normalize_required(&command.purpose, "verification.commands.purpose")?;
            command.expected_outcome = normalize_required(
                &command.expected_outcome,
                "verification.commands.expectedOutcome",
            )?;
            command.cwd = normalize_cwd(&command.cwd)?;
            normalize_references(
                &mut command.criterion_ids,
                &criterion_ids,
                "verification.commands.criterionIds",
            )?;
            verification_coverage.extend(command.criterion_ids.iter().cloned());
        }
        for inspection in &mut self.verification.inspections {
            inspection.id = normalize_identifier(&inspection.id, "verification.inspections.id")?;
            register_id(&mut all_ids, &inspection.id)?;
            inspection.instruction = normalize_required(
                &inspection.instruction,
                "verification.inspections.instruction",
            )?;
            inspection.expected_outcome = normalize_required(
                &inspection.expected_outcome,
                "verification.inspections.expectedOutcome",
            )?;
            normalize_targets(&mut inspection.targets, "verification.inspections.targets")?;
            normalize_references(
                &mut inspection.criterion_ids,
                &criterion_ids,
                "verification.inspections.criterionIds",
            )?;
            verification_coverage.extend(inspection.criterion_ids.iter().cloned());
        }

        require_full_coverage(&criterion_ids, &step_coverage, "implementation steps")?;
        require_full_coverage(&criterion_ids, &verification_coverage, "verification")?;
        normalize_dependencies(&mut self.dependencies)?;
        normalize_evidence(&mut self.evidence)?;

        let serialized = serde_json::to_vec(&self)?;
        if serialized.len() > MAX_EXECUTOR_BLUEPRINT_BYTES {
            bail!(
                "executor blueprint exceeds the {MAX_EXECUTOR_BLUEPRINT_BYTES}-byte context budget"
            )
        }
        Ok(self)
    }

    pub(crate) fn fingerprint(&self) -> Result<String> {
        Ok(pl_core::canonical_json_hash(&serde_json::to_value(self)?))
    }

    pub(crate) fn verification_ids(&self) -> impl Iterator<Item = &str> {
        self.verification
            .commands
            .iter()
            .map(|check| check.id.as_str())
            .chain(
                self.verification
                    .inspections
                    .iter()
                    .map(|check| check.id.as_str()),
            )
    }

    pub(crate) fn verification_count(&self) -> usize {
        self.verification.commands.len() + self.verification.inspections.len()
    }
}

fn scope_hint_covers_target(hint: &str, target: &str) -> bool {
    target == hint
        || target
            .strip_prefix(hint)
            .is_some_and(|suffix| suffix.starts_with('/'))
}
