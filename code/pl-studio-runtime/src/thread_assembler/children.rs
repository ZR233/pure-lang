//! Product-selected child configuration and resource preparation.
use super::child_resources::{CleanupGate, UnpublishedChild};
use super::{StudioThreadSpec, ThreadAssemblyError};
use futures::future::BoxFuture;
use pl_tool::collaboration::thread::AgentWorkspaceDisposition;
use std::{future::Future, sync::Arc};

/// Facts passed to the product factory; caller identity is supplied by core, not model arguments.
#[derive(Debug, Clone)]
pub struct ChildThreadRequest {
    pub id: String,
    pub cancellation: tokio_util::sync::CancellationToken,
    pub caller: String,
    pub call_id: String,
    pub profile_id: String,
    pub writable_paths: Option<Vec<String>>,
    pub metadata: pl_core::context::OpaquePayload,
}

/// Resolves configuration, creation permissions, limits and physical resources for one child.
pub trait StudioChildFactory: Send + Sync + std::fmt::Debug + 'static {
    /// Must reject unauthorized profiles and exceeded product limits before allocating resources.
    /// The factory owns unpublished resources and must honor cancellation during preparation.
    fn prepare(
        &self,
        request: ChildThreadRequest,
    ) -> impl Future<Output = Result<StudioThreadSpec, ThreadAssemblyError>> + Send;

    /// Finishes published physical resources after the Thread and observation barrier close.
    /// Implementations must preserve resources by default and retain failed cleanup for retry.
    ///
    /// # Errors
    /// Returns the actual product resource shutdown error without dropping its owner.
    fn close_published(
        &self,
        _id: &str,
        _disposition: AgentWorkspaceDisposition,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send {
        async { Ok(()) }
    }

    /// Idempotently releases resources allocated under this trusted identity before publication.
    /// Missing resources are already released; failures must retain the factory's resource owner.
    fn discard_unpublished(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send;
}

/// Product resource preparation after resolving an enabled Profile from canonical configuration.
pub trait StudioChildResources: Send + Sync + std::fmt::Debug + 'static {
    /// Checks caller policy and limits, then prepares owned workspace/tool/storage resources.
    /// Initial instructions and their provenance must be captured from this resolved Profile/config snapshot.
    fn prepare(
        &self,
        request: &ChildThreadRequest,
        profile: &crate::config::ResolvedAgentProfile,
    ) -> impl Future<Output = Result<StudioThreadSpec, ThreadAssemblyError>> + Send;

    /// Finishes published physical resources after the Thread and observation barrier close.
    /// Implementations must preserve resources by default and retain failed cleanup for retry.
    ///
    /// # Errors
    /// Returns the actual product resource shutdown error without dropping its owner.
    fn close_published(
        &self,
        _id: &str,
        _disposition: AgentWorkspaceDisposition,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send {
        async { Ok(()) }
    }

    /// Idempotently releases resources allocated under this trusted identity before publication.
    /// Missing resources are already released; failures must retain the factory's resource owner.
    fn discard_unpublished(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send;
}

/// Profile resolution shared with Studio's settings owner; it has no secondary configuration state.
pub struct ConfiguredChildFactory<R> {
    config: crate::config::ConfigRuntime,
    resources: R,
}

impl<R: std::fmt::Debug> std::fmt::Debug for ConfiguredChildFactory<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfiguredChildFactory")
            .field("resources", &self.resources)
            .finish_non_exhaustive()
    }
}

impl<R: StudioChildResources> ConfiguredChildFactory<R> {
    /// Supplies the existing settings owner and the product resource allocator.
    pub fn new(config: crate::config::ConfigRuntime, resources: R) -> Self {
        Self { config, resources }
    }
}

