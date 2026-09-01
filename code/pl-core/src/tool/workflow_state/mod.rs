//! Canonical 工作流状态工具。

use futures::FutureExt;
use pl_protocol::{
    ModeInstructionSnapshot, PureError, WorkflowDefinition, WorkflowOperationReceipt, WorkflowRun,
    WorkflowRunArchive, WorkflowRunLifecycle, WorkflowSessionState, WorkflowStage,
    WorkflowTransition, WorkflowTransitionRecord,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::time::unix_seconds;
use crate::turn::ToolEffect;
use crate::workflow::{
    MAX_ARCHIVED_WORKFLOW_RUNS, MAX_WORKFLOW_HISTORY, MAX_WORKFLOW_OPERATION_RECEIPTS,
    WorkflowValidationIssue, compile_definition, validate_session_state_size,
};

use super::{
    BoxFuture, Tool, ToolBatchPolicy, ToolCallContext, ToolInput, ToolResult, TypedTool,
    deserialize_tool_input,
};

pub const TOOL_WORKFLOW_STATE: &str = "workflow_state";

const MAX_TRANSITION_REASON_BYTES: usize = 2 * 1024;
const MAX_COMPLETION_SUMMARY_BYTES: usize = 8 * 1024;
const MAX_COMPLETION_EVIDENCE: usize = 16;
const WORKFLOW_STATE_DESCRIPTION: &str = concat!(
    "Manage the root conversation's canonical workflow when the active Mode Skill requires it. ",
    "Arguments must be one JSON object with a required top-level `action`; never send `{}`. ",
    "Nested `definition`, `stages`, `transitions`, and `completion` values must be JSON objects or arrays, never JSON-encoded strings. ",
    "For the first compile, omit the optional `expectedRunId` field; never use the string \"null\". ",
    "The first compile must contain the complete graph: include every transition, and give every non-terminal stage at least one outgoing transition. ",
    "Valid JSON examples: status = {\"action\":\"status\",\"view\":\"current\"}; ",
    "compile = {\"action\":\"compile\",\"expectedRevision\":0,\"definition\":{\"title\":\"Task\",\"goal\":\"Complete task\",\"initialStageId\":\"planning\",\"stages\":[{\"id\":\"planning\",\"title\":\"Planning\",\"instructions\":\"Create the plan\",\"completionCriteria\":[\"Plan is ready\"],\"terminal\":false},{\"id\":\"completed\",\"title\":\"Completed\",\"instructions\":\"\",\"completionCriteria\":[],\"terminal\":true}],\"transitions\":[{\"fromStageId\":\"planning\",\"toStageId\":\"completed\",\"when\":\"Work is verified\"}]}}; ",
    "transition = {\"action\":\"transition\",\"expectedRunId\":\"run-id\",\"expectedRevision\":1,\"expectedStageId\":\"planning\",\"toStageId\":\"completed\",\"reason\":\"Work is verified\",\"completion\":{\"summary\":\"Planning completed\",\"evidence\":[\"Plan is ready\"]}}; ",
    "supersede uses the same object-valued `definition` shape as compile and also requires string `expectedRunId`, integer `expectedRevision`, string `expectedStageId`, and string `reason`. ",
    "This tool must be the only tool call in its provider response."
);
const WORKFLOW_STATE_INPUT_ERROR_HINT: &str = concat!(
    "Pass one object with top-level `action`; never pass `{}`. ",
    "Valid actions are `status`, `compile`, `transition`, and `supersede`. ",
    "Pass `definition`, its stage entries, and `completion` as JSON objects, never strings. ",
    "Omit `expectedRunId` for the first compile; never use the string \"null\". ",
    "Compile the complete graph in one call and include an outgoing transition for every non-terminal stage. ",
    "To inspect canonical state, retry with {\"action\":\"status\",\"view\":\"current\"}. ",
    "To create the first run, use `compile` with `expectedRevision`: 0 and a complete `definition`"
);

/// 工作流工具在整个 run 生命周期内冻结当前模式内容。
#[derive(Debug, Clone)]
pub struct WorkflowStateTool {
    working_set: crate::TurnWorkingSetHandle,
    mode: ModeInstructionSnapshot,
}

impl WorkflowStateTool {
    pub fn new(working_set: crate::TurnWorkingSetHandle, mode: ModeInstructionSnapshot) -> Self {
        Self { working_set, mode }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "action",
    deny_unknown_fields
)]
enum WorkflowStateInput {
    Compile {
        expected_revision: u64,
        expected_run_id: Option<String>,
        definition: WorkflowDefinitionInput,
    },
    Status {
        #[serde(default)]
        view: WorkflowStatusView,
    },
    Transition {
        expected_run_id: String,
        expected_revision: u64,
        expected_stage_id: String,
        to_stage_id: String,
        reason: String,
        completion: WorkflowCompletionInput,
    },
    Supersede {
        expected_run_id: String,
        expected_revision: u64,
        expected_stage_id: String,
        reason: String,
        definition: WorkflowDefinitionInput,
    },
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum WorkflowStatusView {
    #[default]
    Current,
    Graph,
    History,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowDefinitionInput {
    title: String,
    goal: String,
    initial_stage_id: String,
    stages: Vec<WorkflowStageInput>,
    /// 完整迁移图；每个非终态 stage 至少需要一条出边。
    transitions: Vec<WorkflowTransitionInput>,
}

impl From<WorkflowDefinitionInput> for WorkflowDefinition {
    fn from(input: WorkflowDefinitionInput) -> Self {
        Self {
            title: input.title,
            goal: input.goal,
            initial_stage_id: input.initial_stage_id,
            stages: input.stages.into_iter().map(Into::into).collect(),
            transitions: input.transitions.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowStageInput {
    id: String,
    title: String,
    instructions: String,
    #[serde(default)]
    completion_criteria: Vec<String>,
    #[serde(default)]
    terminal: bool,
}

impl From<WorkflowStageInput> for WorkflowStage {
    fn from(input: WorkflowStageInput) -> Self {
        Self {
            id: input.id,
            title: input.title,
            instructions: input.instructions,
            completion_criteria: input.completion_criteria,
            terminal: input.terminal,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowTransitionInput {
    from_stage_id: String,
    to_stage_id: String,
    when: String,
}

/// 验证 workflow_state 的 wire 参数形状，不执行状态变更。
///
/// 该入口供 artifact/replay 等外部验收边界复用与运行时工具相同的
/// typed serde contract，避免维护第二套字段、类型和未知字段校验。
pub fn validate_workflow_state_arguments(arguments: serde_json::Value) -> Result<(), PureError> {
    deserialize_tool_input::<WorkflowStateInput>(TOOL_WORKFLOW_STATE, arguments).map(|_| ())
}

/// 验证 provider wire capture 中的 `workflow_state` 调用。
///
/// 完整调用沿用运行时的严格 typed contract。历史请求可能包含一次被模型
/// 重试替换的流式 transition 草稿：它已经带有完整 CAS、reason 和 evidence，
/// 但在参数增量尚未结束时尚未写入 `completion.summary`。该草稿只在 wire
/// replay 边界允许按空 summary 做形状验证，绝不会改变运行时执行语义。
pub fn validate_workflow_state_wire_arguments(
    arguments: serde_json::Value,
) -> Result<(), PureError> {
    match validate_workflow_state_arguments(arguments.clone()) {
        Ok(()) => Ok(()),
        Err(strict_error) => {
            let Some(mut partial) = arguments.as_object().cloned() else {
                return Err(strict_error);
            };
            if partial.get("action").and_then(serde_json::Value::as_str) != Some("transition") {
                return Err(strict_error);
            }
            let Some(mut completion) = partial
                .get("completion")
                .and_then(serde_json::Value::as_object)
                .cloned()
            else {
                return Err(strict_error);
            };
            if completion.contains_key("summary") {
                return Err(strict_error);
            }
            if !completion
                .get("evidence")
                .is_some_and(serde_json::Value::is_array)
            {
                return Err(strict_error);
            }
            completion.insert(
                "summary".to_string(),
                serde_json::Value::String(String::new()),
            );
            partial.insert(
                "completion".to_string(),
                serde_json::Value::Object(completion),
            );
            deserialize_tool_input::<WorkflowStateInput>(
                TOOL_WORKFLOW_STATE,
                serde_json::Value::Object(partial),
            )
            .map(|_| ())
        }
    }
}

impl From<WorkflowTransitionInput> for WorkflowTransition {
    fn from(input: WorkflowTransitionInput) -> Self {
        Self {
            from_stage_id: input.from_stage_id,
            to_stage_id: input.to_stage_id,
            when: input.when,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkflowCompletionInput {
    summary: String,
    #[serde(default)]
    evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowStateResponse {
    accepted: bool,
    code: &'static str,
    operation_revision: u64,
    snapshot: serde_json::Value,
    constraint_prompt: String,
    validation_issues: Vec<WorkflowValidationIssue>,
    recovery_actions: Vec<String>,
}

impl Tool for WorkflowStateTool {
    fn name(&self) -> &str {
        TOOL_WORKFLOW_STATE
    }

    fn description(&self) -> &str {
        WORKFLOW_STATE_DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        workflow_state_input_schema(self.name(), self.description())
    }

    fn effect(&self) -> Option<ToolEffect> {
        Some(ToolEffect::Read)
    }

    fn batch_policy(&self) -> ToolBatchPolicy {
        ToolBatchPolicy::Solo
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        async move {
            let argument_hash = crate::canonical_json_hash(&input.arguments);
            let input = deserialize_tool_input::<WorkflowStateInput>(self.name(), input.arguments)
                .map_err(workflow_state_input_error)?;
            let response = self.execute_action(input, &context, argument_hash)?;
            ToolResult::json(response)
        }
        .boxed()
    }
}

fn workflow_state_input_schema(tool_name: &str, description: &str) -> serde_json::Value {
    let mut schema = TypedTool::<WorkflowStateInput>::new(tool_name, description).input_schema();
    let Some(root) = schema.as_object_mut() else {
        return schema;
    };
    let Some(variants) = root.get("oneOf").and_then(serde_json::Value::as_array) else {
        return schema;
    };

    let definitions = root
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut properties = serde_json::Map::new();
    let mut actions = Vec::new();
    for variant in variants {
        let Some(variant_properties) = variant
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (name, property) in variant_properties {
            if name == "action" {
                if let Some(action) = property.get("const") {
                    actions.push(action.clone());
                }
            } else {
                let mut property = property.clone();
                inline_workflow_schema_refs(&mut property, &definitions, 0);
                properties.entry(name.clone()).or_insert(property);
            }
        }
    }
    if actions.is_empty() {
        return schema;
    }

    properties.insert(
        "action".to_string(),
        serde_json::json!({
            "type": "string",
            "enum": actions,
            "description": "Required workflow operation. Choose exactly one value and provide that operation's fields."
        }),
    );
    properties.insert(
        "expectedRunId".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "Existing canonical run ID for transition, supersede, or compile after a terminal run. Omit this field for the first compile; never pass the string `null`."
        }),
    );
    root.insert(
        "properties".to_string(),
        serde_json::Value::Object(properties),
    );
    root.insert("required".to_string(), serde_json::json!(["action"]));
    root.insert(
        "additionalProperties".to_string(),
        serde_json::Value::Bool(false),
    );
    schema
}

fn inline_workflow_schema_refs(
    schema: &mut serde_json::Value,
    definitions: &serde_json::Map<String, serde_json::Value>,
    depth: usize,
) {
    if depth >= 16 {
        return;
    }
    let referenced = schema
        .get("$ref")
        .and_then(serde_json::Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .map(ToOwned::to_owned);
    if let Some(referenced) = referenced
        && let Some(definition) = definitions.get(&referenced)
    {
        *schema = definition.clone();
        inline_workflow_schema_refs(schema, definitions, depth + 1);
        return;
    }

    match schema {
        serde_json::Value::Array(items) => {
            for item in items {
                inline_workflow_schema_refs(item, definitions, depth + 1);
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                inline_workflow_schema_refs(value, definitions, depth + 1);
            }
        }
        _ => {}
    }
}

fn workflow_state_input_error(error: PureError) -> PureError {
    let PureError::ToolExecutionFailed { error, .. } = error else {
        return error;
    };
    PureError::ToolExecutionFailed {
        tool: TOOL_WORKFLOW_STATE.to_string(),
        error: format!("{error}. {WORKFLOW_STATE_INPUT_ERROR_HINT}"),
    }
}

impl WorkflowStateTool {
    fn execute_action(
        &self,
        input: WorkflowStateInput,
        context: &ToolCallContext,
        argument_hash: String,
    ) -> Result<WorkflowStateResponse, PureError> {
        if let WorkflowStateInput::Status { view } = input {
            let state = self.working_set.workflow().unwrap_or_default();
            return Ok(accepted_response("status", &state, snapshot(&state, view)));
        }

        let operation_id = format!(
            "{}/{}",
            context.identity().turn_id,
            context.identity().call_id
        );
        let mut state = self.working_set.workflow().unwrap_or_default();
        if let Some(receipt) = state
            .operation_receipts
            .iter()
            .find(|receipt| receipt.operation_id == operation_id)
        {
            if receipt.argument_hash == argument_hash {
                return Ok(accepted_response(
                    "alreadyApplied",
                    &state,
                    current_snapshot(&state),
                ));
            }
            return Ok(rejected_response(
                "operationIdentityConflict",
                &state,
                Vec::new(),
                vec!["Retry with a new tool call identity.".to_string()],
            ));
        }

        let code = match input {
            WorkflowStateInput::Compile {
                expected_revision,
                expected_run_id,
                definition,
            } => self.compile(
                &mut state,
                expected_revision,
                expected_run_id.as_deref(),
                definition.into(),
                context,
                &argument_hash,
                &operation_id,
            )?,
            WorkflowStateInput::Transition {
                expected_run_id,
                expected_revision,
                expected_stage_id,
                to_stage_id,
                reason,
                completion,
            } => self.transition(
                &mut state,
                WorkflowCas {
                    expected_run_id: &expected_run_id,
                    expected_revision,
                    expected_stage_id: &expected_stage_id,
                },
                &to_stage_id,
                reason,
                completion,
                context,
                &argument_hash,
                &operation_id,
            )?,
            WorkflowStateInput::Supersede {
                expected_run_id,
                expected_revision,
                expected_stage_id,
                reason,
                definition,
            } => self.supersede(
                &mut state,
                WorkflowCas {
                    expected_run_id: &expected_run_id,
                    expected_revision,
                    expected_stage_id: &expected_stage_id,
                },
                reason,
                definition.into(),
                context,
                &argument_hash,
                &operation_id,
            )?,
            WorkflowStateInput::Status { .. } => unreachable!("status returned above"),
        };

        if let MutationOutcome::Rejected(response) = code {
            return Ok(response);
        }
        let MutationOutcome::Accepted(code) = code else {
            unreachable!("rejected outcome returned above")
        };
        validate_session_state_size(&state)?;
        self.working_set
            .apply(crate::TurnWorkingSetChange::ReplaceWorkflow(Some(
                state.clone(),
            )))?;
        Ok(accepted_response(code, &state, current_snapshot(&state)))
    }

    #[allow(clippy::too_many_arguments)]
    fn compile(
        &self,
        state: &mut WorkflowSessionState,
        expected_revision: u64,
        expected_run_id: Option<&str>,
        definition: WorkflowDefinition,
        context: &ToolCallContext,
        argument_hash: &str,
        operation_id: &str,
    ) -> Result<MutationOutcome, PureError> {
        let compiled = match compile_definition(definition) {
            Ok(compiled) => compiled,
            Err(error) => {
                return Ok(MutationOutcome::Rejected(rejected_response(
                    "invalidDefinition",
                    state,
                    error.issues().to_vec(),
                    vec![
                        "Correct the reported definition issues and call compile again."
                            .to_string(),
                    ],
                )));
            }
        };
        if let Some(run) = &state.current_run {
            if run.lifecycle == WorkflowRunLifecycle::Active {
                return Ok(MutationOutcome::Rejected(rejected_response(
                    "activeWorkflowExists",
                    state,
                    Vec::new(),
                    vec!["Transition the active run to a terminal stage, or use supersede for a materially changed goal.".to_string()],
                )));
            }
            if expected_run_id != Some(run.run_id.as_str()) {
                return Ok(MutationOutcome::Rejected(cas_rejection(
                    "runMismatch",
                    state,
                )));
            }
        } else if expected_run_id.is_some() {
            return Ok(MutationOutcome::Rejected(cas_rejection(
                "runMismatch",
                state,
            )));
        }
        if expected_revision != state.revision {
            return Ok(MutationOutcome::Rejected(cas_rejection(
                "staleRevision",
                state,
            )));
        }

        if let Some(previous) = state.current_run.take() {
            archive_run(
                state,
                previous,
                "completed",
                "Started a new workflow lineage",
            )?;
        }
        let next_revision = state.revision.saturating_add(1);
        let identity_seed = format!("{operation_id}:{argument_hash}:{next_revision}");
        let now = unix_seconds();
        let initial_stage = compiled
            .definition
            .stages
            .iter()
            .find(|stage| stage.id == compiled.definition.initial_stage_id)
            .expect("compiled definition contains its initial stage");
        let initial_stage_id = initial_stage.id.clone();
        let lifecycle = lifecycle_for_stage(initial_stage);
        state.current_run = Some(WorkflowRun {
            lineage_id: generated_id("lineage", &identity_seed),
            run_id: generated_id("run", &identity_seed),
            definition: compiled.definition,
            definition_hash: compiled.definition_hash,
            mode: self.mode.clone(),
            lifecycle,
            current_stage_id: initial_stage_id,
            compiled_at: now,
            updated_at: now,
            history_tail: Vec::new(),
            archived_transition_count: 0,
            archived_transition_digest: String::new(),
        });
        commit_receipt(state, next_revision, operation_id, argument_hash, context);
        Ok(MutationOutcome::Accepted("compiled"))
    }

    #[allow(clippy::too_many_arguments)]
    fn transition(
        &self,
        state: &mut WorkflowSessionState,
        cas: WorkflowCas<'_>,
        to_stage_id: &str,
        reason: String,
        completion: WorkflowCompletionInput,
        context: &ToolCallContext,
        argument_hash: &str,
        operation_id: &str,
    ) -> Result<MutationOutcome, PureError> {
        if let Some(response) = validate_completion(state, &reason, &completion) {
            return Ok(MutationOutcome::Rejected(response));
        }
        if let Some(response) = validate_cas(state, cas, true) {
            return Ok(MutationOutcome::Rejected(response));
        }
        let run = state
            .current_run
            .as_mut()
            .expect("CAS validated current run");
        let Some(target) = run
            .definition
            .stages
            .iter()
            .find(|stage| stage.id == to_stage_id)
        else {
            return Ok(MutationOutcome::Rejected(rejected_response(
                "unknownTargetStage",
                state,
                Vec::new(),
                vec![
                    "Use one of the direct outgoing stage IDs in the canonical snapshot."
                        .to_string(),
                ],
            )));
        };
        let target_id = target.id.clone();
        let target_lifecycle = lifecycle_for_stage(target);
        if !run.definition.transitions.iter().any(|transition| {
            transition.from_stage_id == run.current_stage_id && transition.to_stage_id == target_id
        }) {
            return Ok(MutationOutcome::Rejected(rejected_response(
                "transitionNotAllowed",
                state,
                Vec::new(),
                vec!["Choose a direct outgoing transition from the current stage.".to_string()],
            )));
        }

        let next_revision = state.revision.saturating_add(1);
        let from_stage_id = run.current_stage_id.clone();
        let now = unix_seconds();
        run.current_stage_id = target_id.clone();
        run.lifecycle = target_lifecycle;
        run.updated_at = now;
        run.history_tail.push(WorkflowTransitionRecord {
            revision: next_revision,
            from_stage_id,
            to_stage_id: target_id,
            reason,
            summary: completion.summary,
            evidence: completion.evidence,
            turn_id: context.identity().turn_id.clone(),
            call_id: context.identity().call_id.clone(),
            transitioned_at: now,
        });
        trim_history(run)?;
        commit_receipt(state, next_revision, operation_id, argument_hash, context);
        Ok(MutationOutcome::Accepted("transitioned"))
    }

    #[allow(clippy::too_many_arguments)]
    fn supersede(
        &self,
        state: &mut WorkflowSessionState,
        cas: WorkflowCas<'_>,
        reason: String,
        definition: WorkflowDefinition,
        context: &ToolCallContext,
        argument_hash: &str,
        operation_id: &str,
    ) -> Result<MutationOutcome, PureError> {
        if reason.trim().is_empty() || reason.len() > MAX_TRANSITION_REASON_BYTES {
            return Ok(MutationOutcome::Rejected(rejected_response(
                "invalidDefinition",
                state,
                vec![WorkflowValidationIssue {
                    code: "invalidReason",
                    path: "reason".to_string(),
                    message: format!(
                        "reason must be non-empty and at most {MAX_TRANSITION_REASON_BYTES} bytes"
                    ),
                }],
                vec!["Provide a concise supersede reason.".to_string()],
            )));
        }
        let compiled = match compile_definition(definition) {
            Ok(compiled) => compiled,
            Err(error) => {
                return Ok(MutationOutcome::Rejected(rejected_response(
                    "invalidDefinition",
                    state,
                    error.issues().to_vec(),
                    vec![
                        "Correct the replacement definition; the active run is unchanged."
                            .to_string(),
                    ],
                )));
            }
        };
        if let Some(response) = validate_cas(state, cas, true) {
            return Ok(MutationOutcome::Rejected(response));
        }

        let old_run = state.current_run.take().expect("CAS validated current run");
        let lineage_id = old_run.lineage_id.clone();
        let frozen_mode = old_run.mode.clone();
        archive_run(state, old_run, "superseded", reason.trim())?;
        let next_revision = state.revision.saturating_add(1);
        let identity_seed = format!("{operation_id}:{argument_hash}:{next_revision}");
        let now = unix_seconds();
        let initial_stage = compiled
            .definition
            .stages
            .iter()
            .find(|stage| stage.id == compiled.definition.initial_stage_id)
            .expect("compiled definition contains its initial stage");
        let initial_stage_id = initial_stage.id.clone();
        let lifecycle = lifecycle_for_stage(initial_stage);
        state.current_run = Some(WorkflowRun {
            lineage_id,
            run_id: generated_id("run", &identity_seed),
            definition: compiled.definition,
            definition_hash: compiled.definition_hash,
            mode: frozen_mode,
            lifecycle,
            current_stage_id: initial_stage_id,
            compiled_at: now,
            updated_at: now,
            history_tail: Vec::new(),
            archived_transition_count: 0,
            archived_transition_digest: String::new(),
        });
        commit_receipt(state, next_revision, operation_id, argument_hash, context);
        Ok(MutationOutcome::Accepted("superseded"))
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkflowCas<'a> {
    expected_run_id: &'a str,
    expected_revision: u64,
    expected_stage_id: &'a str,
}

enum MutationOutcome {
    Accepted(&'static str),
    Rejected(WorkflowStateResponse),
}

fn validate_cas(
    state: &WorkflowSessionState,
    cas: WorkflowCas<'_>,
    require_active: bool,
) -> Option<WorkflowStateResponse> {
    let Some(run) = &state.current_run else {
        return Some(rejected_response(
            "workflowNotCompiled",
            state,
            Vec::new(),
            vec!["Compile a workflow definition before changing stages.".to_string()],
        ));
    };
    if run.run_id != cas.expected_run_id {
        return Some(cas_rejection("runMismatch", state));
    }
    if state.revision != cas.expected_revision {
        return Some(cas_rejection("staleRevision", state));
    }
    if run.current_stage_id != cas.expected_stage_id {
        return Some(cas_rejection("stageMismatch", state));
    }
    if require_active && run.lifecycle == WorkflowRunLifecycle::Terminal {
        return Some(rejected_response(
            "terminalWorkflow",
            state,
            Vec::new(),
            vec!["Use compile to start a new lineage after a terminal run.".to_string()],
        ));
    }
    None
}

fn validate_completion(
    state: &WorkflowSessionState,
    reason: &str,
    completion: &WorkflowCompletionInput,
) -> Option<WorkflowStateResponse> {
    let mut issues = Vec::new();
    if reason.trim().is_empty() || reason.len() > MAX_TRANSITION_REASON_BYTES {
        issues.push(WorkflowValidationIssue {
            code: "invalidReason",
            path: "reason".to_string(),
            message: format!(
                "reason must be non-empty and at most {MAX_TRANSITION_REASON_BYTES} bytes"
            ),
        });
    }
    if completion.summary.trim().is_empty()
        || completion.summary.len() > MAX_COMPLETION_SUMMARY_BYTES
    {
        issues.push(WorkflowValidationIssue {
            code: "invalidSummary",
            path: "completion.summary".to_string(),
            message: format!(
                "summary must be non-empty and at most {MAX_COMPLETION_SUMMARY_BYTES} bytes"
            ),
        });
    }
    if completion.evidence.len() > MAX_COMPLETION_EVIDENCE {
        issues.push(WorkflowValidationIssue {
            code: "tooManyEvidenceItems",
            path: "completion.evidence".to_string(),
            message: format!("evidence may contain at most {MAX_COMPLETION_EVIDENCE} items"),
        });
    }
    (!issues.is_empty()).then(|| {
        rejected_response(
            "invalidDefinition",
            state,
            issues,
            vec!["Correct the completion record and retry the transition.".to_string()],
        )
    })
}

fn lifecycle_for_stage(stage: &WorkflowStage) -> WorkflowRunLifecycle {
    if stage.terminal {
        WorkflowRunLifecycle::Terminal
    } else {
        WorkflowRunLifecycle::Active
    }
}

fn commit_receipt(
    state: &mut WorkflowSessionState,
    next_revision: u64,
    operation_id: &str,
    argument_hash: &str,
    _context: &ToolCallContext,
) {
    state.revision = next_revision;
    state.operation_receipts.push(WorkflowOperationReceipt {
        operation_id: operation_id.to_string(),
        argument_hash: argument_hash.to_string(),
        operation_revision: next_revision,
    });
    let drain = state
        .operation_receipts
        .len()
        .saturating_sub(MAX_WORKFLOW_OPERATION_RECEIPTS);
    state.operation_receipts.drain(..drain);
}

fn trim_history(run: &mut WorkflowRun) -> Result<(), PureError> {
    let drain = run.history_tail.len().saturating_sub(MAX_WORKFLOW_HISTORY);
    for record in run.history_tail.drain(..drain) {
        let encoded = serde_json::to_vec(&record)?;
        run.archived_transition_digest = crate::canonical_content_hash(
            [
                run.archived_transition_digest.as_bytes(),
                encoded.as_slice(),
            ]
            .concat()
            .as_slice(),
        );
        run.archived_transition_count = run.archived_transition_count.saturating_add(1);
    }
    Ok(())
}

fn archive_run(
    state: &mut WorkflowSessionState,
    run: WorkflowRun,
    outcome: &str,
    summary: &str,
) -> Result<(), PureError> {
    state.archived_runs.push(WorkflowRunArchive {
        lineage_id: run.lineage_id,
        run_id: run.run_id,
        title: run.definition.title,
        definition_hash: run.definition_hash,
        final_stage_id: run.current_stage_id,
        outcome: outcome.to_string(),
        summary: summary.to_string(),
        archived_at: unix_seconds(),
    });
    while state.archived_runs.len() > MAX_ARCHIVED_WORKFLOW_RUNS {
        let archived = state.archived_runs.remove(0);
        let encoded = serde_json::to_vec(&archived)?;
        state.archived_run_digest = crate::canonical_content_hash(
            [state.archived_run_digest.as_bytes(), encoded.as_slice()]
                .concat()
                .as_slice(),
        );
        state.archived_run_count = state.archived_run_count.saturating_add(1);
    }
    Ok(())
}

fn generated_id(prefix: &str, seed: &str) -> String {
    let hash = crate::canonical_content_hash(seed.as_bytes());
    format!("{prefix}-{}", &hash[7..31])
}

fn accepted_response(
    code: &'static str,
    state: &WorkflowSessionState,
    snapshot: serde_json::Value,
) -> WorkflowStateResponse {
    WorkflowStateResponse {
        accepted: true,
        code,
        operation_revision: state.revision,
        snapshot,
        constraint_prompt: constraint_prompt(state),
        validation_issues: Vec::new(),
        recovery_actions: Vec::new(),
    }
}

fn rejected_response(
    code: &'static str,
    state: &WorkflowSessionState,
    validation_issues: Vec<WorkflowValidationIssue>,
    recovery_actions: Vec<String>,
) -> WorkflowStateResponse {
    WorkflowStateResponse {
        accepted: false,
        code,
        operation_revision: state.revision,
        snapshot: current_snapshot(state),
        constraint_prompt: constraint_prompt(state),
        validation_issues,
        recovery_actions,
    }
}

fn cas_rejection(code: &'static str, state: &WorkflowSessionState) -> WorkflowStateResponse {
    rejected_response(
        code,
        state,
        Vec::new(),
        vec!["Read the canonical snapshot and retry with its run ID, revision, and current stage ID.".to_string()],
    )
}

fn snapshot(state: &WorkflowSessionState, view: WorkflowStatusView) -> serde_json::Value {
    match view {
        WorkflowStatusView::Current => current_snapshot(state),
        WorkflowStatusView::Graph => state.current_run.as_ref().map_or_else(
            || serde_json::json!({ "revision": state.revision, "currentRun": null }),
            |run| {
                serde_json::json!({
                    "revision": state.revision,
                    "runId": run.run_id,
                    "definitionHash": run.definition_hash,
                    "definition": run.definition,
                })
            },
        ),
        WorkflowStatusView::History => state.current_run.as_ref().map_or_else(
            || serde_json::json!({ "revision": state.revision, "currentRun": null }),
            |run| {
                serde_json::json!({
                    "revision": state.revision,
                    "runId": run.run_id,
                    "historyTail": run.history_tail,
                    "archivedTransitionCount": run.archived_transition_count,
                    "archivedTransitionDigest": run.archived_transition_digest,
                    "archivedRuns": state.archived_runs,
                    "archivedRunCount": state.archived_run_count,
                    "archivedRunDigest": state.archived_run_digest,
                })
            },
        ),
    }
}

fn current_snapshot(state: &WorkflowSessionState) -> serde_json::Value {
    let Some(run) = &state.current_run else {
        return serde_json::json!({
            "revision": 0,
            "currentRun": null,
            "instruction": "Call workflow_state with action compile, expectedRevision 0, omit expectedRunId, and provide the complete workflow definition and transition graph before doing stage work."
        });
    };
    let allowed_transitions = run
        .definition
        .transitions
        .iter()
        .filter(|transition| transition.from_stage_id == run.current_stage_id)
        .map(|transition| {
            serde_json::json!({
                "toStageId": transition.to_stage_id,
                "when": transition.when,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "revision": state.revision,
        "runId": run.run_id,
        "lineageId": run.lineage_id,
        "definitionHash": run.definition_hash,
        "modeId": run.mode.mode_id,
        "lifecycle": run.lifecycle,
        "currentStageId": run.current_stage_id,
        "currentStage": run.definition.stages.iter().find(|stage| stage.id == run.current_stage_id),
        "allowedTransitions": allowed_transitions,
        "updatedAt": run.updated_at,
    })
}

fn constraint_prompt(state: &WorkflowSessionState) -> String {
    let Some(run) = &state.current_run else {
        return "Compile a complete stage graph before starting work.".to_string();
    };
    if run.lifecycle == WorkflowRunLifecycle::Terminal {
        return format!(
            "Workflow run {} is terminal at `{}`; do not mutate it.",
            run.run_id, run.current_stage_id
        );
    }
    let stage = run
        .definition
        .stages
        .iter()
        .find(|stage| stage.id == run.current_stage_id)
        .expect("compiled run contains current stage");
    format!(
        "Current stage `{}`: {} Complete its criteria, then use one direct outgoing transition with runId `{}` and revision {}.",
        stage.id, stage.instructions, run.run_id, state.revision
    )
}

#[cfg(test)]
mod tests;
