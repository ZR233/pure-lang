//! Thread Mode 预设状态图的纯函数编译器。

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use pl_protocol::{WorkflowDefinition, WorkflowState, WorkflowStateKind, WorkflowTransition};
use serde::Serialize;

use crate::canonical_json_hash;

pub const MAX_WORKFLOW_STATES: usize = 32;
pub const MAX_WORKFLOW_TRANSITIONS: usize = 96;
pub const MAX_WORKFLOW_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_WORKFLOW_STATE_BYTES: usize = 256 * 1024;
pub const MAX_WORKFLOW_HISTORY: usize = 64;
pub const MAX_ARCHIVED_WORKFLOW_RUNS: usize = 16;
pub const MAX_WORKFLOW_OPERATION_RECEIPTS: usize = 32;

const MAX_STATE_ID_BYTES: usize = 64;
const MAX_TITLE_CHARS: usize = 128;
const MAX_GOAL_BYTES: usize = 8 * 1024;
const MAX_STATE_INSTRUCTIONS_BYTES: usize = 4 * 1024;
const MAX_GUARD_BYTES: usize = 2 * 1024;
const MAX_COMPLETION_CRITERIA: usize = 16;
const MAX_COMPLETION_CRITERION_BYTES: usize = 512;

/// 注册后可由一个 Turn 快照共享的不可变状态图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorkflowDefinition {
    definition: WorkflowDefinition,
    graph_hash: String,
    states_by_id: BTreeMap<String, usize>,
    outgoing_by_state: BTreeMap<String, Vec<usize>>,
}

impl CompiledWorkflowDefinition {
    pub fn definition(&self) -> &WorkflowDefinition {
        &self.definition
    }

    pub fn graph_hash(&self) -> &str {
        &self.graph_hash
    }

    pub fn initial_state(&self) -> &WorkflowState {
        self.state(&self.definition.initial_state_id)
            .expect("compiled initial state must exist")
    }

    pub fn state(&self, state_id: &str) -> Option<&WorkflowState> {
        self.states_by_id
            .get(state_id)
            .map(|index| &self.definition.states[*index])
    }

    pub fn outgoing(&self, state_id: &str) -> Vec<&WorkflowTransition> {
        self.outgoing_by_state
            .get(state_id)
            .into_iter()
            .flatten()
            .map(|index| &self.definition.transitions[*index])
            .collect()
    }

    pub fn transition(&self, source: &str, target: &str) -> Option<&WorkflowTransition> {
        self.outgoing(source)
            .into_iter()
            .find(|transition| transition.target_state_id == target)
    }
}

/// 一条稳定、可测试的状态图定义问题。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowValidationIssue {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

/// 状态图定义未通过注册时编译。
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

/// 编译并规范化扁平、单活动 state 的确定性状态图。
///
/// # Errors
///
/// 定义违反大小、唯一性、端点、终态或可达性约束时返回所有稳定排序的问题。
pub fn compile_workflow_definition(
    definition: WorkflowDefinition,
) -> Result<CompiledWorkflowDefinition, WorkflowCompilerError> {
    let definition = normalize_definition(definition);
    let mut issues = Vec::new();
    validate_schema(&definition, &mut issues);
    validate_ids(&definition, &mut issues);

    let states = definition
        .states
        .iter()
        .map(|state| (state.id.as_str(), state))
        .collect::<BTreeMap<_, _>>();
    validate_endpoints(&definition, &states, &mut issues);
    validate_initial_state(&definition, &states, &mut issues);
    validate_final_states(&definition, &mut issues);
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

    let states_by_id = definition
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| (state.id.clone(), index))
        .collect();
    let mut outgoing_by_state = BTreeMap::<String, Vec<usize>>::new();
    for (index, transition) in definition.transitions.iter().enumerate() {
        outgoing_by_state
            .entry(transition.source_state_id.clone())
            .or_default()
            .push(index);
    }
    Ok(CompiledWorkflowDefinition {
        graph_hash: canonical_json_hash(&canonical),
        definition,
        states_by_id,
        outgoing_by_state,
    })
}

fn normalize_definition(mut definition: WorkflowDefinition) -> WorkflowDefinition {
    definition.title = definition.title.trim().to_string();
    definition.goal = definition.goal.trim().to_string();
    definition.initial_state_id = definition.initial_state_id.trim().to_string();
    for state in &mut definition.states {
        state.id = state.id.trim().to_string();
        state.title = state.title.trim().to_string();
        state.instructions = state.instructions.trim().to_string();
        for criterion in &mut state.completion_criteria {
            *criterion = criterion.trim().to_string();
        }
    }
    for transition in &mut definition.transitions {
        transition.source_state_id = transition.source_state_id.trim().to_string();
        transition.target_state_id = transition.target_state_id.trim().to_string();
        transition.guard = transition.guard.trim().to_string();
    }
    definition
        .states
        .sort_by(|left, right| left.id.cmp(&right.id));
    definition.transitions.sort_by(|left, right| {
        (&left.source_state_id, &left.target_state_id, &left.guard).cmp(&(
            &right.source_state_id,
            &right.target_state_id,
            &right.guard,
        ))
    });
    definition
}

