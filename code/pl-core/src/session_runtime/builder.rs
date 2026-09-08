use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::agent_runtime::agent_loop::AgentLoopHandle;
use crate::{
    AgentIdentity, AgentRuntimeHandle, AgentSession, AgentToolSet, BeforeModelStepHook,
    GlobalToolInheritance, ToolGroupId, ToolInstallGroup, ToolManager, ToolSessionRuntime,
};

use super::{
    SessionControl, SessionEventSource, SessionEventSubscription, SessionMessageSender,
    SessionSourceError,
};

/// Immutable creation context. Capabilities address this actor incarnation only.
#[derive(Clone)]
pub struct SessionBuildContext {
    pub(crate) identity: AgentIdentity,
    pub(crate) session: AgentSession,
    pub(crate) runtime: AgentRuntimeHandle,
    pub(crate) actor: AgentLoopHandle,
    pub(crate) cancellation: CancellationToken,
    pub(crate) tool_session: ToolSessionRuntime,
    pub(crate) source: String,
}

impl SessionBuildContext {
    /// Returns the identity frozen for this creation attempt.
    pub fn identity(&self) -> &AgentIdentity {
        &self.identity
    }
    /// Returns the initial canonical session, not a mutable owner reference.
    pub fn session(&self) -> &AgentSession {
        &self.session
    }
    /// Returns host runtime capabilities bound to this creation attempt.
    ///
    /// RPCs return `NotReady` until the owner starts, and fail after this session closes.
    /// Constructing adapters is allowed here; awaiting owner work during initialization is not.
    pub fn runtime(&self) -> &AgentRuntimeHandle {
        &self.runtime
    }
    /// Returns state-tool bindings shared by every Turn of this session.
    pub fn tool_session_runtime(&self) -> ToolSessionRuntime {
        self.tool_session.clone()
    }
    /// Returns cancellation for the session, independent of any Turn.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
    /// Returns controls bound directly to this owner incarnation.
    pub fn control(&self) -> SessionControl {
        SessionControl::new(self.actor.clone(), self.identity.id.clone())
    }
    /// Returns a publisher attributed to the currently registered factory or source.
    ///
    /// # Errors
    /// Rejects invalid registration source identities.
    pub fn messages(&self) -> Result<SessionMessageSender, super::SessionMessageError> {
        SessionMessageSender::new(self.actor.clone(), self.source.clone())
    }
    fn for_source(&self, source: &str) -> Self {
        let mut context = self.clone();
        context.source = source.to_owned();
        context
    }
}

impl std::fmt::Debug for SessionBuildContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBuildContext")
            .field("identity", &self.identity)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

type ToolFactory = dyn Fn(SessionBuildContext) -> BoxFuture<'static, crate::Result<Vec<ToolInstallGroup>>>
    + Send
    + Sync;
type SourceFactory = dyn Fn(
        SessionBuildContext,
    ) -> BoxFuture<'static, Result<SessionEventSubscription, SessionSourceError>>
    + Send
    + Sync;

/// Declarations collected before session publication. There is no alternate tool registry.
#[derive(Clone, Default)]
pub struct SessionRuntimeBuilder {
    delivery_window: Option<std::time::Duration>,
    workspace: Option<super::SessionWorkspaceBinding>,
    manager: ToolManager,
    groups: Vec<ToolInstallGroup>,
    factories: Vec<(String, Arc<ToolFactory>)>,
    sources: Vec<(String, Arc<SourceFactory>)>,
    refresh: Option<BeforeModelStepHook>,
}

