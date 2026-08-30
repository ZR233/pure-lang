use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use pl_protocol::OutputStream;
use pl_trace::{
    AgentEvent, AgentEventSender, TraceDelta, TraceEventDraft, TraceEventKind, TraceEventSink,
    TraceEventSinkError, TracePartDeltaEvent,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

use crate::turn::{InteractionCallback, PermissionMode, UserInputMode};

/// Agent workspace 是否允许宿主权限策略访问 root 之外的路径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceBoundary {
    #[default]
    Confined,
    HostPermitted,
}

impl WorkspaceBoundary {
    fn allows_host_paths(self) -> bool {
        matches!(self, Self::HostPermitted)
    }
}

/// Agent workspace 的修改能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceMutability {
    ReadOnly,
    #[default]
    ReadWrite,
}

/// 单个 Agent 的 canonical workspace 边界。
///
/// 宿主负责根据 durable owner 构造该值；所有内置路径工具、命令 cwd、Git、LSP 与项目
/// skills 必须消费同一个 root。`Confined` 不会被 turn 的 `full-access` 权限放宽。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentWorkspace {
    root: PathBuf,
    boundary: WorkspaceBoundary,
    mutability: WorkspaceMutability,
}

impl AgentWorkspace {
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            boundary: WorkspaceBoundary::HostPermitted,
            mutability: WorkspaceMutability::ReadWrite,
        }
    }

    pub fn confined(root: impl Into<PathBuf>, mutability: WorkspaceMutability) -> Self {
        Self {
            root: root.into(),
            boundary: WorkspaceBoundary::Confined,
            mutability,
        }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn boundary(&self) -> WorkspaceBoundary {
        self.boundary
    }

    pub fn mutability(&self) -> WorkspaceMutability {
        self.mutability
    }
}

/// Runtime coordination policy for tools within one model response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRuntimeLockPolicy {
    Exclusive,
    Shared,
    None,
}

/// 同一个 provider response 内工具调用的组合策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolBatchPolicy {
    /// 可与其他调用共存，实际并行度仍由 runtime lock policy 决定。
    #[default]
    Coexist,
    /// 必须是本批唯一调用；违反时整批拒绝且不执行任何工具。
    Solo,
}

/// 一次工具调用的稳定身份。
///
/// 这些字段由 dispatcher 从冻结的 model step 与 provider call 构造。工具不得自行
/// 猜测或重写这些身份；动态注册的外部工具也可依赖它们生成审计事件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCallIdentity {
    pub call_id: String,
    pub item_id: String,
    pub agent_id: String,
    pub parent_agent_id: Option<String>,
    pub agent_path: Option<String>,
    pub agent_role: String,
    pub agent_depth: u32,
    pub session_id: String,
    pub turn_id: String,
    pub step: u32,
    pub started_sequence: u64,
    pub revision_base: u64,
}

/// 工具运行期输出的 canonical delta 出口。
///
/// sink 先提交 authoritative trace，再向非权威观察通道广播；revision 由调用身份和
/// 输出顺序共同决定，工具无需接触内部 sender 或自行拼装 timeline 事件。
#[derive(Clone)]
pub struct ToolOutputDeltaEmitter {
    event_tx: tokio::sync::broadcast::WeakSender<AgentEvent>,
    trace_sink: Option<Arc<dyn TraceEventSink>>,
    identity: ToolCallIdentity,
    next_revision: Arc<AtomicU64>,
    last_error: Arc<StdMutex<Option<TraceEventSinkError>>>,
}

impl fmt::Debug for ToolOutputDeltaEmitter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolOutputDeltaEmitter")
            .field("item_id", &self.identity.item_id)
            .field("trace_sink", &self.trace_sink.as_ref().map(|_| "<sink>"))
            .finish_non_exhaustive()
    }
}