fn validate_schema(definition: &WorkflowDefinition, issues: &mut Vec<WorkflowValidationIssue>) {
    validate_required_text("title", &definition.title, MAX_TITLE_CHARS, true, issues);
    validate_required_text("goal", &definition.goal, MAX_GOAL_BYTES, false, issues);
    if definition.states.is_empty() || definition.states.len() > MAX_WORKFLOW_STATES {
        issues.push(issue(
            "stateCount",
            "states",
            format!("workflow must contain 1 to {MAX_WORKFLOW_STATES} states"),
        ));
    }
    if definition.transitions.len() > MAX_WORKFLOW_TRANSITIONS {
        issues.push(issue(
            "transitionCount",
            "transitions",
            format!("workflow may contain at most {MAX_WORKFLOW_TRANSITIONS} transitions"),
        ));
    }
    for (index, state) in definition.states.iter().enumerate() {
        let path = format!("states[{index}]");
        validate_required_text(
            &format!("{path}.title"),
            &state.title,
            MAX_TITLE_CHARS,
            true,
            issues,
        );
        if state.kind == WorkflowStateKind::Atomic {
            validate_required_text(
                &format!("{path}.instructions"),
                &state.instructions,
                MAX_STATE_INSTRUCTIONS_BYTES,
                false,
                issues,
            );
            if state.completion_criteria.is_empty() {
                issues.push(issue(
                    "required",
                    format!("{path}.completionCriteria"),
                    "an atomic state must include completion criteria",
                ));
            }
        } else if state.instructions.len() > MAX_STATE_INSTRUCTIONS_BYTES {
            issues.push(issue(
                "tooLarge",
                format!("{path}.instructions"),
                format!("instructions may contain at most {MAX_STATE_INSTRUCTIONS_BYTES} bytes"),
            ));
        }
        if state.completion_criteria.len() > MAX_COMPLETION_CRITERIA {
            issues.push(issue(
                "tooMany",
                format!("{path}.completionCriteria"),
                format!("a state may contain at most {MAX_COMPLETION_CRITERIA} criteria"),
            ));
        }
        for (criterion_index, criterion) in state.completion_criteria.iter().enumerate() {
            validate_required_text(
                &format!("{path}.completionCriteria[{criterion_index}]"),
                criterion,
                MAX_COMPLETION_CRITERION_BYTES,
                false,
                issues,
            );
        }
    }
    for (index, transition) in definition.transitions.iter().enumerate() {
        validate_required_text(
            &format!("transitions[{index}].guard"),
            &transition.guard,
            MAX_GUARD_BYTES,
            false,
            issues,
        );
    }
}

fn validate_required_text(
    path: &str,
    value: &str,
    limit: usize,
    character_limit: bool,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    if value.is_empty() {
        issues.push(issue("required", path, "value must not be empty"));
    } else {
        let actual = if character_limit {
            value.chars().count()
        } else {
            value.len()
        };
        if actual > limit {
            issues.push(issue(
                "tooLarge",
                path,
                format!("value may contain at most {limit} units"),
            ));
        }
    }
}

fn validate_ids(definition: &WorkflowDefinition, issues: &mut Vec<WorkflowValidationIssue>) {
    validate_id("initialStateId", &definition.initial_state_id, issues);
    let mut state_ids = HashSet::new();
    for (index, state) in definition.states.iter().enumerate() {
        validate_id(format!("states[{index}].id"), &state.id, issues);
        if !state_ids.insert(state.id.as_str()) {
            issues.push(issue(
                "duplicateStateId",
                format!("states[{index}].id"),
                format!("state id `{}` is duplicated", state.id),
            ));
        }
    }
    let mut edges = HashSet::new();
    for (index, transition) in definition.transitions.iter().enumerate() {
        validate_id(
            format!("transitions[{index}].sourceStateId"),
            &transition.source_state_id,
            issues,
        );
        validate_id(
            format!("transitions[{index}].targetStateId"),
            &transition.target_state_id,
            issues,
        );
        if !edges.insert((
            transition.source_state_id.as_str(),
            transition.target_state_id.as_str(),
        )) {
            issues.push(issue(
                "duplicateTransition",
                format!("transitions[{index}]"),
                format!(
                    "transition `{}` -> `{}` is duplicated",
                    transition.source_state_id, transition.target_state_id
                ),
            ));
        }
    }
}

