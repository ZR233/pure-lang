//! One assembly and resource-owner registry for root, child and restored Threads.
mod activation;
mod agent_queries;
mod agents;
mod observation;
pub(crate) mod progress;
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
        ContextCapacity, ThreadError, ThreadHandle, ThreadLifecycle,
        cold::ColdStoreHandle,
        journal::{self, ThreadCommit},
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
    ProgressOnly,
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
    pub hosted_tools: Vec<pl_model::runtime::HostedTool>,
    pub history: Vec<Arc<ThreadCommit>>,
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
            .field("history_records", &self.history.len())
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
    agent_queries: Option<agent_queries::QueryServices>,
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

struct Reservation {
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
    pub(crate) fn progress_tool_registration(
        &self,
    ) -> Result<pl_core::tool::opaque::Registration, ThreadAssemblyError> {
        progress::registration()
    }

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
        if !spec.history.is_empty()
            && (!spec.initial_context.is_empty() || !spec.initial_extensions.is_empty())
        {
            return Err(ThreadAssemblyError::InitialContextOnRecovery);
        }
        pl_core::context::ContextSnapshot {
            revision: 0,
            records: spec.initial_context.clone().into(),
        }
        .validate_complete()
        .map_err(ThreadError::from)?;
        journal::replay(&spec.history)?;
        if spec
            .history
            .first()
            .is_some_and(|commit| commit.thread_id != spec.id)
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        match spec.agent_controls {
            AgentControlExposure::Disabled => {}
            AgentControlExposure::ProgressOnly => spec.tools.push(progress::registration()?),
            AgentControlExposure::Enabled => {
                if spec.parent_id.is_some() {
                    return Err(ThreadAssemblyError::ChildDepth);
                }
                spec.tools.extend(self.agent_control_tools()?);
                spec.tools.push(progress::registration()?);
            }
        }
        let runtime = ModelRuntime::from_route(&spec.route)?;
        let model = ThreadModel::new(runtime, spec.route.reasoning_config())
            .with_hosted_tools(spec.hosted_tools);
        let thread = ThreadHandle::restore(
            spec.id.clone(),
            ModelFactory::new(model).open_session().await?,
            spec.history,
        )?;
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