impl SessionRuntimeBuilder {
    /// Sets the shared response window for an admitted task batch; zero always yields handles.
    pub fn with_delivery_window(mut self, window: std::time::Duration) -> Self {
        self.delivery_window = Some(window);
        self
    }
    /// Freezes the physical scope shared by every Turn and executor.
    pub fn with_workspace(mut self, workspace: super::SessionWorkspaceBinding) -> Self {
        self.workspace = Some(workspace);
        self
    }
    /// Creates a declaration set using one shared manager's standard tool scopes.
    pub fn new(manager: ToolManager) -> Self {
        Self {
            manager,
            ..Self::default()
        }
    }
    /// Adds an already constructed ordinary installation group.
    pub fn with_tools(mut self, group: ToolInstallGroup) -> Self {
        self.groups.push(group);
        self
    }
    /// Registers a factory that binds tools to session-owned capabilities at creation.
    pub fn with_tool_factory<F, Fut>(mut self, source: impl Into<String>, factory: F) -> Self
    where
        F: Fn(SessionBuildContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = crate::Result<Vec<ToolInstallGroup>>> + Send + 'static,
    {
        self.factories.push((
            source.into(),
            Arc::new(move |context| Box::pin(factory(context))),
        ));
        self
    }
    /// Registers a source whose subscription is initialized before publication.
    pub fn with_event_source<S: SessionEventSource>(
        mut self,
        id: impl Into<String>,
        source: S,
    ) -> Self {
        let source = Arc::new(source);
        self.sources.push((
            id.into(),
            Arc::new(move |context| {
                let source = source.clone();
                Box::pin(async move { source.initialize(context).await })
            }),
        ));
        self
    }
    /// Registers the sole dynamic-directory refresh window before model-step freezing.
    pub fn with_refresh(mut self, refresh: BeforeModelStepHook) -> Self {
        self.refresh = Some(refresh);
        self
    }

    pub(crate) async fn build(
        self,
        context: SessionBuildContext,
    ) -> Result<SessionRuntime, SessionBuildError> {
        let initialization = context.cancellation.clone().drop_guard();
        let mut names = BTreeSet::new();
        for (name, _) in &self.factories {
            validate_source(name, &mut names)?;
        }
        for (name, _) in &self.sources {
            validate_source(name, &mut names)?;
        }
        let tools = self.manager.agent_tool_set(
            context.identity.id.to_string(),
            GlobalToolInheritance::Isolated,
        );
        let mut groups = self.groups;
        groups.push(
            ToolInstallGroup::direct(
                ToolGroupId::new("session-runtime"),
                super::session_control_tools(),
            )
            .with_developer_instructions(super::SESSION_TASK_INSTRUCTIONS),
        );
        for (name, factory) in self.factories {
            let result =
                std::panic::AssertUnwindSafe(async { factory(context.for_source(&name)).await })
                    .catch_unwind()
                    .await
                    .map_err(|_| SessionBuildError::Panicked {
                        source_id: name.clone(),
                    })?;
            groups.extend(result.map_err(SessionBuildError::Tools)?);
        }
        tools
            .install_batch(groups)
            .map_err(SessionBuildError::Tools)?;
        let mut initialized: Vec<(String, SessionEventSubscription)> = Vec::new();
        for (name, source) in self.sources {
            let result =
                std::panic::AssertUnwindSafe(async { source(context.for_source(&name)).await })
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        Err(SessionSourceError::new(
                            "initialize",
                            std::io::Error::other("source initialization panicked"),
                        ))
                    });
            match result {
                Ok(subscription) => initialized.push((name, subscription)),
                Err(error) => {
                    context.cancellation.cancel();
                    let mut cleanup_failure = None;
                    for (_, subscription) in initialized {
                        let cleaned = std::panic::AssertUnwindSafe(subscription.run)
                            .catch_unwind()
                            .await
                            .unwrap_or_else(|_| {
                                Err(SessionSourceError::new(
                                    "rollback",
                                    std::io::Error::other("source cleanup panicked"),
                                ))
                            });
                        if let Err(cleanup) = cleaned {
                            cleanup_failure.get_or_insert(cleanup);
                        }
                    }
                    if let Some(cleanup) = cleanup_failure {
                        return Err(SessionBuildError::Rollback {
                            source_id: name,
                            initialization: error,
                            cleanup,
                        });
                    }
                    return Err(SessionBuildError::Source {
                        source_id: name,
                        source: error,
                    });
                }
            }
        }
        Ok(SessionRuntime {
            handle: SessionRuntimeHandle {
                delivery_window: self
                    .delivery_window
                    .unwrap_or(std::time::Duration::from_secs(1)),
                workspace: self.workspace,
                tools,
                tool_session: context.tool_session,
                refresh: self.refresh,
            },
            cancellation: initialization.disarm(),
            initialized,
            sources: tokio::task::JoinSet::new(),
            source_tasks: BTreeMap::new(),
            inbox_changed: context.actor.inbox_changed(),
            tasks_changed: context.actor.tasks_changed(),
        })
    }
}

impl std::fmt::Debug for SessionRuntimeBuilder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionRuntimeBuilder")
            .field("groups", &self.groups.len())
            .field("factories", &self.factories.len())
            .field("sources", &self.sources.len())
            .finish_non_exhaustive()
    }
}

