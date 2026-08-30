//! 可由 Mode Skill 编译的通用工作流协议。

use serde::{Deserialize, Serialize};

/// 工作流编译时冻结的模式指令。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeInstructionSnapshot {
    pub mode_id: String,
    pub display_name: String,
    pub source: String,
    pub provider_id: String,
    pub revision: String,
    pub content_hash: String,
    pub content: String,
}

/// 一个可编译阶段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStage {
    pub id: String,
    pub title: String,
    pub instructions: String,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    #[serde(default)]
    pub terminal: bool,
}

/// 一条有向阶段转换。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTransition {
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub when: String,
}

/// Mode Skill 提交给编译器的完整定义。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub title: String,
    pub goal: String,
    pub initial_stage_id: String,
    #[serde(default)]
    pub stages: Vec<WorkflowStage>,
    #[serde(default)]
    pub transitions: Vec<WorkflowTransition>,
}

/// 当前 run 的主生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowRunLifecycle {
    Active,
    Terminal,
}

/// 一次已提交的阶段完成事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTransitionRecord {
    pub revision: u64,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub reason: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub turn_id: String,
    pub call_id: String,
    pub transitioned_at: i64,
}

/// 当前完整 run。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub lineage_id: String,
    pub run_id: String,
    pub definition: WorkflowDefinition,
    pub definition_hash: String,
    pub mode: ModeInstructionSnapshot,
    pub lifecycle: WorkflowRunLifecycle,
    pub current_stage_id: String,
    pub compiled_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub history_tail: Vec<WorkflowTransitionRecord>,
    #[serde(default)]
    pub archived_transition_count: u64,
    #[serde(default)]
    pub archived_transition_digest: String,
}

/// 已从热状态归档的 run 摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunArchive {
    pub lineage_id: String,
    pub run_id: String,
    pub title: String,
    pub definition_hash: String,
    pub final_stage_id: String,
    pub outcome: String,
    pub summary: String,
    pub archived_at: i64,
}

/// 最近一次成功 mutation 的幂等凭据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowOperationReceipt {
    pub operation_id: String,
    pub argument_hash: String,
    pub operation_revision: u64,
}

/// 与 canonical Agent session 一起保存的工作流状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSessionState {
    #[serde(default)]
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_run: Option<WorkflowRun>,
    #[serde(default)]
    pub archived_runs: Vec<WorkflowRunArchive>,
    #[serde(default)]
    pub archived_run_count: u64,
    #[serde(default)]
    pub archived_run_digest: String,
    #[serde(default)]
    pub operation_receipts: Vec<WorkflowOperationReceipt>,
}

/// Thread stream 向 GUI 暴露的当前工作流投影。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRuntimeSnapshot {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_run: Option<WorkflowRun>,
}

impl From<&WorkflowSessionState> for WorkflowRuntimeSnapshot {
    fn from(state: &WorkflowSessionState) -> Self {
        Self {
            revision: state.revision,
            current_run: state.current_run.clone(),
        }
    }
}