impl<R: StudioChildResources> StudioChildFactory for ConfiguredChildFactory<R> {
    fn close_published(
        &self,
        id: &str,
        disposition: AgentWorkspaceDisposition,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send {
        self.resources.close_published(id, disposition)
    }

    fn discard_unpublished(
        &self,
        id: &str,
    ) -> impl Future<Output = Result<(), ThreadAssemblyError>> + Send {
        self.resources.discard_unpublished(id)
    }

    async fn prepare(
        &self,
        request: ChildThreadRequest,
    ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
        if request.cancellation.is_cancelled() {
            return Err(pl_core::thread::ThreadError::Cancelled.into());
        }
        let config = self.config.clone();
        let profile_id = request.profile_id.clone();
        let profile =
            tokio::task::spawn_blocking(move || config.resolve_agent_profile(&profile_id))
                .await??;
        if request.cancellation.is_cancelled() {
            return Err(pl_core::thread::ThreadError::Cancelled.into());
        }
        if request.writable_paths.is_some()
            && profile.profile.workspace_mode != pl_protocol::AgentWorkspaceMode::Directory
        {
            return Err(ThreadAssemblyError::WorkspaceScope(request.profile_id));
        }
        let mut spec = self.resources.prepare(&request, &profile).await?;
        if spec.id != request.id
            || !spec.history.is_empty()
            || spec.parent_id.as_deref() != Some(request.caller.as_str())
        {
            return Err(ThreadAssemblyError::Identity(spec.id));
        }
        spec.route = profile.route;
        Ok(spec)
    }
}

trait ErasedFactory: Send + Sync + std::fmt::Debug {
    fn close_published<'a>(
        &'a self,
        id: &'a str,
        disposition: AgentWorkspaceDisposition,
    ) -> BoxFuture<'a, Result<(), ThreadAssemblyError>>;

    fn discard_unpublished<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<(), ThreadAssemblyError>>;
    fn prepare(
        &self,
        request: ChildThreadRequest,
    ) -> BoxFuture<'_, Result<StudioThreadSpec, ThreadAssemblyError>>;
}
impl<T: StudioChildFactory> ErasedFactory for T {
    fn close_published<'a>(
        &'a self,
        id: &'a str,
        disposition: AgentWorkspaceDisposition,
    ) -> BoxFuture<'a, Result<(), ThreadAssemblyError>> {
        Box::pin(StudioChildFactory::close_published(self, id, disposition))
    }

    fn discard_unpublished<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, Result<(), ThreadAssemblyError>> {
        Box::pin(StudioChildFactory::discard_unpublished(self, id))
    }
    fn prepare(
        &self,
        request: ChildThreadRequest,
    ) -> BoxFuture<'_, Result<StudioThreadSpec, ThreadAssemblyError>> {
        Box::pin(StudioChildFactory::prepare(self, request))
    }
}

#[derive(Debug, Clone)]
pub(super) struct ChildFactory(Arc<dyn ErasedFactory>);
impl ChildFactory {
    pub(super) async fn close_published(
        &self,
        id: &str,
        disposition: AgentWorkspaceDisposition,
    ) -> Result<(), ThreadAssemblyError> {
        self.0.close_published(id, disposition).await
    }

    pub(super) async fn discard_unpublished(&self, id: &str) -> Result<(), ThreadAssemblyError> {
        self.0.discard_unpublished(id).await
    }
    pub(super) fn new(factory: impl StudioChildFactory) -> Self {
        Self(Arc::new(factory))
    }
    pub(super) async fn prepare(
        &self,
        request: ChildThreadRequest,
    ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
        self.0.prepare(request).await
    }
}

/// Preparation is a product-owned resource before the core Thread exists.
#[derive(Debug)]
pub(super) struct PreparingChild {
    pub(super) parent: String,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
}

pub(super) struct ChildReservation {
    registry: Arc<super::Registry>,
    pub(super) id: String,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
}

impl Drop for ChildReservation {
    fn drop(&mut self) {
        self.registry.state().preparing_children.remove(&self.id);
        self.registry.changed.notify_waiters();
    }
}