impl ToolOutputDeltaEmitter {
    fn new(
        identity: ToolCallIdentity,
        event_tx: &AgentEventSender,
        trace_sink: Option<Arc<dyn TraceEventSink>>,
    ) -> Self {
        Self {
            event_tx: event_tx.downgrade(),
            trace_sink,
            next_revision: Arc::new(AtomicU64::new(identity.revision_base)),
            identity,
            last_error: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn emit(
        &self,
        stream: OutputStream,
        text: impl Into<String>,
    ) -> Result<(), TraceEventSinkError> {
        let revision = self
            .next_revision
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        self.emit_at(stream, text, revision)
    }

    pub(crate) fn emit_at(
        &self,
        stream: OutputStream,
        text: impl Into<String>,
        revision: u64,
    ) -> Result<(), TraceEventSinkError> {
        self.next_revision.fetch_max(revision, Ordering::Relaxed);
        let mut delta = text.into();
        if matches!(stream, OutputStream::Stderr) {
            delta = format!("[stderr] {delta}");
        }
        let timestamp = crate::time::unix_seconds();
        let event = TracePartDeltaEvent {
            turn_id: self.identity.turn_id.clone(),
            item_id: self.identity.item_id.clone(),
            started_sequence: self.identity.started_sequence,
            revision,
            created_at: timestamp,
            updated_at: timestamp,
            delta: TraceDelta::ToolResult { delta },
        };
        if let Some(sink) = &self.trace_sink
            && let Err(error) = sink.emit(TraceEventDraft::new(
                timestamp,
                TraceEventKind::TracePartDelta {
                    event: event.clone(),
                },
            ))
        {
            if let Ok(mut slot) = self.last_error.lock() {
                *slot = Some(error.clone());
            }
            return Err(error);
        }
        if let Some(event_tx) = self.event_tx.upgrade() {
            let _ = event_tx.send(AgentEvent::TracePartDelta { event });
        }
        Ok(())
    }

    fn take_error(&self) -> Option<TraceEventSinkError> {
        self.last_error.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// Dispatcher 已经裁决后的调用级审批与宿主交互能力。
///
/// 它不携带完整 [`crate::TurnOptions`]。具体工具只能读取本次调用获批的路径能力，
/// 或使用宿主显式提供的 interaction callback。
#[derive(Clone)]
pub struct ToolApprovalContext {
    permission_mode: PermissionMode,
    workspace_access: WorkspaceAccess,
    interaction_callback: Option<InteractionCallback>,
    user_input_mode: UserInputMode,
}

impl ToolApprovalContext {
    pub fn new(permission_mode: PermissionMode, workspace_access: WorkspaceAccess) -> Self {
        Self {
            permission_mode,
            workspace_access,
            interaction_callback: None,
            user_input_mode: UserInputMode::AwaitResponse,
        }
    }

    pub fn with_interaction(
        mut self,
        callback: Option<InteractionCallback>,
        mode: UserInputMode,
    ) -> Self {
        self.interaction_callback = callback;
        self.user_input_mode = mode;
        self
    }

    pub fn permission_mode(&self) -> PermissionMode {
        self.permission_mode
    }

    pub fn workspace_access(&self) -> WorkspaceAccess {
        self.workspace_access
    }

    pub fn interaction_callback(&self) -> Option<InteractionCallback> {
        self.interaction_callback.clone()
    }

    pub fn user_input_mode(&self) -> UserInputMode {
        self.user_input_mode
    }
}

impl fmt::Debug for ToolApprovalContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolApprovalContext")
            .field("permission_mode", &self.permission_mode)
            .field("workspace_access", &self.workspace_access)
            .field(
                "interaction_callback",
                &self.interaction_callback.as_ref().map(|_| "<callback>"),
            )
            .field("user_input_mode", &self.user_input_mode)
            .finish()
    }
}

/// 单次工具调用的最小运行时上下文。
///
/// 这里只保留调用身份、取消、审批/交互能力和事件出口。workspace backend、LSP、
/// product session 与 working set 等依赖必须由具体 [`crate::Tool`] 在构造时捕获。
#[derive(Clone)]
pub struct ToolCallContext {
    identity: ToolCallIdentity,
    cancellation_token: Option<CancellationToken>,
    approval: ToolApprovalContext,
    event_tx: AgentEventSender,
    output: ToolOutputDeltaEmitter,
}

impl ToolCallContext {
    pub fn new(identity: ToolCallIdentity, event_tx: AgentEventSender) -> Self {
        let output = ToolOutputDeltaEmitter::new(identity.clone(), &event_tx, None);
        Self {
            identity,
            cancellation_token: None,
            approval: ToolApprovalContext::new(
                PermissionMode::RequestApproval,
                WorkspaceAccess::WorkspaceOnly,
            ),
            event_tx,
            output,
        }
    }

    pub fn with_trace_sink(mut self, trace_sink: Option<Arc<dyn TraceEventSink>>) -> Self {
        self.output =
            ToolOutputDeltaEmitter::new(self.identity.clone(), &self.event_tx, trace_sink);
        self
    }

    pub fn with_cancellation(mut self, cancellation_token: Option<CancellationToken>) -> Self {
        self.cancellation_token = cancellation_token;
        self
    }

    pub fn with_approval(mut self, approval: ToolApprovalContext) -> Self {
        self.approval = approval;
        self
    }

    pub fn identity(&self) -> &ToolCallIdentity {
        &self.identity
    }

    pub fn cancellation_token(&self) -> Option<CancellationToken> {
        self.cancellation_token.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub fn approval(&self) -> &ToolApprovalContext {
        &self.approval
    }

    pub fn events(&self) -> &AgentEventSender {
        &self.event_tx
    }

    /// 立即发布一段工具运行输出。
    pub fn emit_output_delta(
        &self,
        stream: OutputStream,
        text: impl Into<String>,
    ) -> Result<(), TraceEventSinkError> {
        self.output.emit(stream, text)
    }

    pub(crate) fn output_delta_emitter(&self) -> ToolOutputDeltaEmitter {
        self.output.clone()
    }

    pub(crate) fn take_output_delta_error(&self) -> Option<TraceEventSinkError> {
        self.output.take_error()
    }

    #[cfg(test)]
    pub(crate) fn test(event_tx: AgentEventSender) -> Self {
        Self::new(
            ToolCallIdentity {
                call_id: "call-1".to_string(),
                item_id: "call-1".to_string(),
                agent_id: "agent-1".to_string(),
                parent_agent_id: None,
                agent_path: Some("/root".to_string()),
                agent_role: "root".to_string(),
                agent_depth: 0,
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                step: 0,
                started_sequence: 0,
                revision_base: 0,
            },
            event_tx,
        )
    }
}

/// 单次工具调用的路径访问策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceAccess {
    #[default]
    WorkspaceOnly,
    ExternalAllowed,
}

impl WorkspaceAccess {
    pub fn allows_external(self) -> bool {
        matches!(self, Self::ExternalAllowed)
    }
}

/// 当前工具调用所在的 subagent 运行边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentContext {
    pub id: String,
    pub parent_id: Option<String>,
    pub agent_path: Option<String>,
    pub role: String,
    pub task: String,
    pub depth: u32,
}

impl fmt::Debug for ToolCallContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolCallContext")
            .field("identity", &self.identity)
            .field(
                "cancellation_token",
                &self.cancellation_token.as_ref().map(|_| "<token>"),
            )
            .field("approval", &self.approval)
            .finish_non_exhaustive()
    }
}

