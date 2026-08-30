//! Mode Skill 驱动的通用工作流编译与模型投影。

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use pl_protocol::{
    ContextSectionId, ModelContextSectionSnapshot, PureError, WorkflowDefinition,
    WorkflowRunLifecycle, WorkflowSessionState, WorkflowStage,
};
use serde::Serialize;

use crate::{WORKFLOW_CONTEXT_SECTION_ID, canonical_content_hash, canonical_json_hash};

pub const MAX_WORKFLOW_STAGES: usize = 32;
pub const MAX_WORKFLOW_TRANSITIONS: usize = 96;
pub const MAX_WORKFLOW_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_WORKFLOW_STATE_BYTES: usize = 256 * 1024;
pub const MAX_WORKFLOW_HISTORY: usize = 64;
pub const MAX_ARCHIVED_WORKFLOW_RUNS: usize = 16;
pub const MAX_WORKFLOW_OPERATION_RECEIPTS: usize = 32;

const MAX_STAGE_ID_BYTES: usize = 64;
const MAX_STAGE_TITLE_CHARS: usize = 128;
const MAX_STAGE_INSTRUCTIONS_BYTES: usize = 4 * 1024;
const MAX_COMPLETION_CRITERIA: usize = 16;
const MAX_COMPLETION_CRITERION_BYTES: usize = 512;

/// 编译后可直接冻结进 run 的规范化定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorkflowDefinition {
    pub definition: WorkflowDefinition,
    pub definition_hash: String,
}

/// 一条稳定、可供工具返回给模型的定义问题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowValidationIssue {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

/// 工作流定义未通过纯函数编译。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCompilerError {
    issues: Vec<WorkflowValidationIssue>,
}

impl WorkflowCompilerError {
    pub fn issues(&self) -> &[WorkflowValidationIssue] {
        &self.issues
    }
}

impl fmt::Display for WorkflowCompilerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "workflow definition has {} issue(s)",
            self.issues.len()
        )
    }
}

impl std::error::Error for WorkflowCompilerError {}

/// 编译并规范化 Mode Skill 给出的工作流图。
///
/// 检查顺序固定为 schema 边界、ID、端点、初态、终态、正向可达性和反向终态
/// 可达性。图检查复杂度为 `O(V + E)`。
pub fn compile_definition(
    definition: WorkflowDefinition,
) -> Result<CompiledWorkflowDefinition, WorkflowCompilerError> {
    let definition = normalize_definition(definition);
    let mut issues = Vec::new();
    validate_schema(&definition, &mut issues);
    validate_ids(&definition, &mut issues);

    let stages = definition
        .stages
        .iter()
        .map(|stage| (stage.id.as_str(), stage))
        .collect::<HashMap<_, _>>();
    validate_endpoints(&definition, &stages, &mut issues);
    validate_initial_stage(&definition, &stages, &mut issues);
    validate_terminal_stages(&definition, &stages, &mut issues);

    if issues.is_empty() {
        validate_reachability(&definition, &mut issues);
    }

    let canonical = serde_json::to_value(&definition).map_err(|error| WorkflowCompilerError {
        issues: vec![issue(
            "invalidSchema",
            "$",
            format!("definition cannot be serialized: {error}"),
        )],
    })?;
    let canonical_bytes =
        serde_json::to_vec(&canonical).map_err(|error| WorkflowCompilerError {
            issues: vec![issue(
                "invalidSchema",
                "$",
                format!("definition cannot be serialized: {error}"),
            )],
        })?;
    if canonical_bytes.len() > MAX_WORKFLOW_DEFINITION_BYTES {
        issues.push(issue(
            "definitionTooLarge",
            "$",
            format!("normalized definition exceeds {MAX_WORKFLOW_DEFINITION_BYTES} bytes"),
        ));
    }

    if !issues.is_empty() {
        return Err(WorkflowCompilerError { issues });
    }

    Ok(CompiledWorkflowDefinition {
        definition,
        definition_hash: canonical_json_hash(&canonical),
    })
}

