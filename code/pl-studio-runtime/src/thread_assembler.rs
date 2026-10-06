//! One assembly and resource-owner registry for root, child and restored Threads.
mod activation;
mod agent_services;
mod agents;
pub(crate) mod observation;
pub use activation::{StudioActivationFactory, ThreadActivation, ThreadPreparation};
mod interaction_key;
mod user_input;
pub use user_input::{UserInteractionError, project_user_input};
mod interaction_router;
pub use interaction_key::{InteractionKeyError, decode_interaction_key};
pub use interaction_router::{StudioInteractionError, project_thread_interaction};
mod approval;
mod permission_projection;
mod plan_confirmation;
pub(crate) use approval::{ApprovalHost, StudioApprovalOptions};
pub use permission_projection::{PermissionProjectionError, project_execution_permission};
pub use plan_confirmation::{PlanInteractionError, project_plan_confirmation};
mod child_resources;
mod children;
mod published_resources;
pub use children::{
    ChildThreadRequest, ConfiguredChildFactory, StudioChildFactory, StudioChildResources,
};
mod mcp;
mod media;
mod tools;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
pub use tools::{StudioCommandBinding, StudioGitBinding, StudioThreadTools, StudioWorkspaceTools};

use pl_core::{
    context::ResourceAccess,
    model::ModelFactory,
    thread::{
        ContextCapacity, ThreadCheckpoint, ThreadError, ThreadHandle, ThreadLifecycle,
        cold::ColdStoreHandle,
    },
    tool::opaque::Registration,
};
use pl_model::{
    config::ResolvedModelRoute,
    runtime::{ModelRuntime, ThreadModel},
};

/// Product-selected collaboration exposure, independent from core tool scheduling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgentControlExposure {
    #[default]
    Disabled,
    Enabled,
}

/// Already resolved product inputs. Tool instances are transferred once, never cloned.
pub struct StudioThreadSpec {
    pub context_preparation: Option<pl_core::thread::context_preparation::ContextPreparer>,
    pub agent_controls: AgentControlExposure,
    pub execution: pl_core::thread::input::InputDriverOptions,
    pub id: String,
    pub parent_id: Option<String>,
    pub route: ResolvedModelRoute,
    /// False publishes the owner without a physical model until a later deferred update succeeds.
    pub model_available: bool,
    pub hosted_tools: Vec<pl_model::runtime::HostedTool>,
    pub checkpoint: Option<ThreadCheckpoint>,
    /// Initial instructions and records for a new Thread only; never used to re-render recovery.
    pub initial_context: Vec<pl_core::context::ContextRecord>,
    pub initial_extensions: BTreeMap<String, pl_core::context::OpaquePayload>,
    pub tools: Vec<Registration>,
    pub resources: ResourceAccess,
    pub capacity: ContextCapacity,
    pub cold_store: Option<ColdStoreHandle>,
}

