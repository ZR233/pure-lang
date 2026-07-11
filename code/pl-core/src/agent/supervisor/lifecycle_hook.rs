use std::pin::Pin;

use pl_protocol::{AgentStatus, PureError};

use crate::agent::worktree::WorktreeCreateSpec;

/// 宿主在 agent id 分配后收到的通用 spawn 生命周期请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnLifecycleRequest {
    pub agent_id: String,
    pub agent_path: String,
    pub owner_path: String,
    pub session_id: String,
    pub task_name: String,
    pub role: String,
    pub owned_paths: Vec<String>,
    pub requested_by_call_id: String,
}

/// 宿主为本次 spawn 固定的资源准备结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSpawnPreparation {
    worktree: Option<WorktreeCreateSpec>,
    lifecycle_token: Option<String>,
}

impl AgentSpawnPreparation {
    pub fn without_worktree() -> Self {
        Self {
            worktree: None,
            lifecycle_token: None,
        }
    }

    pub fn with_worktree(worktree: WorktreeCreateSpec) -> Self {
        Self {
            worktree: Some(worktree),
            lifecycle_token: None,
        }
    }

    pub fn with_token(lifecycle_token: impl Into<String>) -> Self {
        Self {
            worktree: None,
            lifecycle_token: Some(lifecycle_token.into()),
        }
    }

    pub fn with_worktree_and_token(
        worktree: WorktreeCreateSpec,
        lifecycle_token: impl Into<String>,
    ) -> Self {
        Self {
            worktree: Some(worktree),
            lifecycle_token: Some(lifecycle_token.into()),
        }
    }

    pub(super) fn worktree(&self) -> Option<&WorktreeCreateSpec> {
        self.worktree.as_ref()
    }

    pub(crate) fn lifecycle_token(&self) -> Option<&str> {
        self.lifecycle_token.as_deref()
    }
}

/// close 生命周期使用的明确处置分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCloseDispositionKind {
    Merge,
    Discard,
}

/// 宿主在通用 close 路径改变状态前收到的校验请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCloseLifecycleRequest {
    pub agent_id: String,
    pub agent_path: String,
    pub role: String,
    pub disposition: AgentCloseDispositionKind,
}

/// 宿主无关的 agent 状态投影，用于让持久化生命周期覆盖内存快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLifecycleProjection {
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
}

impl AgentLifecycleProjection {
    pub fn new(status: AgentStatus, summary: Option<String>, error: Option<String>) -> Self {
        Self {
            status,
            summary,
            error,
        }
    }
}

/// coordinator 消费终态事实所需的宿主无关输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTerminalStateChange {
    pub agent_id: String,
    pub role: String,
    pub status: AgentStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
}

/// 生命周期 hook 投影单个 supervisor 快照所需的通用输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLifecycleProjectionRequest {
    pub lifecycle_token: String,
    pub role: String,
    pub snapshot: AgentLifecycleProjection,
}

/// 宿主围绕 supervisor spawn/close 原子边界实现的生命周期 hook。
///
/// `prepare_spawn` 应预留并持久化资源，`activate_spawn` 在 child turn 启动前提交
/// Running 状态，`rollback_spawn` 必须撤销已准备的事实。实现不得自行启动 agent turn。
pub trait AgentLifecycleHook: std::fmt::Debug + Send + Sync {
    fn prepare_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<AgentSpawnPreparation, PureError>> + Send + 'a>,
    >;

    fn activate_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        preparation: &'a AgentSpawnPreparation,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>>;

    fn rollback_spawn<'a>(
        &'a self,
        request: &'a AgentSpawnLifecycleRequest,
        preparation: &'a AgentSpawnPreparation,
        error: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>>;

    fn validate_close<'a>(
        &'a self,
        request: &'a AgentCloseLifecycleRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), PureError>> + Send + 'a>>;

    fn project_snapshot<'a>(
        &'a self,
        request: &'a AgentLifecycleProjectionRequest,
    ) -> Pin<
        Box<
            dyn std::future::Future<Output = Result<AgentLifecycleProjection, PureError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(request.snapshot.clone()) })
    }
}