fn validate_id(path: impl Into<String>, id: &str, issues: &mut Vec<WorkflowValidationIssue>) {
    if id.is_empty()
        || id.len() > MAX_STATE_ID_BYTES
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        issues.push(issue(
            "invalidId",
            path,
            format!(
                "`{id}` must be 1-{MAX_STATE_ID_BYTES} lowercase ASCII letters, digits, `-`, or `_`"
            ),
        ));
    }
}

fn validate_endpoints(
    definition: &WorkflowDefinition,
    states: &BTreeMap<&str, &WorkflowState>,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    for (index, transition) in definition.transitions.iter().enumerate() {
        for (field, id) in [
            ("sourceStateId", transition.source_state_id.as_str()),
            ("targetStateId", transition.target_state_id.as_str()),
        ] {
            if !states.contains_key(id) {
                issues.push(issue(
                    "unknownEndpoint",
                    format!("transitions[{index}].{field}"),
                    format!("unknown state `{id}`"),
                ));
            }
        }
    }
}

fn validate_initial_state(
    definition: &WorkflowDefinition,
    states: &BTreeMap<&str, &WorkflowState>,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    if !states.contains_key(definition.initial_state_id.as_str()) {
        issues.push(issue(
            "unknownInitialState",
            "initialStateId",
            format!("unknown initial state `{}`", definition.initial_state_id),
        ));
    }
}

fn validate_final_states(
    definition: &WorkflowDefinition,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    if !definition
        .states
        .iter()
        .any(|state| state.kind == WorkflowStateKind::Final)
    {
        issues.push(issue(
            "missingFinalState",
            "states",
            "workflow must contain at least one final state",
        ));
    }
    let mut outgoing = BTreeMap::<&str, usize>::new();
    for transition in &definition.transitions {
        *outgoing.entry(&transition.source_state_id).or_default() += 1;
    }
    for state in &definition.states {
        let count = outgoing.get(state.id.as_str()).copied().unwrap_or_default();
        if state.kind == WorkflowStateKind::Final && count > 0 {
            issues.push(issue(
                "finalHasOutgoingTransition",
                format!("states.{}", state.id),
                "a final state must not have outgoing transitions",
            ));
        } else if state.kind == WorkflowStateKind::Atomic && count == 0 {
            issues.push(issue(
                "atomicHasNoOutgoingTransition",
                format!("states.{}", state.id),
                "an atomic state must have an outgoing transition",
            ));
        }
    }
}

fn validate_reachability(
    definition: &WorkflowDefinition,
    issues: &mut Vec<WorkflowValidationIssue>,
) {
    let mut outgoing = BTreeMap::<&str, Vec<&str>>::new();
    let mut incoming = BTreeMap::<&str, Vec<&str>>::new();
    for transition in &definition.transitions {
        outgoing
            .entry(&transition.source_state_id)
            .or_default()
            .push(&transition.target_state_id);
        incoming
            .entry(&transition.target_state_id)
            .or_default()
            .push(&transition.source_state_id);
    }
    let reachable = walk(&definition.initial_state_id, &outgoing);
    for id in definition
        .states
        .iter()
        .map(|state| state.id.as_str())
        .filter(|id| !reachable.contains(id))
    {
        issues.push(issue(
            "unreachableState",
            format!("states.{id}"),
            format!("state `{id}` is not reachable from the initial state"),
        ));
    }

    let mut reaches_final = HashSet::new();
    let mut queue = definition
        .states
        .iter()
        .filter_map(|state| (state.kind == WorkflowStateKind::Final).then_some(state.id.as_str()))
        .collect::<VecDeque<_>>();
    while let Some(id) = queue.pop_front() {
        if !reaches_final.insert(id) {
            continue;
        }
        queue.extend(incoming.get(id).into_iter().flatten().copied());
    }
    for state in &definition.states {
        if state.kind == WorkflowStateKind::Atomic && !reaches_final.contains(state.id.as_str()) {
            issues.push(issue(
                "cannotReachFinal",
                format!("states.{}", state.id),
                format!("state `{}` has no path to a final state", state.id),
            ));
        }
    }
}

fn walk<'a>(start: &'a str, edges: &BTreeMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
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
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::thread::test_support::graph;

    #[test]
    fn normalized_graph_order_has_a_stable_hash() {
        let first = compile_workflow_definition(graph("")).expect("valid graph");
        let mut reordered = graph("");
        reordered.states.reverse();
        let second = compile_workflow_definition(reordered).expect("valid reordered graph");

        assert_eq!(first.graph_hash(), second.graph_hash());
    }
}