    /// Creates a child with selected context from its originating, immutable model request.
    /// Model sessions, task state, mutable extensions and executors are created independently.
    ///
    /// # Errors
    /// Rejects mismatched parent identities, recovery history or malformed inherited context.
    pub async fn assemble_child(
        &self,
        mut spec: StudioThreadSpec,
        caller: &pl_core::tool::opaque::CallContext,
        inheritance: pl_core::context::ContextInheritance,
    ) -> Result<ThreadHandle, ThreadAssemblyError> {
        if spec.parent_id.as_deref() != Some(caller.thread_id.as_str()) || !spec.history.is_empty()
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        let inherited = caller
            .context
            .inherit(inheritance)
            .map_err(ThreadError::from)?;
        let (mut instructions, mut history): (Vec<_>, Vec<_>) = inherited
            .into_iter()
            .partition(|record| record.source == pl_core::context::ContextSource::Instruction);
        let (new_instructions, current_facts): (Vec<_>, Vec<_>) = spec
            .initial_context
            .into_iter()
            .partition(|record| record.source == pl_core::context::ContextSource::Instruction);
        instructions.extend(new_instructions);
        history.extend(current_facts);
        instructions.extend(history);
        spec.initial_context = instructions;
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
        if thread.snapshot().lifecycle != ThreadLifecycle::Closed
            && let Err(error) = thread.close().await
            && thread.snapshot().lifecycle != ThreadLifecycle::Closed
        {
            return Err(error.into());
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

    /// Seals assembly, drains preparing owners and closes descendants before parents.
    /// Failures retain their entries and can be retried with this method or `close`.
    pub async fn close_all(&self) -> Vec<(String, ThreadAssemblyError)> {
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
        let unpublished = {
            self.0
                .state()
                .unpublished_children
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        let mut failures = Vec::new();
        for id in unpublished {
            if let Err(error) = self.discard_unpublished_child(&id).await {
                failures.push((id, error));
            }
        }
        let mut ids = {
            let state = self.0.state();
            state
                .entries
                .keys()
                .map(|id| {
                    let mut depth = 0;
                    let mut cursor = state
                        .entries
                        .get(id)
                        .and_then(|entry| entry.parent_id.as_ref());
                    while let Some(parent) = cursor {
                        depth += 1;
                        if depth > state.entries.len() {
                            break;
                        }
                        cursor = state
                            .entries
                            .get(parent)
                            .and_then(|entry| entry.parent_id.as_ref());
                    }
                    (depth, id.clone())
                })
                .collect::<Vec<_>>()
        };
        ids.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        for (_, id) in ids {
            if let Err(error) = self.close(&id).await {
                failures.push((id, error));
            }
        }
        failures
    }
}

pub(crate) use tools::{StudioCommandProcesses, WeakCommandProcesses};

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        tool::{
            ToolOutput,
            opaque::{CallContext, Tool, ToolError},
        },
    };
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Debug)]
    struct CloseProbe {
        id: String,
        log: Arc<Mutex<Vec<String>>>,
        fail_once: AtomicBool,
    }
    impl Tool for CloseProbe {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            panic!("assembly and replay must never execute a tool")
        }
        async fn close(&self) -> Result<(), ToolError> {
            self.log.lock().unwrap().push(self.id.clone());
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(ToolError::new(std::io::Error::other("retry close")));
            }
            Ok(())
        }
    }

    pub(super) fn spec(
        id: &str,
        parent_id: Option<&str>,
        root: &std::path::Path,
    ) -> StudioThreadSpec {
        StudioThreadSpec {
            context_preparation: None,
            agent_controls: AgentControlExposure::Disabled,
            execution: pl_core::thread::input::InputDriverOptions {
                max_model_steps: std::num::NonZeroU32::new(64).unwrap(),
            },
            hosted_tools: Vec::new(),
            id: id.into(),
            parent_id: parent_id.map(str::to_owned),
            route: ResolvedModelRoute {
                pricing_mode: pl_protocol::PricingMode::Catalog,
                role: pl_protocol::AgentRoleId::new("test").unwrap(),
                provider_id: pl_model::config::ProviderId::new("test").unwrap(),
                endpoint: pl_model::provider::ProviderEndpoint::deepseek(None),
                model: pl_model::model::ModelInfo::compatible("assembly-test"),
                effort: None,
            },
            history: Vec::new(),
            initial_context: Vec::new(),
            initial_extensions: BTreeMap::new(),
            tools: Vec::new(),
            capacity: ContextCapacity::default(),
            cold_store: None,
            resources: ResourceAccess::new(crate::resource_store::FileResourceStore::new(
                root.to_owned(),
            )),
        }
    }

    #[tokio::test]
    async fn initial_instructions_commit_once_and_cannot_rewrite_recovered_history() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        let mut specification = spec("instructions", None, directory.path());
        let initial = pl_core::context::ContextRecord {
            id: "rules".into(),
            turn_id: None,
            source: pl_core::context::ContextSource::Instruction,
            content: vec![pl_core::context::ContextContent::Text {
                text: Arc::from("original stable rules"),
            }],
            tool_calls: Vec::new(),
        };
        specification.initial_context.push(initial.clone());
        let thread = assembler.assemble(specification).await.unwrap();
        assert_eq!(
            thread.snapshot().context.records.as_ref(),
            std::slice::from_ref(&initial)
        );
        let history = thread.journal().await.unwrap();
        assembler.close("instructions").await.unwrap();
        let mut restore = spec("instructions", None, directory.path());
        restore.history = history.clone();
        restore.initial_context.push(initial);
        assert!(matches!(
            assembler.assemble(restore).await,
            Err(ThreadAssemblyError::InitialContextOnRecovery)
        ));
        let mut restore = spec("instructions", None, directory.path());
        restore.history = history;
        let restored = assembler.assemble(restore).await.unwrap();
        assert_eq!(restored.snapshot().context, thread.snapshot().context);
        assert!(assembler.close_all().await.is_empty());
    }

    #[tokio::test]
    async fn observation_baseline_is_installed_before_owner_becomes_executable() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        let registry = Arc::downgrade(&assembler.0);
        let baselines = Arc::new(Mutex::new(Vec::new()));
        let observed = baselines.clone();
        assembler
            .observe_assembly(
                move |id, thread| {
                    let registry = StudioThreadAssembler(registry.upgrade().unwrap());
                    assert!(
                        registry.thread(&id).is_none(),
                        "execution must not race the observer baseline"
                    );
                    observed
                        .lock()
                        .unwrap()
                        .push((id, thread.snapshot().commit_sequence));
                },
                |_| Box::pin(async { Ok(()) }),
            )
            .unwrap();
        let thread = assembler
            .assemble(spec("observed", None, directory.path()))
            .await
            .unwrap();
        assert!(assembler.thread("observed").is_some());
        assert_eq!(
            *baselines.lock().unwrap(),
            vec![("observed".into(), thread.snapshot().commit_sequence)]
        );
        assert!(assembler.close_all().await.is_empty());
    }

    #[tokio::test]
    async fn idle_live_child_and_its_parent_remain_resident_until_explicit_close() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        assembler
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        assembler
            .assemble(spec("child", Some("root"), directory.path()))
            .await
            .unwrap();
        assert!(!assembler.evict_idle("child").await.unwrap());
        assert!(!assembler.evict_idle("root").await.unwrap());
        assert!(assembler.thread("root").is_some());
        assert!(assembler.thread("child").is_some());
        assert!(assembler.close_all().await.is_empty());
    }

    #[tokio::test]
    async fn failed_child_close_retains_both_owners_and_retries_before_parent_close() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut root = spec("root", None, directory.path());
        root.tools.push(
            Registration::new(
                "probe".into(),
                OpaquePayload::text("schema"),
                CloseProbe {
                    id: "root".into(),
                    log: log.clone(),
                    fail_once: AtomicBool::new(false),
                },
            )
            .unwrap(),
        );
        let root = assembler.assemble(root).await.unwrap();
        let mut child = spec("child", Some("root"), directory.path());
        child.tools.push(
            Registration::new(
                "probe".into(),
                OpaquePayload::text("schema"),
                CloseProbe {
                    id: "child".into(),
                    log: log.clone(),
                    fail_once: AtomicBool::new(true),
                },
            )
            .unwrap(),
        );
        assembler.assemble(child).await.unwrap();
        let failures = assembler.close_all().await;
        assert_eq!(
            failures
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>(),
            vec!["child", "root"]
        );
        assert_eq!(root.snapshot().lifecycle, ThreadLifecycle::Open);
        assert_eq!(assembler.0.state().entries.len(), 2);
        assert!(assembler.close_all().await.is_empty());
        assert_eq!(*log.lock().unwrap(), vec!["child", "child", "root"]);
        assert!(assembler.0.state().entries.is_empty());
    }

    #[tokio::test]
    async fn closing_one_tree_retains_failed_owners_and_does_not_close_an_unrelated_tree() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        let log = Arc::new(Mutex::new(Vec::new()));
        let root = assembler
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let unrelated = assembler
            .assemble(spec("unrelated", None, directory.path()))
            .await
            .unwrap();
        let mut child = spec("child", Some("root"), directory.path());
        child.tools.push(
            Registration::new(
                "probe".into(),
                OpaquePayload::text("schema"),
                CloseProbe {
                    id: "child".into(),
                    log: log.clone(),
                    fail_once: AtomicBool::new(true),
                },
            )
            .unwrap(),
        );
        assembler.assemble(child).await.unwrap();
        assert!(assembler.close_tree("root").await.is_err());
        assert_eq!(assembler.observed_threads().len(), 3);
        assert!(
            assembler.thread("root").is_none(),
            "a failed tree close must seal new input"
        );
        assert_eq!(root.snapshot().lifecycle, ThreadLifecycle::Open);
        assert_eq!(unrelated.snapshot().lifecycle, ThreadLifecycle::Open);
        assembler.close_tree("root").await.unwrap();
        assert_eq!(root.snapshot().lifecycle, ThreadLifecycle::Closed);
        assert_eq!(unrelated.snapshot().lifecycle, ThreadLifecycle::Open);
        assert_eq!(*log.lock().unwrap(), vec!["child", "child"]);
        assert!(assembler.close_all().await.is_empty());
    }

    #[tokio::test]
    async fn delayed_old_close_does_not_remove_a_recreated_identity() {
        let directory = tempfile::tempdir().unwrap();
        let assembler = StudioThreadAssembler::default();
        assembler
            .assemble(spec("thread", None, directory.path()))
            .await
            .unwrap();
        let first = assembler.close("thread");
        let second = assembler.close("thread");
        tokio::pin!(first, second);
        assert!(futures::poll!(first.as_mut()).is_pending());
        assert!(futures::poll!(second.as_mut()).is_pending());
        first.await.unwrap();
        let new_owner = assembler
            .assemble(spec("thread", None, directory.path()))
            .await
            .unwrap();
        second.await.unwrap();
        assert!(assembler.thread("thread").is_some());
        assert_eq!(new_owner.snapshot().lifecycle, ThreadLifecycle::Open);
        assert!(assembler.close_all().await.is_empty());
    }

    #[test]
    fn reservations_require_an_open_parent_before_resources_are_created() {
        let assembler = StudioThreadAssembler::default();
        assert!(matches!(
            assembler.reserve("first", Some("second")),
            Err(ThreadAssemblyError::Identity(_))
        ));
        let pending_parent = assembler.reserve("second", None).unwrap();
        assert!(matches!(
            assembler.reserve("first", Some("second")),
            Err(ThreadAssemblyError::Identity(_))
        ));
        assert!(matches!(
            assembler.reserve("first", Some("first")),
            Err(ThreadAssemblyError::Identity(_))
        ));
        drop(pending_parent);
        assert!(assembler.0.state().creating.is_empty());
    }
    #[tokio::test]
    async fn child_profile_instructions_precede_inherited_history_and_keep_original_content() {
        use pl_core::context::{
            ContextContent, ContextInheritance, ContextRecord, ContextSource, HistoryInheritance,
            InstructionInheritance,
        };
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let historical = ContextRecord {
            id: "original-user".into(),
            turn_id: None,
            source: ContextSource::User,
            content: vec![ContextContent::Text {
                text: Arc::from("  原始历史\r\n"),
            }],
            tool_calls: Vec::new(),
        };
        let mut root_spec = spec("root", None, directory.path());
        root_spec.initial_context = vec![historical.clone()];
        let root = owner.assemble(root_spec).await.unwrap();
        let mut child_spec = spec("child", Some("root"), directory.path());
        let instruction = ContextRecord {
            id: "child-profile".into(),
            turn_id: None,
            source: ContextSource::Instruction,
            content: vec![ContextContent::Text {
                text: Arc::from("child role"),
            }],
            tool_calls: Vec::new(),
        };
        child_spec.initial_context = vec![instruction.clone()];
        let caller = CallContext {
            grant: Default::default(),
            context: root.snapshot().context,
            model_projection: None,
            tasks: None,
            thread_id: "root".into(),
            turn_id: "turn".into(),
            call_id: "spawn".into(),
            cancellation: Default::default(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        };
        let child = owner
            .assemble_child(
                child_spec,
                &caller,
                ContextInheritance {
                    history: HistoryInheritance::All,
                    instructions: InstructionInheritance::Exclude,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            child.snapshot().context.records.as_ref(),
            &[instruction, historical.clone()]
        );
        assert_eq!(root.snapshot().context.records.as_ref(), &[historical]);
        assert!(owner.close_all().await.is_empty());
    }
}