/// 由 workspace 类工具在注册时捕获的稳定运行时依赖。
///
/// 该对象统一持有 agent workspace、LSP 通知出口与写入串行化边界；调用级批准仍从
/// [`ToolCallContext`] 读取，因此旧 plan 不会看到后续 workspace replacement。
#[derive(Clone)]
pub struct ToolWorkspace {
    workspace: AgentWorkspace,
    lsp_runtime: Option<pl_lsp::LspRuntimeRegistry>,
}

impl ToolWorkspace {
    pub fn new(workspace: AgentWorkspace) -> Self {
        Self {
            workspace,
            lsp_runtime: None,
        }
    }

    pub fn with_lsp_runtime(mut self, runtime: Option<pl_lsp::LspRuntimeRegistry>) -> Self {
        self.lsp_runtime = runtime;
        self
    }

    pub fn workspace(&self) -> &AgentWorkspace {
        &self.workspace
    }

    pub fn root(&self) -> &std::path::Path {
        self.workspace.root()
    }

    pub fn allows_workspace_escape(&self, context: &ToolCallContext) -> bool {
        self.workspace.boundary.allows_host_paths()
            && (context.approval.permission_mode.allows_workspace_escape()
                || context.approval.workspace_access.allows_external())
    }

    pub fn ensure_workspace_writable(&self) -> crate::Result<()> {
        if self.workspace.mutability == WorkspaceMutability::ReadOnly {
            return Err(crate::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "agent workspace is read-only".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn write_lock(&self) -> WorkspaceWriteGuard {
        workspace_write_locks()
            .lock_for(self.workspace.root())
            .await
    }

    pub(crate) async fn notify_changed(&self, path: &std::path::Path) {
        if let Some(runtime) = &self.lsp_runtime {
            runtime.notify_file_changed(path).await;
        }
    }

    pub(crate) async fn notify_deleted(&self, path: &std::path::Path) {
        if let Some(runtime) = &self.lsp_runtime {
            runtime.notify_file_deleted(path).await;
        }
    }

    pub(crate) fn lsp_runtime(&self) -> Option<pl_lsp::LspRuntimeRegistry> {
        self.lsp_runtime.clone()
    }
}

impl fmt::Debug for ToolWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolWorkspace")
            .field("workspace", &self.workspace)
            .field("lsp_runtime", &self.lsp_runtime.is_some())
            .finish()
    }
}

/// 由需要会话数据的具体工具捕获的 per-agent session runtime。
///
/// `TurnEngine` 在每个 turn 开始时绑定新的 canonical working set，并在每次工具批次
/// 前刷新只读 session 快照。普通工具不会通过 [`ToolCallContext`] 获得产品会话对象。
#[derive(Clone)]
pub struct ToolSessionRuntime {
    parent_session: Arc<std::sync::RwLock<Arc<crate::session::AgentSession>>>,
    working_set: crate::TurnWorkingSetHandle,
}

impl Default for ToolSessionRuntime {
    fn default() -> Self {
        Self {
            parent_session: Arc::new(std::sync::RwLock::new(Arc::new(
                crate::session::AgentSession::new(),
            ))),
            working_set: crate::TurnWorkingSetHandle::default(),
        }
    }
}

impl ToolSessionRuntime {
    pub fn parent_session(&self) -> Arc<crate::session::AgentSession> {
        self.parent_session
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn working_set(&self) -> crate::TurnWorkingSetHandle {
        self.working_set.clone()
    }

    pub(crate) fn begin_turn(&self, session: &crate::session::AgentSession) -> crate::Result<()> {
        self.working_set.reset_from_session(session)?;
        self.update_parent_session(Arc::new(session.clone()));
        Ok(())
    }

    pub(crate) fn update_parent_session(&self, session: Arc<crate::session::AgentSession>) {
        *self
            .parent_session
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = session;
    }
}

impl fmt::Debug for ToolSessionRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolSessionRuntime")
            .field("working_set", &self.working_set)
            .finish_non_exhaustive()
    }
}

type WorkspaceWriteGuard = OwnedMutexGuard<()>;

#[derive(Default)]
struct WorkspaceWriteLocks {
    locks: std::sync::Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl WorkspaceWriteLocks {
    async fn lock_for(&self, workspace_root: &std::path::Path) -> WorkspaceWriteGuard {
        let key =
            std::fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
        let lock = {
            let mut locks = self.locks.lock().unwrap_or_else(|poisoned| {
                tracing::warn!("workspace write lock was poisoned, recovering");
                poisoned.into_inner()
            });
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        lock.lock_owned().await
    }
}

fn workspace_write_locks() -> &'static WorkspaceWriteLocks {
    static LOCKS: OnceLock<WorkspaceWriteLocks> = OnceLock::new();
    LOCKS.get_or_init(WorkspaceWriteLocks::default)
}