impl std::fmt::Debug for StudioThreadSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StudioThreadSpec")
            .field("id", &self.id)
            .field("parent_id", &self.parent_id)
            .field(
                "checkpoint_revision",
                &self
                    .checkpoint
                    .as_ref()
                    .map(|checkpoint| checkpoint.state_revision),
            )
            .field("tools", &self.tools.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ThreadAssemblyError {
    #[error("Thread activation failed")]
    ActivationFailed(#[source] Arc<ThreadAssemblyError>),
    #[error("Thread resource preparation panicked")]
    ActivationPanicked,
    #[error("Thread {0} has unpublished resources requiring cleanup")]
    Unpublished(String),
    #[error("Studio resource operation failed: {operation}")]
    Resource {
        operation: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("only a root Thread may create child agents")]
    ChildDepth,
    #[error("agent tree capacity of {0} includes preparing children")]
    ChildCapacity(usize),
    #[error("Profile {0} does not accept a directory write scope")]
    WorkspaceScope(String),
    #[error("child configuration resolution failed")]
    Configuration(#[from] crate::config::ConfigRuntimeError),
    #[error("child preparation worker failed")]
    PreparationWorker(#[from] tokio::task::JoinError),
    #[error("tool catalog registration failed")]
    Registry(#[from] pl_core::tool::opaque::RegistryError),
    #[error("Thread assembly is closed")]
    Closed,
    #[error("initial context cannot replace restored Thread history during assembly")]
    InitialContextOnRecovery,
    #[error("Thread identity is already owned or has an invalid parent: {0}")]
    Identity(String),
    #[error("Thread tool binding was invalidated: {0}")]
    InvalidatedBinding(String),
    #[error("message identity {0} conflicts with an accepted delivery")]
    MessageConflict(String),
    #[error("message identity {0} is already accepted with an unverifiable body")]
    MessageUnverifiable(String),
    #[error("Thread {thread_id} workspace is unavailable: {reason}")]
    Workspace { thread_id: String, reason: String },
    #[error("Thread is still being assembled: {0}")]
    Preparing(String),
    #[error("Thread {0} workspace close disposition is already frozen")]
    CloseDisposition(String),
    #[error("Thread close is waiting for descendant {0}")]
    Descendant(String),
    #[error("model binding construction failed")]
    Binding(#[from] pl_model::PureError),
    #[error("model session construction failed")]
    Model(#[from] pl_core::model::ModelError),
    #[error("Thread setup or close failed")]
    Thread(#[from] ThreadError),
    #[error("Thread {id} setup failed ({setup}); cleanup also failed ({cleanup})")]
    Cleanup {
        id: String,
        #[source]
        setup: Box<ThreadError>,
        cleanup: Box<ThreadError>,
    },
}

#[derive(Debug)]
struct Entry {
    published_resources_closed: bool,
    workspace_disposition: Option<pl_tool::collaboration::thread::AgentWorkspaceDisposition>,
    cleanup: Arc<child_resources::CleanupGate>,
    execution: pl_core::thread::input::InputDriverOptions,
    incarnation: Arc<()>,
    parent_id: Option<String>,
    ready: bool,
    thread: ThreadHandle,
}

type ActivationOutcome = Result<ThreadHandle, Arc<ThreadAssemblyError>>;
#[derive(Debug)]
struct Preparation {
    parent_id: Option<String>,
    cancellation: tokio_util::sync::CancellationToken,
    result: Option<tokio::sync::watch::Receiver<Option<ActivationOutcome>>>,
}

#[derive(Debug, Default)]
struct State {
    agent_services: Option<agent_services::AgentServices>,
    observation: Option<observation::AssemblyObservation>,
    child_factory: Option<children::ChildFactory>,
    closing: bool,
    creating: BTreeMap<String, Preparation>,
    preparing_children: BTreeMap<String, children::PreparingChild>,
    unpublished_children: BTreeMap<String, child_resources::UnpublishedChild>,
    entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Default)]
struct Registry {
    state: Mutex<State>,
    changed: tokio::sync::Notify,
}

impl Registry {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Holds handles until actual resource shutdown succeeds; canonical execution state remains in core.
#[derive(Debug, Clone, Default)]
pub struct StudioThreadAssembler(Arc<Registry>);

pub(crate) struct Reservation {
    registry: Arc<Registry>,
    id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry.state().creating.remove(&self.id);
        self.registry.changed.notify_waiters();
    }
}

impl StudioThreadAssembler {
    /// Installs the product factory before publishing any Thread that can create descendants.
    ///
    /// # Errors
    /// Rejects changes after assembly has begun or shutdown has started.
    pub fn set_child_factory(
        &self,
        factory: impl StudioChildFactory,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing || !state.entries.is_empty() || !state.creating.is_empty() {
            return Err(ThreadAssemblyError::Closed);
        }
        state.child_factory = Some(children::ChildFactory::new(factory));
        Ok(())
    }

    /// Validates history, opens a fresh model session, binds tools/services and publishes once.
    /// History recovery does not invoke models or tools or re-render old content.
    ///
    /// # Errors
    /// Rejects duplicate identities and corrupt history; failed cleanup keeps its owner registered.
    pub async fn assemble(
        &self,
        spec: StudioThreadSpec,
    ) -> Result<ThreadHandle, ThreadAssemblyError> {
        let reservation = self.reserve(&spec.id, spec.parent_id.as_deref())?;
        self.assemble_reserved(spec, &reservation).await
    }

    async fn assemble_reserved(
        &self,
        mut spec: StudioThreadSpec,
        reservation: &Reservation,
    ) -> Result<ThreadHandle, ThreadAssemblyError> {
        if spec.id != reservation.id {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        if spec.id.is_empty()
            || spec.parent_id.as_deref() == Some(spec.id.as_str())
            || spec.parent_id.as_ref().is_some_and(String::is_empty)
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        if spec.checkpoint.is_some() && !spec.initial_context.is_empty() {
            return Err(ThreadAssemblyError::InitialContextOnRecovery);
        }
        pl_core::context::ContextSnapshot {
            revision: 0,
            records: spec.initial_context.clone().into(),
        }
        .validate_complete()
        .map_err(ThreadError::from)?;
        if spec
            .checkpoint
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.thread_id != spec.id)
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        match spec.agent_controls {
            AgentControlExposure::Disabled => {}
            AgentControlExposure::Enabled => {
                if spec.parent_id.is_some() {
                    return Err(ThreadAssemblyError::ChildDepth);
                }
                spec.tools.extend(self.agent_control_tools()?);
            }
        }
        let thread = if spec.model_available {
            let runtime = ModelRuntime::from_route(&spec.route)?;
            let model = ThreadModel::new(runtime, spec.route.reasoning_config())
                .with_hosted_tools(spec.hosted_tools);
            ThreadHandle::resume(
                spec.id.clone(),
                ModelFactory::new(model).open_session().await?,
                spec.checkpoint,
            )?
        } else {
            ThreadHandle::resume_without_model(spec.id.clone(), spec.checkpoint)?
        };
        self.0.state().entries.insert(
            spec.id.clone(),
            Entry {
                published_resources_closed: false,
                workspace_disposition: None,
                cleanup: Arc::new(child_resources::CleanupGate::default()),
                execution: spec.execution,
                incarnation: Arc::new(()),
                parent_id: spec.parent_id,
                ready: false,
                thread: thread.clone(),
            },
        );
        let setup = async {
            thread
                .set_context_preparation(spec.context_preparation)
                .await?;
            thread.set_resources(spec.resources).await?;
            thread.set_capacity(spec.capacity).await?;
            thread.register_tools(spec.tools).await?;
            if let Some(store) = spec.cold_store {
                thread.attach_storage(store).await?;
            }
            if !spec.initial_extensions.is_empty() {
                thread
                    .mutate_extensions(
                        spec.initial_extensions
                            .into_iter()
                            .map(|(id, payload)| {
                                pl_core::thread::extensions::ExtensionMutation::Put {
                                    id,
                                    expected_revision: None,
                                    payload,
                                }
                            })
                            .collect(),
                    )
                    .await?;
            }
            if !spec.initial_context.is_empty() {
                thread
                    .replace_context(pl_core::thread::ReplaceContext {
                        expected_revision: 0,
                        reason: pl_core::thread::ContextReplacementReason::Rebuild,
                        records: spec.initial_context,
                    })
                    .await?;
            }
            // Freeze the observer's recovery baseline before exposing this owner to execution.
            // Observers may query the registry, so invoke them without its lock held.
            self.publish_observation(&spec.id, &thread);
            let mut state = self.0.state();
            if state.closing {
                return Err(ThreadError::Closed);
            }
            let ready = !state.unpublished_children.contains_key(&spec.id);
            let entry = state.entries.get_mut(&spec.id).ok_or(ThreadError::Closed)?;
            entry.ready = ready;
            Ok::<_, ThreadError>(())
        }
        .await;
        if let Err(setup) = setup {
            match thread.close().await {
                Ok(()) => {
                    let removed = { self.0.state().entries.remove(&spec.id) };
                    drop(removed);
                }
                Err(cleanup) => {
                    return Err(ThreadAssemblyError::Cleanup {
                        id: spec.id,
                        setup: Box::new(setup),
                        cleanup: Box::new(cleanup),
                    });
                }
            }
            return Err(ThreadAssemblyError::Thread(setup));
        }
        Ok(thread)
    }

    /// Creates a child with an independent context from its frozen Profile and initial facts.
    /// Model sessions, task state, mutable extensions and executors are created independently.
    ///
    /// # Errors
    /// Rejects mismatched parent identities or recovery history.
    pub async fn assemble_child(
        &self,
        spec: StudioThreadSpec,
        caller: &pl_core::tool::opaque::CallContext,
    ) -> Result<ThreadHandle, ThreadAssemblyError> {
        if spec.parent_id.as_deref() != Some(caller.thread_id.as_str()) || spec.checkpoint.is_some()
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        self.assemble(spec).await
    }

    fn reserve(&self, id: &str, parent: Option<&str>) -> Result<Reservation, ThreadAssemblyError> {
        if id.is_empty() || parent.is_some_and(|parent| parent.is_empty() || parent == id) {
            return Err(ThreadAssemblyError::Identity(id.into()));
        }
        let mut state = self.0.state();
        if state.closing {
            return Err(ThreadAssemblyError::Closed);
        }
        if state.creating.contains_key(id) || state.entries.contains_key(id) {
            return Err(ThreadAssemblyError::Identity(id.to_owned()));
        }
        if let Some(parent) = parent {
            state
                .entries
                .get(parent)
                .filter(|entry| {
                    entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
                })
                .ok_or_else(|| ThreadAssemblyError::Identity(parent.to_owned()))?;
        }
        let mut cursor = parent;
        let mut visited = BTreeSet::new();
        while let Some(parent) = cursor {
            if parent == id || !visited.insert(parent) {
                return Err(ThreadAssemblyError::Identity(id.to_owned()));
            }
            cursor = state
                .entries
                .get(parent)
                .and_then(|entry| entry.parent_id.as_deref())
                .or_else(|| {
                    state
                        .creating
                        .get(parent)
                        .and_then(|preparation| preparation.parent_id.as_deref())
                });
        }
        state.creating.insert(
            id.to_owned(),
            Preparation {
                parent_id: parent.map(str::to_owned),
                cancellation: Default::default(),
                result: None,
            },
        );
        Ok(Reservation {
            registry: self.0.clone(),
            id: id.to_owned(),
        })
    }

    /// Admits a projected product input and lets the Thread owner drive its queue.
    /// Root, child and restored owners share this path after the same assembly publication.
    /// No product task or second input queue is created.
    ///
    /// # Errors
    /// Rejects unpublished identities and input admission failures. A receipt remains valid if execution later fails.
    pub async fn submit_input(
        &self,
        id: &str,
        input: pl_core::thread::input::ThreadInput,
        options: pl_core::thread::input::InputDriverOptions,
    ) -> Result<pl_core::thread::input::InputRecord, ThreadAssemblyError> {
        let thread = {
            let mut state = self.0.state();
            if state.closing {
                return Err(ThreadAssemblyError::Closed);
            }
            let entry = state
                .entries
                .get_mut(id)
                .filter(|entry| {
                    entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
                })
                .ok_or_else(|| ThreadAssemblyError::Identity(id.to_owned()))?;
            entry.execution = options;
            entry.thread.clone()
        };
        Ok(thread.submit_input_and_run(input, options).await?)
    }

    /// Applies a resolved model binding at the next serial Turn boundary without rebuilding tools.
    ///
    /// # Errors
    /// Rejects missing/unpublished owners and preserves model construction or close errors.
    pub async fn replace_model(
        &self,
        id: &str,
        route: &ResolvedModelRoute,
        hosted_tools: Vec<pl_model::runtime::HostedTool>,
        preparation: Option<pl_core::thread::context_preparation::ContextPreparer>,
    ) -> Result<(), ThreadAssemblyError> {
        let thread = self
            .thread(id)
            .ok_or_else(|| ThreadAssemblyError::Identity(id.to_owned()))?;
        let model = ThreadModel::new(ModelRuntime::from_route(route)?, route.reasoning_config())
            .with_hosted_tools(hosted_tools);
        thread
            .replace_model_with_preparation(ModelFactory::new(model), preparation)
            .await?;
        Ok(())
    }

    /// Returns only fully assembled owners, never partially initialized resources.
    pub fn thread(&self, id: &str) -> Option<ThreadHandle> {
        let state = self.0.state();
        if state.closing {
            return None;
        }
        state
            .entries
            .get(id)
            .filter(|entry| {
                entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
            })
            .map(|entry| entry.thread.clone())
    }

    /// Returns read-only handles for all published or closing owners, including failed shutdowns.
    pub fn observed_threads(&self) -> Vec<(String, ThreadHandle)> {
        self.0
            .state()
            .entries
            .iter()
            .map(|(id, entry)| (id.clone(), entry.thread.clone()))
            .collect()
    }

    /// Evicts a leaf only after core atomically seals idle admission and saves its final facts.
    ///
    /// # Errors
    /// Retains an unpublished closing owner when resource release or persistence fails.
    pub async fn evict_idle(&self, id: &str) -> Result<bool, ThreadAssemblyError> {
        let candidate = {
            let mut state = self.0.state();
            if state.creating.contains_key(id)
                || state.entries.get(id).is_some_and(|entry| {
                    entry.parent_id.is_some()
                        && entry.thread.snapshot().lifecycle != ThreadLifecycle::Closed
                })
                || state
                    .entries
                    .values()
                    .any(|entry| entry.parent_id.as_deref() == Some(id))
                || state
                    .creating
                    .values()
                    .any(|entry| entry.parent_id.as_deref() == Some(id))
                || state
                    .preparing_children
                    .values()
                    .any(|entry| entry.parent == id)
                || state
                    .unpublished_children
                    .values()
                    .any(|entry| entry.parent == id)
            {
                return Ok(false);
            }
            state.entries.get_mut(id).map(|entry| {
                entry.ready = false;
                (entry.incarnation.clone(), entry.thread.clone())
            })
        };
        let Some((incarnation, thread)) = candidate else {
            return Ok(true);
        };
        if !thread.close_if_idle().await? {
            let mut state = self.0.state();
            if !state.closing
                && let Some(entry) = state.entries.get_mut(id)
                && Arc::ptr_eq(&entry.incarnation, &incarnation)
            {
                entry.ready = true;
            }
            return Ok(false);
        }
        self.close(id).await?;
        Ok(true)
    }

    /// Closes one tree from leaves to root, retaining failures for a later retry.
    ///
    /// # Errors
    /// Propagates resource cleanup or concurrent preparation failures without removing the owner.
    pub async fn close_tree(&self, root: &str) -> Result<(), ThreadAssemblyError> {
        let mut targets = {
            let mut state = self.0.state();
            let mut targets = Vec::new();
            for id in state.entries.keys() {
                let mut cursor = Some(id.as_str());
                let mut depth = 0;
                while let Some(current) = cursor {
                    if current == root {
                        targets.push((depth, id.clone()));
                        break;
                    }
                    depth += 1;
                    if depth > state.entries.len() {
                        return Err(ThreadAssemblyError::Identity(id.clone()));
                    }
                    cursor = state
                        .entries
                        .get(current)
                        .and_then(|entry| entry.parent_id.as_deref());
                }
            }
            // Seal the entire known tree before awaiting any child, preventing new descendant spawns.
            for (_, id) in &targets {
                if let Some(entry) = state.entries.get_mut(id) {
                    entry.ready = false;
                }
            }
            targets
        };
        targets.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        for (_, id) in targets {
            self.close(&id).await?;
        }
        self.close(root).await
    }

    /// Closes an owner, including failed unpublished assembly, while retaining failed closes for retry.
    ///
    /// # Errors
    /// Returns in-progress assembly or the actual Thread shutdown failure.
    pub async fn close(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        let preparation = {
            self.0
                .state()
                .creating
                .get(id)
                .map(|preparation| preparation.cancellation.clone())
        };
        if let Some(cancellation) = preparation {
            cancellation.cancel();
            return Err(ThreadAssemblyError::Preparing(id.into()));
        }
        if self.0.state().preparing_children.contains_key(id) {
            return Err(ThreadAssemblyError::Preparing(id.into()));
        }
        self.close_thread(id).await?;
        self.discard_unpublished_child(id).await
    }

    async fn close_thread(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        if let Some(entry) = self.0.state().entries.get_mut(id) {
            entry
                .workspace_disposition
                .get_or_insert(Default::default());
        }
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let preparing = {
                let mut state = self.0.state();
                if let Some(entry) = state.entries.get_mut(id) {
                    entry.ready = false;
                }
                state
                    .preparing_children
                    .values()
                    .filter(|child| child.parent == id)
                    .map(|child| child.cancellation.clone())
                    .collect::<Vec<_>>()
            };
            if preparing.is_empty() {
                break;
            }
            for cancellation in preparing {
                cancellation.cancel();
            }
            changed.await;
        }
        let thread = {
            let mut state = self.0.state();
            if let Some((child, _)) = state
                .unpublished_children
                .iter()
                .find(|(_, child)| child.parent == id)
            {
                return Err(ThreadAssemblyError::Preparing(child.clone()));
            }
            if state.creating.contains_key(id) {
                return Err(ThreadAssemblyError::Preparing(id.to_owned()));
            }
            if let Some((child, _)) = state
                .preparing_children
                .iter()
                .find(|(_, preparing)| preparing.parent == id)
            {
                return Err(ThreadAssemblyError::Preparing(child.clone()));
            }
            if let Some((child, _)) = state
                .entries
                .iter()
                .find(|(_, entry)| entry.parent_id.as_deref() == Some(id))
            {
                return Err(ThreadAssemblyError::Descendant(child.clone()));
            }
            if let Some((child, _)) = state
                .creating
                .iter()
                .find(|(_, preparation)| preparation.parent_id.as_deref() == Some(id))
            {
                return Err(ThreadAssemblyError::Preparing(child.clone()));
            }
            state.entries.get_mut(id).map(|entry| {
                entry.ready = false;
                (entry.incarnation.clone(), entry.thread.clone())
            })
        };
        let Some((incarnation, thread)) = thread else {
            return Ok(());
        };
        if thread.snapshot().lifecycle != ThreadLifecycle::Closed {
            let closed = thread.close().await;
            if let Err(error) = closed
                && thread.snapshot().lifecycle != ThreadLifecycle::Closed
            {
                return Err(error.into());
            }
        }
        self.drain_observation(id).await?;
        self.close_published_resources(id, &incarnation).await?;
        let removed = {
            let mut state = self.0.state();
            if state
                .entries
                .get(id)
                .is_some_and(|entry| Arc::ptr_eq(&entry.incarnation, &incarnation))
            {
                state.entries.remove(id)
            } else {
                None
            }
        };
        drop(removed);
        Ok(())
    }

    /// Seals shared admission, broadcasts cancellation, then closes every independent
    /// forest/branch concurrently while a global final ACK confirms all admitted preparations
    /// actually drained.
    ///
    /// 与旧实现的区别：全局 preparation 等待不再是**前置** barrier——它与独立森林/分支的关闭并发
    /// （`futures::join!`），所以某个 root 的卡住子分支/准备只阻塞它自己之外，独立资源仍立即
    /// close；但本方法**绝不**在还有已准入 `creating`/`preparing_children` 未真实排空时返回成功，
    /// 因此不会让严格关闭误判生产者已停而停止 writer / 关库 / 发布 `Stopped`。父子依赖（子关闭
    /// 确认先于需要它的父关闭）与 owner 保留规则不变：失败累计、entry 保留、可重试。
    ///
    /// Failures retain their entries and can be retried with this method or `close`.
    pub async fn close_all(&self) -> Vec<(String, ThreadAssemblyError)> {
        // 共享准入先封闭，所有准备过程 cancellation 先广播；此处不等待它们排空。
        let preparing = {
            let mut state = self.0.state();
            state.closing = true;
            state
                .preparing_children
                .values()
                .map(|child| child.cancellation.clone())
                .chain(
                    state
                        .creating
                        .values()
                        .map(|preparation| preparation.cancellation.clone()),
                )
                .collect::<Vec<_>>()
        };
        for cancellation in preparing {
            cancellation.cancel();
        }

        let mut covered: BTreeSet<String> = BTreeSet::new();
        let mut failures = Vec::new();

        // 第一轮：独立森林/orphan 关闭与「全部已准入 creating/preparing 真实排空」的全局最终 ACK
        // 并发推进——独立资源先开始自身 close，不被全局 ACK 前置阻塞；全局 ACK 又保证最终成功前
        // 不存在仍在清理/持有资源的准备 owner。
        let (roots, orphan_unpublished) = self.select_uncovered_roots(&covered);
        self.mark_covered(&mut covered, &roots, &orphan_unpublished);
        let (first_failures, ()) = futures::join!(
            self.close_independent_batch(roots, orphan_unpublished),
            self.await_all_preparations_drained(),
        );
        failures.extend(first_failures);

        // 迟到 owner：准备排空后才可见、第一轮快照未覆盖的 entries/unpublished（例如 closing 前
        // 已准入、快照之后才插入 entry、setup 失败后 cleanup 也失败而保留 owner），用同一局部化
        // 方式收尾。此刻不会再有新的 assembly / child preparation，集合只会收缩，循环终止。
        loop {
            let (roots, orphan_unpublished) = self.select_uncovered_roots(&covered);
            if roots.is_empty() && orphan_unpublished.is_empty() {
                break;
            }
            self.mark_covered(&mut covered, &roots, &orphan_unpublished);
            let batch_failures = self
                .close_independent_batch(roots, orphan_unpublished)
                .await;
            failures.extend(batch_failures);
        }
        failures
    }

    /// 选取当前尚未负责（`covered` 之外）的独立森林根与 orphan unpublished 子资源。
    ///
    /// 根 = 父 entry 不在册，或父 entry 已在 `covered`（其自身 close 不会再等待该子项）。orphan =
    /// 父 entry 不在册，或父 entry 已在 `covered`。这样迟到 owner 也有归属、不被漏关。
    fn select_uncovered_roots(&self, covered: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
        let state = self.0.state();
        let mut roots = Vec::new();
        for (id, entry) in state.entries.iter() {
            if covered.contains(id) {
                continue;
            }
            let parent_present = entry
                .parent_id
                .as_deref()
                .is_some_and(|parent| state.entries.contains_key(parent));
            let parent_covered = entry
                .parent_id
                .as_deref()
                .is_some_and(|parent| covered.contains(parent));
            if !parent_present || parent_covered {
                roots.push(id.clone());
            }
        }
        let mut orphan_unpublished = Vec::new();
        for (id, child) in state.unpublished_children.iter() {
            if covered.contains(id) {
                continue;
            }
            if !state.entries.contains_key(&child.parent) || covered.contains(&child.parent) {
                orphan_unpublished.push(id.clone());
            }
        }
        (roots, orphan_unpublished)
    }

    /// 把一批根及其可达子树、以及 orphan 子资源标记为已负责，避免后续批次重复 close。
    fn mark_covered(
        &self,
        covered: &mut BTreeSet<String>,
        roots: &[String],
        orphan_unpublished: &[String],
    ) {
        let state = self.0.state();
        let mut stack = roots.to_vec();
        while let Some(id) = stack.pop() {
            if !covered.insert(id.clone()) {
                continue;
            }
            for (child, entry) in state.entries.iter() {
                if entry.parent_id.as_deref() == Some(id.as_str()) {
                    stack.push(child.clone());
                }
            }
        }
        for id in orphan_unpublished {
            covered.insert(id.clone());
        }
    }

    /// 并发关闭一批独立森林根与 orphan unpublished，返回累计失败（`join_all` 保序，顺序确定）。
    async fn close_independent_batch(
        &self,
        roots: Vec<String>,
        orphan_unpublished: Vec<String>,
    ) -> Vec<(String, ThreadAssemblyError)> {
        let mut root_futures = Vec::with_capacity(roots.len());
        for root in roots {
            root_futures.push(self.close_entry_tree(root));
        }
        let mut orphan_futures = Vec::with_capacity(orphan_unpublished.len());
        for child in orphan_unpublished {
            orphan_futures.push(async move {
                // 只有该子资源自身的创建准备排空后才可 discard（`close_thread` 会拒绝 preparing 者）。
                self.await_own_preparations_drained(&child).await;
                match self.discard_unpublished_child(&child).await {
                    Ok(()) => Vec::new(),
                    Err(error) => vec![(child, error)],
                }
            });
        }
        let (tree_failures, orphan_failures) = futures::join!(
            futures::future::join_all(root_futures),
            futures::future::join_all(orphan_futures),
        );
        let mut failures = Vec::new();
        for group in tree_failures {
            failures.extend(group);
        }
        for group in orphan_failures {
            failures.extend(group);
        }
        failures
    }

    /// 全局最终 ACK：等待**全部**已准入的 `creating`/`preparing_children` owner 真实排空。
    ///
    /// 与独立树关闭并发进行，不是前置 barrier；但 close_all 成功返回前必须等到它完成，避免仍在
    /// 清理/持有资源的准备 owner 被误判为已停（否则严格关闭会停 writer / 关库 / 发布 `Stopped`）。
    /// 取消已广播，`closing` 保证不会新增准备 owner，因此集合只会收缩。
    async fn await_all_preparations_drained(&self) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let drained = {
                let state = self.0.state();
                state.creating.is_empty() && state.preparing_children.is_empty()
            };
            if drained {
                break;
            }
            changed.await;
        }
    }

    /// 递归关闭一个在册 entry 及其整棵依赖子树，只等待自身需要的子资源与准备过程。
    ///
    /// 依赖：该 entry 的每个子 entry（`parent_id == id`）、它自己的 unpublished 子资源，以及它
    /// 自己的 creating/preparing 子过程。它们彼此独立，因此并发收束；全部就绪后才 `close(id)`。
    /// 只等待自身依赖，绝不等待其他森林/分支。失败累计并保留 owner，返回本子树全部失败（含 id
    /// 自身）。通过 boxed future 递归，避免 async fn 的无限类型。
    fn close_entry_tree<'a>(
        &'a self,
        id: String,
    ) -> futures::future::BoxFuture<'a, Vec<(String, ThreadAssemblyError)>> {
        Box::pin(async move {
            let mut failures = Vec::new();

            // 一次持锁快照自身直接子依赖，不跨 await。
            let (child_entries, child_unpublished) = {
                let state = self.0.state();
                let mut child_entries = Vec::new();
                let mut child_unpublished = Vec::new();
                for (child, entry) in state.entries.iter() {
                    if entry.parent_id.as_deref() == Some(id.as_str()) {
                        child_entries.push(child.clone());
                    }
                }
                for (child, unpublished) in state.unpublished_children.iter() {
                    if unpublished.parent == id {
                        child_unpublished.push(child.clone());
                    }
                }
                (child_entries, child_unpublished)
            };

            let mut child_futures: Vec<
                futures::future::BoxFuture<'a, Vec<(String, ThreadAssemblyError)>>,
            > = Vec::with_capacity(child_entries.len());
            for child in child_entries {
                child_futures.push(self.close_entry_tree(child));
            }
            let mut unpublished_futures = Vec::with_capacity(child_unpublished.len());
            for child in child_unpublished {
                unpublished_futures.push(async move {
                    // 该子资源自身的创建准备排空后才可 discard（`discard_unpublished_child` 会拒绝
                    // preparing 者）。
                    self.await_own_preparations_drained(&child).await;
                    let outcome = self.discard_unpublished_child(&child).await;
                    (child, outcome)
                });
            }
            // 自身的 creating/preparing 子过程：取消已由 `close_all` 全局广播，这里只对自身这部分
            // 有界等待，不等待其他分支。
            let own_preparations = self.await_own_preparations_drained(&id);
            let (child_failures, unpublished_outcomes, ()) = futures::join!(
                futures::future::join_all(child_futures),
                futures::future::join_all(unpublished_futures),
                own_preparations,
            );
            for group in child_failures {
                failures.extend(group);
            }
            for (child, outcome) in unpublished_outcomes {
                if let Err(error) = outcome {
                    failures.push((child, error));
                }
            }

            // 依赖就绪后才关闭自身：原 `close` 的真实父子保护与 incarnation/owner 保留规则不变，
            // 子关闭未确认（失败保留）时 `close` 返回对应错误并被累计，绝不早释放。
            if let Err(error) = self.close(&id).await {
                failures.push((id, error));
            }
            failures
        })
    }

    /// 等待并只等待某个节点的自身依赖型准备过程排空。
    ///
    /// 覆盖两类真实依赖：节点**自身**的创建准备（`creating`/`preparing_children` 含该 id），以及
    /// 节点**子项**的创建准备（`creating` 的 parent/`preparing_children` 的 parent 指向该 id）。
    /// 取消已由 `close_all` 全局广播；这里只在该节点确有对应准备过程时等待其真实离开注册表，不等待
    /// 任何其他分支或全局集合。`closing`/`entry.ready=false` 保证等待期间不会产生新的子过程。
    async fn await_own_preparations_drained(&self, id: &str) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let blocked = {
                let state = self.0.state();
                state.creating.contains_key(id)
                    || state.preparing_children.contains_key(id)
                    || state
                        .creating
                        .values()
                        .any(|preparation| preparation.parent_id.as_deref() == Some(id))
                    || state
                        .preparing_children
                        .values()
                        .any(|child| child.parent == id)
            };
            if !blocked {
                break;
            }
            changed.await;
        }
    }
}

pub(crate) use tools::{StudioCommandProcesses, WeakCommandProcesses};
