//! 拥有所有权与静态内置 Thread Mode 注册描述。

use pl_protocol::{
    ThreadModeId, WorkflowDefinition, WorkflowState, WorkflowStateKind, WorkflowTransition,
};

/// 外部 loader 与 Studio 内置注册共同提交给 Manager 的输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadModeRegistration {
    pub id: ThreadModeId,
    pub display_name: String,
    pub description: String,
    pub order: u32,
    pub prompt: String,
    pub workflow: Option<WorkflowDefinition>,
}

/// 可在 Rust 数据段中保存的内置 Mode 描述。
#[derive(Debug, Clone, Copy)]
pub struct StaticThreadModeRegistration {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub order: u32,
    pub prompt: &'static str,
    pub workflow: Option<StaticWorkflowDefinition>,
}

impl StaticThreadModeRegistration {
    /// 把随二进制发布的静态描述转换为统一拥有所有权的注册输入。
    ///
    /// # Errors
    ///
    /// 内置 ID 不符合 `ThreadModeId` wire 约束时返回错误。
    pub fn to_registration(self) -> Result<ThreadModeRegistration, pl_protocol::UnknownLabelError> {
        Ok(ThreadModeRegistration {
            id: ThreadModeId::new(self.id)?,
            display_name: self.display_name.to_string(),
            description: self.description.to_string(),
            order: self.order,
            prompt: self.prompt.to_string(),
            workflow: self.workflow.map(Into::into),
        })
    }
}

/// 可由静态切片完整表达的状态图。
#[derive(Debug, Clone, Copy)]
pub struct StaticWorkflowDefinition {
    pub title: &'static str,
    pub goal: &'static str,
    pub initial_state_id: &'static str,
    pub states: &'static [StaticWorkflowState],
    pub transitions: &'static [StaticWorkflowTransition],
}

impl From<StaticWorkflowDefinition> for WorkflowDefinition {
    fn from(value: StaticWorkflowDefinition) -> Self {
        Self {
            title: value.title.to_string(),
            goal: value.goal.to_string(),
            initial_state_id: value.initial_state_id.to_string(),
            states: value.states.iter().copied().map(Into::into).collect(),
            transitions: value.transitions.iter().copied().map(Into::into).collect(),
        }
    }
}

/// 一个静态 state。
#[derive(Debug, Clone, Copy)]
pub struct StaticWorkflowState {
    pub id: &'static str,
    pub title: &'static str,
    pub instructions: &'static str,
    pub completion_criteria: &'static [&'static str],
    pub kind: WorkflowStateKind,
}

impl From<StaticWorkflowState> for WorkflowState {
    fn from(value: StaticWorkflowState) -> Self {
        Self {
            id: value.id.to_string(),
            title: value.title.to_string(),
            instructions: value.instructions.to_string(),
            completion_criteria: value
                .completion_criteria
                .iter()
                .map(|criterion| (*criterion).to_string())
                .collect(),
            kind: value.kind,
        }
    }
}

/// 一条静态 transition。
#[derive(Debug, Clone, Copy)]
pub struct StaticWorkflowTransition {
    pub source_state_id: &'static str,
    pub target_state_id: &'static str,
    pub guard: &'static str,
}

impl From<StaticWorkflowTransition> for WorkflowTransition {
    fn from(value: StaticWorkflowTransition) -> Self {
        Self {
            source_state_id: value.source_state_id.to_string(),
            target_state_id: value.target_state_id.to_string(),
            guard: value.guard.to_string(),
        }
    }
}