/// Session assembly errors, preserving initialization and cleanup failures.
#[derive(Debug, thiserror::Error)]
pub enum SessionBuildError {
    #[error("session factory {source_id} panicked during initialization")]
    Panicked { source_id: String },
    #[error("invalid or duplicate session source: {0}")]
    SourceIdentity(String),
    #[error("session tool installation failed")]
    Tools(#[source] crate::PureError),
    #[error("session source {source_id} initialization failed")]
    Source {
        source_id: String,
        #[source]
        source: SessionSourceError,
    },
    #[error("session source {source_id} initialization and rollback failed: {initialization}")]
    Rollback {
        source_id: String,
        initialization: SessionSourceError,
        #[source]
        cleanup: SessionSourceError,
    },
}

/// Cloneable tool bindings for preparing a Turn; physical sources stay with the owner.
#[derive(Debug, Clone)]
pub struct SessionRuntimeHandle {
    delivery_window: std::time::Duration,
    workspace: Option<super::SessionWorkspaceBinding>,
    tools: AgentToolSet,
    tool_session: ToolSessionRuntime,
    refresh: Option<BeforeModelStepHook>,
}

impl SessionRuntimeHandle {
    pub(crate) fn delivery_window(&self) -> std::time::Duration {
        self.delivery_window
    }
    /// Returns the physical execution scope declared by the host, if any.
    pub fn workspace_binding(&self) -> Option<&super::SessionWorkspaceBinding> {
        self.workspace.as_ref()
    }
    /// Returns the canonical tool scope; dynamic replacement retains old executor leases.
    pub fn tools(&self) -> &AgentToolSet {
        &self.tools
    }
    /// Returns the session's state-tool bindings.
    pub fn tool_session_runtime(&self) -> ToolSessionRuntime {
        self.tool_session.clone()
    }
    /// Returns the refresh callback registered at session creation.
    pub fn refresh_hook(&self) -> Option<BeforeModelStepHook> {
        self.refresh.clone()
    }
}

pub(crate) struct SessionRuntime {
    pub(crate) handle: SessionRuntimeHandle,
    cancellation: CancellationToken,
    initialized: Vec<(String, SessionEventSubscription)>,
    sources: tokio::task::JoinSet<(String, Result<(), SessionSourceError>)>,
    source_tasks: BTreeMap<tokio::task::Id, String>,
    inbox_changed: Arc<tokio::sync::Notify>,
    pub(crate) tasks_changed: Arc<tokio::sync::Notify>,
}

impl SessionRuntime {
    pub(crate) fn notify_inbox_change(&self) {
        self.inbox_changed.notify_waiters();
    }
    pub(crate) fn source_ids(&self) -> impl Iterator<Item = &str> {
        self.initialized.iter().map(|(id, _)| id.as_str())
    }
    pub(crate) fn start_sources(&mut self) {
        for (id, subscription) in self.initialized.drain(..) {
            let name = id.clone();
            let task = self
                .sources
                .spawn(async move { (id, subscription.run.await) });
            self.source_tasks.insert(task.id(), name);
        }
    }
    pub(crate) fn has_sources(&self) -> bool {
        !self.initialized.is_empty() || !self.sources.is_empty()
    }
    pub(crate) fn has_running_sources(&self) -> bool {
        !self.sources.is_empty()
    }
    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }
    pub(crate) fn release_tools(&mut self) {
        self.handle.tools.clear();
        self.handle.refresh = None;
        self.handle.workspace = None;
        self.handle.tool_session.cache().invalidate_all();
    }
    pub(crate) fn is_closing(&self) -> bool {
        self.cancellation.is_cancelled()
    }
    pub(crate) async fn next_source(&mut self) -> Option<(String, Result<(), SessionSourceError>)> {
        match self.sources.join_next_with_id().await? {
            Ok((id, result)) => {
                self.source_tasks.remove(&id);
                Some(result)
            }
            Err(error) => {
                let source = self
                    .source_tasks
                    .remove(&error.id())
                    .expect("every source task is registered before polling");
                Some((source, Err(SessionSourceError::new("run", error))))
            }
        }
    }
    pub(crate) async fn rollback(&mut self) -> Result<(), SessionSourceError> {
        self.cancel();
        self.start_sources();
        let mut failure = None;
        while let Some((_, result)) = self.next_source().await {
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for SessionRuntime {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn validate_source(name: &str, names: &mut BTreeSet<String>) -> Result<(), SessionBuildError> {
    if name.trim().is_empty() || name.len() > 128 || !names.insert(name.to_owned()) {
        return Err(SessionBuildError::SourceIdentity(name.to_owned()));
    }
    Ok(())
}
