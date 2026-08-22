use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{TaskRun, WorkUnit};
use super::normalize_scope_hints;

pub(crate) const TASK_EXECUTOR_HANDOFF_SECTION_ID: &str = "studio.task_executor_handoff";
const TASK_EXECUTOR_HANDOFF_VERSION: u32 = 4;
const MAX_EXECUTOR_BLUEPRINT_BYTES: usize = 20 * 1024;

/// Planner 可以随 executor allocation 一起提交的结构化依赖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskExecutorDependency {
    pub(crate) kind: String,
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) note: Option<String>,
}

/// 已完成探索的稳定定位证据；正文留在原文件或 child transcript。
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
pub(crate) struct TaskExecutorScope {
    pub(crate) in_scope: Vec<String>,
    pub(crate) out_of_scope: Vec<String>,
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

impl TaskExecutorBlueprint {
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

impl TaskExecutorHandoff {
    pub(crate) fn new(
        run: &TaskRun,
        work_unit: &WorkUnit,
        parent_thread_id: String,
        blueprint: TaskExecutorBlueprint,
    ) -> Result<Self> {
        run.design()
            .context("Task executor allocation requires a finalized design stage")?;
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
                content: run.plan.clone(),
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

fn normalize_required(value: &str, field: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("{field} must not be empty")
    }
    Ok(value.to_string())
}

fn normalize_identifier(value: &str, field: &str) -> Result<String> {
    let value = normalize_required(value, field)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("{field} `{value}` must contain only letters, digits, '-' or '_'")
    }
    Ok(value)
}

fn normalize_required_list(
    values: Vec<String>,
    field: &str,
    require_non_empty: bool,
) -> Result<Vec<String>> {
    if require_non_empty && values.is_empty() {
        bail!("{field} must not be empty")
    }
    values
        .into_iter()
        .map(|value| normalize_required(&value, field))
        .collect()
}

fn register_id(ids: &mut BTreeSet<String>, id: &str) -> Result<()> {
    if !ids.insert(id.to_string()) {
        bail!("duplicate executor blueprint id `{id}`")
    }
    Ok(())
}

fn normalize_targets(targets: &mut [TaskExecutorTarget], field: &str) -> Result<()> {
    if targets.is_empty() {
        bail!("{field} must not be empty")
    }
    for target in targets {
        target.path = normalize_scope_hints(std::slice::from_ref(&target.path))?
            .into_iter()
            .next()
            .context("executor target path is missing")?;
        target.symbol = target
            .symbol
            .take()
            .map(|symbol| normalize_required(&symbol, "target.symbol"))
            .transpose()?;
    }
    Ok(())
}

fn normalize_references(
    references: &mut [String],
    valid: &BTreeSet<String>,
    field: &str,
) -> Result<()> {
    if references.is_empty() {
        bail!("{field} must not be empty")
    }
    let mut seen = BTreeSet::new();
    for reference in references.iter_mut() {
        *reference = normalize_identifier(reference, field)?;
        if !valid.contains(reference) {
            bail!("{field} references unknown acceptance criterion `{reference}`")
        }
        if !seen.insert(reference.clone()) {
            bail!("{field} contains duplicate reference `{reference}`")
        }
    }
    Ok(())
}

fn require_full_coverage(
    expected: &BTreeSet<String>,
    actual: &BTreeSet<String>,
    source: &str,
) -> Result<()> {
    let missing = expected.difference(actual).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "acceptance criteria missing {source} coverage: {}",
            missing.join(", ")
        )
    }
    Ok(())
}

fn normalize_cwd(value: &str) -> Result<String> {
    let cwd = value.trim();
    if cwd == "." {
        return Ok(cwd.to_string());
    }
    normalize_scope_hints(&[cwd.to_string()])?
        .into_iter()
        .next()
        .context("verification command cwd is missing")
}

fn normalize_dependencies(dependencies: &mut [TaskExecutorDependency]) -> Result<()> {
    for dependency in dependencies {
        dependency.kind = normalize_required(&dependency.kind, "dependencies.kind")?;
        dependency.id = normalize_required(&dependency.id, "dependencies.id")?;
        dependency.note = dependency
            .note
            .take()
            .map(|note| normalize_required(&note, "dependencies.note"))
            .transpose()?;
    }
    Ok(())
}

fn normalize_evidence(evidence: &mut [TaskExecutorEvidence]) -> Result<()> {
    for item in evidence {
        item.path = normalize_scope_hints(std::slice::from_ref(&item.path))?
            .into_iter()
            .next()
            .context("executor evidence path is missing")?;
        item.symbol = item
            .symbol
            .take()
            .map(|symbol| normalize_required(&symbol, "evidence.symbol"))
            .transpose()?;
        item.content_hash = item
            .content_hash
            .take()
            .map(|hash| normalize_required(&hash, "evidence.contentHash"))
            .transpose()?;
        item.note = item
            .note
            .take()
            .map(|note| normalize_required(&note, "evidence.note"))
            .transpose()?;
    }
    Ok(())
}

pub(crate) fn verification_result_map<'a, T>(
    blueprint: &TaskExecutorBlueprint,
    results: impl IntoIterator<Item = (&'a str, T)>,
) -> Result<BTreeMap<String, T>> {
    let expected = blueprint.verification_ids().collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    for (id, value) in results {
        if !expected.contains(id) {
            bail!("verification result references unknown check `{id}`")
        }
        if actual.insert(id.to_string(), value).is_some() {
            bail!("verification result repeats check `{id}`")
        }
    }
    let missing = expected
        .into_iter()
        .filter(|id| !actual.contains_key(*id))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "verification results are missing checks: {}",
            missing.join(", ")
        )
    }
    Ok(actual)
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