/// 拒绝超过 canonical working-state 上限的状态。
pub fn validate_session_state_size(state: &WorkflowSessionState) -> Result<(), PureError> {
    let bytes = serde_json::to_vec(state)?;
    if bytes.len() > MAX_WORKFLOW_STATE_BYTES {
        return Err(PureError::ConfigError(format!(
            "workflow state exceeds {MAX_WORKFLOW_STATE_BYTES} bytes"
        )));
    }
    Ok(())
}

/// 从完整 typed state 派生供模型消费的精简 `pl.workflow` section。
pub fn model_context_section(state: &WorkflowSessionState) -> Option<ModelContextSectionSnapshot> {
    let run = state.current_run.as_ref()?;
    let stage = run
        .definition
        .stages
        .iter()
        .find(|stage| stage.id == run.current_stage_id)?;
    let allowed_transitions = run
        .definition
        .transitions
        .iter()
        .filter(|transition| transition.from_stage_id == stage.id)
        .map(|transition| WorkflowTransitionProjection {
            to_stage_id: &transition.to_stage_id,
            when: &transition.when,
        })
        .collect::<Vec<_>>();
    let projection = WorkflowContextProjection {
        run_id: &run.run_id,
        revision: state.revision,
        mode_id: &run.mode.mode_id,
        lifecycle: run.lifecycle,
        current_stage: stage,
        allowed_transitions,
        latest_completion_summary: run
            .history_tail
            .last()
            .map(|transition| transition.summary.as_str()),
        constraint: if run.lifecycle == WorkflowRunLifecycle::Active {
            "Follow the current stage instructions. When its completion criteria are satisfied, call workflow_state once to take a direct outgoing transition."
        } else {
            "The workflow is terminal. Do not mutate it; deliver or continue the current turn normally."
        },
    };
    let content = serde_json::to_string_pretty(&projection).ok()?;
    Some(ModelContextSectionSnapshot {
        id: ContextSectionId::new(WORKFLOW_CONTEXT_SECTION_ID)
            .expect("built-in workflow context id must be valid"),
        title: "Workflow".to_string(),
        content_hash: canonical_content_hash(content.as_bytes()),
        content,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowContextProjection<'a> {
    run_id: &'a str,
    revision: u64,
    mode_id: &'a str,
    lifecycle: WorkflowRunLifecycle,
    current_stage: &'a WorkflowStage,
    allowed_transitions: Vec<WorkflowTransitionProjection<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_completion_summary: Option<&'a str>,
    constraint: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowTransitionProjection<'a> {
    to_stage_id: &'a str,
    when: &'a str,
}

fn normalize_definition(mut definition: WorkflowDefinition) -> WorkflowDefinition {
    definition.title = definition.title.trim().to_string();
    definition.goal = definition.goal.trim().to_string();
    for stage in &mut definition.stages {
        stage.title = stage.title.trim().to_string();
        stage.instructions = stage.instructions.trim().to_string();
        for criterion in &mut stage.completion_criteria {
            *criterion = criterion.trim().to_string();
        }
    }
    for transition in &mut definition.transitions {
        transition.when = transition.when.trim().to_string();
    }
    definition
}

fn validate_schema(definition: &WorkflowDefinition, issues: &mut Vec<WorkflowValidationIssue>) {
    if definition.title.is_empty() {
        issues.push(issue("required", "title", "title must not be empty"));
    } else if definition.title.chars().count() > MAX_STAGE_TITLE_CHARS {
        issues.push(issue(
            "tooLong",
            "title",
            format!("title may contain at most {MAX_STAGE_TITLE_CHARS} characters"),
        ));
    }
    if definition.goal.is_empty() {
        issues.push(issue("required", "goal", "goal must not be empty"));
    }
    if definition.stages.is_empty() || definition.stages.len() > MAX_WORKFLOW_STAGES {
        issues.push(issue(
            "stageCount",
            "stages",
            format!("workflow must contain 1 to {MAX_WORKFLOW_STAGES} stages"),
        ));
    }
    if definition.transitions.len() > MAX_WORKFLOW_TRANSITIONS {
        issues.push(issue(
            "transitionCount",
            "transitions",
            format!("workflow may contain at most {MAX_WORKFLOW_TRANSITIONS} transitions"),
        ));
    }
    for (index, stage) in definition.stages.iter().enumerate() {
        let path = format!("stages[{index}]");
        if stage.title.is_empty() {
            issues.push(issue(
                "required",
                format!("{path}.title"),
                "title must not be empty",
            ));
        } else if stage.title.chars().count() > MAX_STAGE_TITLE_CHARS {
            issues.push(issue(
                "tooLong",
                format!("{path}.title"),
                format!("title may contain at most {MAX_STAGE_TITLE_CHARS} characters"),
            ));
        }
        if !stage.terminal && stage.instructions.is_empty() {
            issues.push(issue(
                "required",
                format!("{path}.instructions"),
                "a non-terminal stage must include instructions",
            ));
        } else if stage.instructions.len() > MAX_STAGE_INSTRUCTIONS_BYTES {
            issues.push(issue(
                "tooLarge",
                format!("{path}.instructions"),
                format!("instructions may contain at most {MAX_STAGE_INSTRUCTIONS_BYTES} bytes"),
            ));
        }
        if !stage.terminal && stage.completion_criteria.is_empty() {
            issues.push(issue(
                "required",
                format!("{path}.completionCriteria"),
                "a non-terminal stage must include completion criteria",
            ));
        }
        if stage.completion_criteria.len() > MAX_COMPLETION_CRITERIA {
            issues.push(issue(
                "tooMany",
                format!("{path}.completionCriteria"),
                format!("a stage may contain at most {MAX_COMPLETION_CRITERIA} criteria"),
            ));
        }
        for (criterion_index, criterion) in stage.completion_criteria.iter().enumerate() {
            if criterion.is_empty() {
                issues.push(issue(
                    "required",
                    format!("{path}.completionCriteria[{criterion_index}]"),
                    "completion criterion must not be empty",
                ));
            } else if criterion.len() > MAX_COMPLETION_CRITERION_BYTES {
                issues.push(issue(
                    "tooLarge",
                    format!("{path}.completionCriteria[{criterion_index}]"),
                    format!(
                        "completion criterion may contain at most {MAX_COMPLETION_CRITERION_BYTES} bytes"
                    ),
                ));
            }
        }
    }
    for (index, transition) in definition.transitions.iter().enumerate() {
        if transition.when.is_empty() {
            issues.push(issue(
                "required",
                format!("transitions[{index}].when"),
                "transition condition must not be empty",
            ));
        }
    }
}

fn validate_ids(definition: &WorkflowDefinition, issues: &mut Vec<WorkflowValidationIssue>) {
    validate_id("initialStageId", &definition.initial_stage_id, issues);
    let mut stage_ids = HashSet::new();
    for (index, stage) in definition.stages.iter().enumerate() {
        validate_id(format!("stages[{index}].id"), &stage.id, issues);
        if !stage_ids.insert(stage.id.as_str()) {
            issues.push(issue(
                "duplicateStageId",
                format!("stages[{index}].id"),
                format!("stage id `{}` is duplicated", stage.id),
            ));
        }
    }
    let mut edges = HashSet::new();
    for (index, transition) in definition.transitions.iter().enumerate() {
        validate_id(
            format!("transitions[{index}].fromStageId"),
            &transition.from_stage_id,
            issues,
        );
        validate_id(
            format!("transitions[{index}].toStageId"),
            &transition.to_stage_id,
            issues,
        );
        if !edges.insert((&transition.from_stage_id, &transition.to_stage_id)) {
            issues.push(issue(
                "duplicateTransition",
                format!("transitions[{index}]"),
                format!(
                    "transition `{}` -> `{}` is duplicated",
                    transition.from_stage_id, transition.to_stage_id
                ),
            ));
        }
    }
}

fn validate_id(path: impl Into<String>, id: &str, issues: &mut Vec<WorkflowValidationIssue>) {
    if id.is_empty()
        || id.len() > MAX_STAGE_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        issues.push(issue(
            "invalidId",
            path,
            format!(
                "`{id}` must be 1-{MAX_STAGE_ID_BYTES} lowercase ASCII letters, digits, `-`, or `_`"
            ),
        ));
    }
}

fn validate_endpoints(
    definition: &WorkflowDefinition,
    stages: &HashMap<&str, &WorkflowStage>,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    for (index, transition) in definition.transitions.iter().enumerate() {
        if !stages.contains_key(transition.from_stage_id.as_str()) {
            issues.push(issue(
                "unknownEndpoint",
                format!("transitions[{index}].fromStageId"),
                format!("unknown stage `{}`", transition.from_stage_id),
            ));
        }
        if !stages.contains_key(transition.to_stage_id.as_str()) {
            issues.push(issue(
                "unknownEndpoint",
                format!("transitions[{index}].toStageId"),
                format!("unknown stage `{}`", transition.to_stage_id),
            ));
        }
    }
}

fn validate_initial_stage(
    definition: &WorkflowDefinition,
    stages: &HashMap<&str, &WorkflowStage>,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    if !stages.contains_key(definition.initial_stage_id.as_str()) {
        issues.push(issue(
            "unknownInitialStage",
            "initialStageId",
            format!("unknown initial stage `{}`", definition.initial_stage_id),
        ));
    }
}

fn validate_terminal_stages(
    definition: &WorkflowDefinition,
    stages: &HashMap<&str, &WorkflowStage>,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    if !stages.values().any(|stage| stage.terminal) {
        issues.push(issue(
            "missingTerminalStage",
            "stages",
            "workflow must contain at least one terminal stage",
        ));
    }
    let outgoing = definition.transitions.iter().fold(
        HashMap::<&str, usize>::new(),
        |mut counts, transition| {
            *counts.entry(&transition.from_stage_id).or_default() += 1;
            counts
        },
    );
    for stage in &definition.stages {
        let id = stage.id.as_str();
        let count = outgoing.get(id).copied().unwrap_or_default();
        if stage.terminal && count > 0 {
            issues.push(issue(
                "terminalHasOutgoingTransition",
                format!("stages.{id}"),
                "a terminal stage must not have outgoing transitions",
            ));
        } else if !stage.terminal && count == 0 {
            issues.push(issue(
                "nonTerminalHasNoOutgoingTransition",
                format!("stages.{id}"),
                "a non-terminal stage must have an outgoing transition",
            ));
        }
    }
}

fn validate_reachability(
    definition: &WorkflowDefinition,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    let mut outgoing = HashMap::<&str, Vec<&str>>::new();
    let mut incoming = HashMap::<&str, Vec<&str>>::new();
    for transition in &definition.transitions {
        outgoing
            .entry(&transition.from_stage_id)
            .or_default()
            .push(&transition.to_stage_id);
        incoming
            .entry(&transition.to_stage_id)
            .or_default()
            .push(&transition.from_stage_id);
    }

    let reachable = walk(&definition.initial_stage_id, &outgoing);
    for id in definition
        .stages
        .iter()
        .map(|stage| stage.id.as_str())
        .filter(|id| !reachable.contains(id))
    {
        issues.push(issue(
            "unreachableStage",
            format!("stages.{id}"),
            format!("stage `{id}` is not reachable from the initial stage"),
        ));
    }

    let mut reaches_terminal = HashSet::new();
    let mut queue = definition
        .stages
        .iter()
        .filter_map(|stage| stage.terminal.then_some(stage.id.as_str()))
        .collect::<VecDeque<_>>();
    while let Some(id) = queue.pop_front() {
        if !reaches_terminal.insert(id) {
            continue;
        }
        queue.extend(incoming.get(id).into_iter().flatten().copied());
    }
    for stage in &definition.stages {
        let id = stage.id.as_str();
        if !stage.terminal && !reaches_terminal.contains(id) {
            issues.push(issue(
                "cannotReachTerminal",
                format!("stages.{id}"),
                format!("stage `{id}` has no path to a terminal stage"),
            ));
        }
    }
}

fn walk<'a>(start: &'a str, edges: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        queue.extend(edges.get(id).into_iter().flatten().copied());
    }
    visited
}

fn issue(
    code: &'static str,
    path: impl Into<String>,
    message: impl Into<String>,
) -> WorkflowValidationIssue {
    WorkflowValidationIssue {
        code,
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;