impl super::StudioThreadAssembler {
    pub(super) fn reserve_child(
        &self,
        context: &pl_core::tool::opaque::CallContext,
    ) -> Result<(ChildFactory, ChildReservation), ThreadAssemblyError> {
        const MAX_TREE_THREADS: usize = 16;
        let mut state = self.0.state();
        if state.closing || context.cancellation.is_cancelled() {
            return Err(ThreadAssemblyError::Closed);
        }
        let parent = state
            .entries
            .get(&context.thread_id)
            .filter(|entry| {
                entry.ready
                    && entry.thread.snapshot().lifecycle == pl_core::thread::ThreadLifecycle::Open
            })
            .ok_or_else(|| ThreadAssemblyError::Identity(context.thread_id.clone()))?;
        // Studio's current product policy permits only root agents to create children.
        if parent.parent_id.is_some() {
            return Err(ThreadAssemblyError::ChildDepth);
        }
        let factory = state
            .child_factory
            .clone()
            .ok_or(ThreadAssemblyError::Closed)?;
        let mut occupied = std::collections::BTreeSet::new();
        occupied.insert(context.thread_id.as_str());
        occupied.extend(
            state
                .entries
                .iter()
                .filter(|(_, entry)| entry.parent_id.as_deref() == Some(context.thread_id.as_str()))
                .map(|(id, _)| id.as_str()),
        );
        occupied.extend(
            state
                .creating
                .iter()
                .filter(|(_, preparation)| {
                    preparation.parent_id.as_deref() == Some(context.thread_id.as_str())
                })
                .map(|(id, _)| id.as_str()),
        );
        occupied.extend(
            state
                .preparing_children
                .iter()
                .filter(|(_, preparing)| preparing.parent == context.thread_id)
                .map(|(id, _)| id.as_str()),
        );
        occupied.extend(
            state
                .unpublished_children
                .iter()
                .filter(|(_, child)| child.parent == context.thread_id)
                .map(|(id, _)| id.as_str()),
        );
        if occupied.len() >= MAX_TREE_THREADS {
            return Err(ThreadAssemblyError::ChildCapacity(MAX_TREE_THREADS));
        }
        let id = crate::studio::new_id("thread");
        let cancellation = context.cancellation.child_token();
        state.unpublished_children.insert(
            id.clone(),
            UnpublishedChild {
                parent: context.thread_id.clone(),
                factory: factory.clone(),
                cleanup: Arc::new(CleanupGate::default()),
            },
        );
        state.preparing_children.insert(
            id.clone(),
            PreparingChild {
                parent: context.thread_id.clone(),
                cancellation: cancellation.clone(),
            },
        );
        Ok((
            factory,
            ChildReservation {
                registry: self.0.clone(),
                id,
                cancellation,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{StudioThreadAssembler, tests::spec};
    use super::*;

    #[derive(Debug)]
    struct RejectFactory;
    impl StudioChildFactory for RejectFactory {
        async fn discard_unpublished(&self, _id: &str) -> Result<(), ThreadAssemblyError> {
            Ok(())
        }
        async fn prepare(
            &self,
            _request: ChildThreadRequest,
        ) -> Result<StudioThreadSpec, ThreadAssemblyError> {
            Err(ThreadAssemblyError::Closed)
        }
    }

    fn context(id: &str) -> pl_core::tool::opaque::CallContext {
        pl_core::tool::opaque::CallContext {
            grant: Default::default(),
            thread_id: id.into(),
            turn_id: "turn".into(),
            call_id: "spawn".into(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        }
    }

    #[tokio::test]
    async fn preparing_children_reserve_tree_capacity_and_shutdown_waits_for_their_owners() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        owner.set_child_factory(RejectFactory).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        owner
            .assemble(spec("other", None, directory.path()))
            .await
            .unwrap();
        let mut reservations = Vec::new();
        for _ in 0..15 {
            reservations.push(owner.reserve_child(&context("root")).unwrap().1);
        }
        assert!(matches!(
            owner.reserve_child(&context("root")),
            Err(ThreadAssemblyError::ChildCapacity(16))
        ));
        let other = owner
            .reserve_child(&context("other"))
            .expect("separate roots have separate quotas")
            .1;
        let released = reservations.pop().unwrap();
        let released_id = released.id.clone();
        drop(released);
        owner.discard_unpublished_child(&released_id).await.unwrap();
        reservations.push(owner.reserve_child(&context("root")).unwrap().1);
        let cancelled = reservations[0].cancellation.clone();
        let closing = tokio::spawn({
            let owner = owner.clone();
            async move { owner.close_all().await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), cancelled.cancelled())
            .await
            .unwrap();
        assert!(
            !closing.is_finished(),
            "shutdown cannot discard unpublished resource owners"
        );
        assert!(owner.reserve_child(&context("other")).is_err());
        drop(reservations);
        drop(other);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), closing)
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert!(owner.0.state().preparing_children.is_empty());
    }

    #[tokio::test]
    async fn child_cannot_allocate_a_grandchild_preparation() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        owner.set_child_factory(RejectFactory).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        owner
            .assemble(spec("child", Some("root"), directory.path()))
            .await
            .unwrap();
        assert!(matches!(
            owner.reserve_child(&context("child")),
            Err(ThreadAssemblyError::ChildDepth)
        ));
        assert!(owner.0.state().preparing_children.is_empty());
        assert!(owner.close_all().await.is_empty());
    }
}
