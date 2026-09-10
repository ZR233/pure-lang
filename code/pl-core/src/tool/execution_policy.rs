//! Host-defined execution admission, independent from tool declarations and payload formats.
use super::opaque::{CallContext, ToolError};
use crate::context::OpaquePayload;
use futures::future::BoxFuture;
use std::{fmt, future::Future, sync::Arc};

/// Capabilities explicitly granted by trusted host policy for one invocation.
/// Names are owned by upper layers; core never infers them from dynamic input or saved history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionGrant {
    capabilities: std::collections::BTreeSet<Arc<str>>,
}
impl ExecutionGrant {
    /// Adds a host-defined capability without changing any Thread scheduling permission.
    pub fn with_capability(mut self, capability: impl Into<Arc<str>>) -> Self {
        self.capabilities.insert(capability.into());
        self
    }
    /// Lets a tool interpret only the capabilities defined by its own owner.
    pub fn contains(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Trusted host policy evaluated before invoking a frozen executor.
/// Approval UI, command classification and review models belong to the implementation.
/// Returning success grants this invocation; arbitrary payload fields do not grant permission.
pub trait ExecutionPolicy: fmt::Debug + Send + Sync + 'static {
    /// May wait for an owner-mediated decision and must honor the call's cancellation token.
    ///
    /// # Errors
    /// Denial or policy failure prevents the executor from being called. Observed review output
    /// may be attached to the error for canonical history without granting framework control.
    fn authorize(
        &self,
        input: &OpaquePayload,
        context: &CallContext,
    ) -> impl Future<Output = Result<ExecutionGrant, ToolError>> + Send;
}

trait ErasedPolicy: fmt::Debug + Send + Sync {
    fn authorize<'a>(
        &'a self,
        input: &'a OpaquePayload,
        context: &'a CallContext,
    ) -> BoxFuture<'a, Result<ExecutionGrant, ToolError>>;
}
impl<T: ExecutionPolicy> ErasedPolicy for T {
    fn authorize<'a>(
        &'a self,
        input: &'a OpaquePayload,
        context: &'a CallContext,
    ) -> BoxFuture<'a, Result<ExecutionGrant, ToolError>> {
        Box::pin(ExecutionPolicy::authorize(self, input, context))
    }
}

/// Immutable policy lease. Clone only when reconnecting under the same authorization policy.
/// A newly constructed lease revokes pending execution under the previous policy without
/// changing the model-visible tool declaration.
#[derive(Debug, Clone)]
pub struct ExecutionPolicyHandle(Arc<dyn ErasedPolicy>);
impl ExecutionPolicyHandle {
    /// Freezes one host policy implementation behind the centralized dynamic boundary.
    pub fn new(policy: impl ExecutionPolicy) -> Self {
        Self(Arc::new(policy))
    }
    pub(crate) fn identity(&self) -> PolicyIdentity {
        PolicyIdentity(Arc::downgrade(&self.0))
    }
    pub(crate) async fn authorize(
        &self,
        input: &OpaquePayload,
        context: &CallContext,
    ) -> Result<ExecutionGrant, ToolError> {
        self.0.authorize(input, context).await
    }
    pub(crate) fn same_policy(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(crate) fn same_policy(
    left: Option<&ExecutionPolicyHandle>,
    right: Option<&ExecutionPolicyHandle>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.same_policy(right),
        (None, Some(_)) | (Some(_), None) => false,
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PolicyIdentity(std::sync::Weak<dyn ErasedPolicy>);
impl PolicyIdentity {
    pub(crate) fn matches(&self, handle: &ExecutionPolicyHandle) -> bool {
        self.0.ptr_eq(&Arc::downgrade(&handle.0))
    }
}
