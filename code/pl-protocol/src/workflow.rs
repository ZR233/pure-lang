//! Thread Mode 使用的扁平确定性状态图协议。

use serde::{Deserialize, Serialize};

use crate::ThreadModeId;

/// 一个 state 在状态机中的语义种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowStateKind {
    Atomic,
    Final,
}

/// 状态图中的一个 state。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowState {
    pub id: String,
    pub title: String,
    pub instructions: String,
    #[serde(default)]
    pub completion_criteria: Vec<String>,
    pub kind: WorkflowStateKind,
}

/// 一条有向 state transition。
///
/// `guard` 是由 Agent 判断的声明性自然语言条件；Runtime 不执行表达式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTransition {
    pub source_state_id: String,
    pub target_state_id: String,
    pub guard: String,
}

/// 注册进 Thread Mode Manager 的完整状态图定义。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub title: String,
    pub goal: String,
    pub initial_state_id: String,
    #[serde(default)]
    pub states: Vec<WorkflowState>,
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

/// 一次已提交的状态转换事实。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTransitionRecord {
    pub revision: u64,
    pub source_state_id: String,
    pub target_state_id: String,
    pub reason: String,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub turn_id: String,
    pub call_id: String,
    pub transitioned_at: i64,
}

/// 与 Agent working state 一起保存的轻量 run。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    pub lineage_id: String,
    pub run_id: String,
    pub mode_id: ThreadModeId,
    pub graph_revision: u64,
    pub graph_hash: String,
    pub lifecycle: WorkflowRunLifecycle,
    pub current_state_id: String,
    pub started_at: i64,
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
    pub mode_id: ThreadModeId,
    pub graph_revision: u64,
    pub graph_hash: String,
    pub final_state_id: String,
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

/// Thread stream 向 GUI 暴露的轻量当前 run 投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRuntimeRunSnapshot {
    pub lineage_id: String,
    pub run_id: String,
    pub mode_id: ThreadModeId,
    pub graph_revision: u64,
    pub graph_hash: String,
    pub lifecycle: WorkflowRunLifecycle,
    pub current_state_id: String,
    pub started_at: i64,
    pub updated_at: i64,
}

/// Thread stream 向 GUI 暴露的当前工作流投影。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRuntimeSnapshot {
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_run: Option<WorkflowRuntimeRunSnapshot>,
}

impl From<&WorkflowSessionState> for WorkflowRuntimeSnapshot {
    fn from(state: &WorkflowSessionState) -> Self {
        Self {
            revision: state.revision,
            current_run: state
                .current_run
                .as_ref()
                .map(|run| WorkflowRuntimeRunSnapshot {
                    lineage_id: run.lineage_id.clone(),
                    run_id: run.run_id.clone(),
                    mode_id: run.mode_id.clone(),
                    graph_revision: run.graph_revision,
                    graph_hash: run.graph_hash.clone(),
                    lifecycle: run.lifecycle,
                    current_state_id: run.current_state_id.clone(),
                    started_at: run.started_at,
                    updated_at: run.updated_at,
                }),
        }
    }
}
