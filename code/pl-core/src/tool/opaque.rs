//! Protocol-independent dynamic tools and immutable execution plans.
use super::ToolOutput;
use crate::{context::OpaquePayload, model::ModelToolDeclaration};
use futures::future::BoxFuture;
use std::{collections::BTreeMap, fmt, future::Future, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Scope assigned by Thread; tools cannot choose their context role or call identity.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// Invocation-only capabilities returned by the registered host policy.
    pub grant: super::execution_policy::ExecutionGrant,
    /// Immutable context actually admitted for the model request that produced this call.
    pub context: crate::context::ContextSnapshot,
    /// Model-owned material frozen with the request that produced this call.
    pub model_projection: Option<OpaquePayload>,
    pub tasks: Option<crate::thread::TaskAccess>,
    pub thread_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub cancellation: CancellationToken,
    pub extensions: Arc<BTreeMap<String, crate::thread::extensions::ExtensionRecord>>,
    pub catalog: Arc<[ModelToolDeclaration]>,
    pub extension_sequence: u64,
}

impl CallContext {
    /// Identity that core will assign if this call commits one interaction request.
    /// Computing it does not create an interaction or grant any control permission.
    pub fn interaction_id(&self) -> String {
        tool_interaction_id(&self.call_id)
    }
}

pub(crate) fn tool_interaction_id(call_id: &str) -> String {
    format!("tool-interaction:{call_id}")
}

/// Minimal tool implementation. Parsing and model-visible projection belong to the producer.
pub trait Tool: Send + Sync + fmt::Debug + 'static {
    /// Executes dynamic input through a Thread-frozen executor.
    fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> impl Future<Output = Result<ToolOutput, ToolError>> + Send;

    /// Stops owned resources. Shared underlying services must only release this tool's lease.
    /// A failed close retains the instance for retry; implementations must make retries safe.
    fn close(&self) -> impl Future<Output = Result<(), ToolError>> + Send {
        async { Ok(()) }
    }
}

/// Tool-specific failure retained with its original source.
#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[error("{source}")]
pub struct ToolError {
    // The canonical output is serialized once by ToolDelivery; this copy is an execution lease.
    #[serde(skip)]
    observed_output: Option<Box<ToolOutput>>,
    #[source]
    #[serde(with = "crate::error_record::required")]
    pub source: Box<dyn std::error::Error + Send + Sync>,
}

impl ToolError {
    /// Preserves the producer's typed error at the dynamic tool boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
            observed_output: None,
        }
    }

    /// Attaches observed facts without granting any successful control or state transition.
    pub fn with_output(mut self, output: ToolOutput) -> Self {
        self.observed_output = Some(Box::new(output));
        self
    }

    pub(crate) fn observed_output(&self) -> Option<&ToolOutput> {
        self.observed_output.as_deref()
    }
}

trait Executor: Send + Sync + fmt::Debug {
    fn close(&self) -> BoxFuture<'_, Result<(), ToolError>>;
    fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> BoxFuture<'_, Result<ToolOutput, ToolError>>;
}
impl<T: Tool> Executor for T {
    fn close(&self) -> BoxFuture<'_, Result<(), ToolError>> {
        Box::pin(Tool::close(self))
    }
    fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
        Box::pin(Tool::execute(self, input, context))
    }
}

/// A declaration and fresh executor transferred into exactly one Thread registry.
/// Physical services may be shared explicitly inside the tool through service leases.
#[derive(Debug)]
pub struct Registration {
    binding: FrozenTool,
}

/// A transfer lease keeps rejected batches available even when the mailbox drops a message.
#[derive(Debug, Clone)]
pub(crate) struct RegistrationBatch(Arc<std::sync::Mutex<Option<Vec<Registration>>>>);
impl RegistrationBatch {
    pub(crate) fn new(tools: Vec<Registration>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(tools))))
    }
    pub(crate) fn take(&self) -> Option<Vec<Registration>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// Failed cleanup of tools rejected before installation; retains the exact instances for retry.
#[derive(Debug, thiserror::Error)]
#[error("Thread rejected tool registration; candidate cleanup failed: {source}")]
pub struct RejectedTools {
    rejection: Option<Box<crate::thread::ThreadError>>,
    #[source]
    source: Arc<ToolError>,
    resources: ToolManager,
}
impl RejectedTools {
    /// Original registration refusal, preserved independently of the cleanup failure.
    pub fn rejection(&self) -> Option<&crate::thread::ThreadError> {
        self.rejection.as_deref()
    }

    pub(crate) fn with_rejection(mut self, rejection: crate::thread::ThreadError) -> Self {
        self.rejection = Some(Box::new(rejection));
        self
    }

    /// Retries only the unfinished closes on the original rejected batch.
    /// # Errors
    /// Preserves the next failed close and retains all unfinished owners.
    pub async fn retry_close(&mut self) -> Result<(), Arc<ToolError>> {
        self.resources.close().await.map_err(|error| {
            self.source = Arc::new(error);
            self.source.clone()
        })
    }
}

pub(crate) async fn close_rejected(tools: Vec<Registration>) -> Result<(), RejectedTools> {
    let mut resources = ToolManager::default();
    resources.retain_candidates(&tools);
    resources.close().await.map_err(|source| RejectedTools {
        rejection: None,
        source: Arc::new(source),
        resources,
    })
}

pub(crate) struct TaskPermissions {
    pub(crate) authorization: Option<ToolAuthorization>,
    pub(crate) wait: bool,
    pub(crate) cancel: bool,
}

/// Opaque, non-secret identity of a host authorization policy. It is never part of model declarations.
#[derive(Debug, Clone)]
pub struct ToolAuthorization(Arc<AuthorizationIdentity>);

#[derive(Debug)]
enum AuthorizationIdentity {
    Named(Arc<str>),
    Unique,
    Scoped {
        parent: ToolAuthorization,
        revision: u64,
    },
}

impl PartialEq for ToolAuthorization {
    fn eq(&self, other: &Self) -> bool {
        if Arc::ptr_eq(&self.0, &other.0) {
            return true;
        }
        match (self.0.as_ref(), other.0.as_ref()) {
            (AuthorizationIdentity::Named(left), AuthorizationIdentity::Named(right)) => {
                left == right
            }
            (
                AuthorizationIdentity::Scoped {
                    parent: left,
                    revision: left_revision,
                },
                AuthorizationIdentity::Scoped {
                    parent: right,
                    revision: right_revision,
                },
            ) => left == right && left_revision == right_revision,
            (AuthorizationIdentity::Unique, _)
            | (AuthorizationIdentity::Named(_), _)
            | (AuthorizationIdentity::Scoped { .. }, _) => false,
        }
    }
}
impl Eq for ToolAuthorization {}

impl ToolAuthorization {
    /// The host must change this identity when an earlier policy must no longer authorize new execution.
    pub fn new(identity: impl Into<Arc<str>>) -> Self {
        Self(Arc::new(AuthorizationIdentity::Named(identity.into())))
    }
    /// Creates a process-local authority whose clones retain the same identity without counters or addresses.
    pub fn unique() -> Self {
        Self(Arc::new(AuthorizationIdentity::Unique))
    }
    /// Binds a revision to this authority; equal revisions from other authorities remain distinct.
    pub fn scoped(&self, revision: u64) -> Self {
        Self(Arc::new(AuthorizationIdentity::Scoped {
            parent: self.clone(),
            revision,
        }))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ToolScheduling {
    #[default]
    Task,
    Foreground,
    ForegroundSolo,
}

/// Internal execution lease: only the owning Thread can freeze or clone it.
#[derive(Debug, Clone)]
pub(crate) struct FrozenTool {
    execution_policy: Option<super::execution_policy::ExecutionPolicyHandle>,
    authorization: Option<ToolAuthorization>,
    declaration: ModelToolDeclaration,
    executor: Arc<dyn Executor>,
    deferred: bool,
    may_end_turn: bool,
    may_update_extensions: bool,
    may_reveal_tools: bool,
    may_request_interaction: bool,
    may_wait_tasks: bool,
    may_cancel_tasks: bool,
    scheduling: ToolScheduling,
}
/// Non-owning execution authority; retaining a call capability cannot retain its executor.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionAuthority {
    tool_id: String,
    authorization: Option<ToolAuthorization>,
    execution_policy: Option<super::execution_policy::PolicyIdentity>,
    may_end_turn: bool,
    may_update_extensions: bool,
    may_reveal_tools: bool,
    may_request_interaction: bool,
    may_wait_tasks: bool,
    may_cancel_tasks: bool,
    solo: bool,
}
impl ExecutionAuthority {
    pub(crate) fn remains_authorized(&self, manager: &ToolManager) -> bool {
        manager.tools.get(&self.tool_id).is_some_and(|current| {
            self.authorization == current.authorization
                && match (&self.execution_policy, &current.execution_policy) {
                    (None, None) => true,
                    (Some(identity), Some(policy)) => identity.matches(policy),
                    (None, Some(_)) | (Some(_), None) => false,
                }
                && (!self.may_end_turn || current.may_end_turn)
                && (!self.may_update_extensions || current.may_update_extensions)
                && (!self.may_reveal_tools || current.may_reveal_tools)
                && (!self.may_request_interaction || current.may_request_interaction)
                && (!self.may_wait_tasks || current.may_wait_tasks)
                && (!self.may_cancel_tasks || current.may_cancel_tasks)
                && (!current.requires_solo_call() || self.solo)
        })
    }
}

impl Registration {
    /// Stable routing identity for host-owned policy and catalog assembly.
    pub fn tool_id(&self) -> &str {
        &self.binding.declaration.tool_id
    }

    /// Requires host authorization before this executor runs; declaration bytes remain unchanged.
    pub fn with_execution_policy(
        mut self,
        policy: super::execution_policy::ExecutionPolicyHandle,
    ) -> Self {
        self.binding.execution_policy = Some(policy);
        self
    }

    /// Associates host policy identity with execution, independently from declaration bytes and reconnects.
    pub fn with_authorization(mut self, authorization: ToolAuthorization) -> Self {
        self.binding.authorization = Some(authorization);
        self
    }

    /// Requires this invocation to be the response's sole tool call and finish in the foreground.
    /// Grants no framework control permissions and does not drain existing background work.
    pub fn foreground(mut self) -> Self {
        self.binding.scheduling = ToolScheduling::ForegroundSolo;
        self
    }

    /// Waits for this invocation to finish before dispatching the next call in the same batch.
    /// Inherits Turn cancellation while permitting coexistence with ordinary calls. This is not
    /// a filesystem transaction or a barrier for existing background work. Explicit Solo
    /// registration and framework control permissions remain Solo.
    pub fn foreground_coexisting(mut self) -> Self {
        if self.binding.scheduling != ToolScheduling::ForegroundSolo {
            self.binding.scheduling = ToolScheduling::Foreground;
        }
        self
    }

    /// Grants read-only task waiting and requires foreground exclusive execution.
    pub fn with_task_waiting(mut self) -> Self {
        self.binding.may_wait_tasks = true;
        self
    }
    /// Grants task cancellation and requires foreground exclusive execution.
    pub fn with_task_cancellation(mut self) -> Self {
        self.binding.may_cancel_tasks = true;
        self
    }

    /// Keeps this declaration out of model requests until explicitly revealed by its stable ID.
    pub fn deferred(mut self) -> Self {
        self.binding.deferred = true;
        self
    }

    /// Grants host-interaction authority and requires an exclusive model call.
    pub fn with_interactions(mut self) -> Self {
        self.binding.may_request_interaction = true;
        self
    }

    /// Grants explicit deferred-tool discovery authority and requires a solo call.
    pub fn with_tool_discovery(mut self) -> Self {
        self.binding.may_reveal_tools = true;
        self
    }

    /// Grants extension CAS authority and requires this tool to execute alone.
    pub fn with_extension_updates(mut self) -> Self {
        self.binding.may_update_extensions = true;
        self
    }

    /// Grants explicit Turn-ending authority and requires this tool to be called alone.
    pub fn with_turn_completion(mut self) -> Self {
        self.binding.may_end_turn = true;
        self
    }

    /// Registers an open payload format without schema decoding.
    ///
    /// # Errors
    /// Rejects empty stable tool identities.
    pub fn new(
        id: String,
        declaration: OpaquePayload,
        tool: impl Tool,
    ) -> Result<Self, RegistryError> {
        if id.is_empty() {
            return Err(RegistryError::EmptyIdentity);
        }
        Ok(Self {
            binding: FrozenTool {
                execution_policy: None,
                authorization: None,
                declaration: ModelToolDeclaration {
                    tool_id: id,
                    declaration,
                },
                executor: Arc::new(tool),
                deferred: false,
                may_end_turn: false,
                may_update_extensions: false,
                may_reveal_tools: false,
                may_request_interaction: false,
                may_wait_tasks: false,
                may_cancel_tasks: false,
                scheduling: ToolScheduling::Task,
            },
        })
    }
}

impl FrozenTool {
    pub(crate) fn remains_authorized(&self, manager: &ToolManager) -> bool {
        self.authority().remains_authorized(manager)
    }

    pub(crate) fn authority(&self) -> ExecutionAuthority {
        ExecutionAuthority {
            tool_id: self.declaration.tool_id.clone(),
            authorization: self.authorization.clone(),
            execution_policy: self
                .execution_policy
                .as_ref()
                .map(super::execution_policy::ExecutionPolicyHandle::identity),
            may_end_turn: self.may_end_turn,
            may_update_extensions: self.may_update_extensions,
            may_reveal_tools: self.may_reveal_tools,
            may_request_interaction: self.may_request_interaction,
            may_wait_tasks: self.may_wait_tasks,
            may_cancel_tasks: self.may_cancel_tasks,
            solo: self.requires_solo_call(),
        }
    }

    pub(crate) fn task_permissions(&self) -> TaskPermissions {
        TaskPermissions {
            authorization: self.authorization.clone(),
            wait: self.may_wait_tasks,
            cancel: self.may_cancel_tasks,
        }
    }
    pub(crate) fn requires_solo_call(&self) -> bool {
        self.scheduling == ToolScheduling::ForegroundSolo
            || self.may_end_turn
            || self.may_update_extensions
            || self.may_reveal_tools
            || self.may_request_interaction
            || self.may_wait_tasks
            || self.may_cancel_tasks
    }
    pub(crate) fn requires_foreground_execution(&self) -> bool {
        self.scheduling != ToolScheduling::Task || self.requires_solo_call()
    }

    pub(crate) fn validate_control(&self, output: &ToolOutput) -> Result<(), ToolError> {
        if (output.control() == super::ToolControl::EndTurn && !self.may_end_turn)
            || (!output.extension_mutations().is_empty() && !self.may_update_extensions)
            || (!output.revealed_tools().is_empty() && !self.may_reveal_tools)
            || (output.control() == super::ToolControl::AwaitInteraction
                && !self.may_request_interaction)
            || ((output.control() == super::ToolControl::AwaitInteraction)
                != output.interaction().is_some())
        {
            Err(ToolError::new(ControlDenied))
        } else {
            Ok(())
        }
    }

    pub(crate) async fn execute(
        &self,
        input: OpaquePayload,
        mut context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        crate::error_record::catch_boundary("tool execution", async {
            if context.cancellation.is_cancelled() {
                return Err(ToolError::new(crate::thread::ThreadError::Cancelled));
            }
            context.grant = match &self.execution_policy {
                Some(policy) => policy.authorize(&input, &context).await?,
                None => Default::default(),
            };
            if let Some(access) = &context.tasks {
                access.validate_execution().await.map_err(ToolError::new)?;
            }
            if context.cancellation.is_cancelled() {
                return Err(ToolError::new(crate::thread::ThreadError::Cancelled));
            }
            self.executor.execute(input, context).await
        })
        .await
        .map_err(ToolError::new)?
    }
}

#[derive(Debug, thiserror::Error)]
#[error("tool returned framework control without registered authority")]
struct ControlDenied;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("tool identity is empty")]
    EmptyIdentity,
    #[error("unknown tool identity: {0}")]
    UnknownIdentity(String),
    #[error("duplicate tool identity in registration batch: {0}")]
    DuplicateIdentity(String),
}

/// Thread-local registry; no global scope or inherited mutable tool instances.
#[derive(Debug, Default)]
pub(crate) struct ToolManager {
    tools: BTreeMap<String, FrozenTool>,
    owned_instances: Vec<Arc<dyn Executor>>,
    closed_instances: usize,
    revealed: BTreeMap<String, ModelToolDeclaration>,
}
impl ToolManager {
    pub(crate) fn permits_task_cancellation(
        &self,
        id: &str,
        authorization: Option<&ToolAuthorization>,
    ) -> bool {
        self.tools.get(id).is_some_and(|tool| {
            tool.may_cancel_tasks && tool.authorization.as_ref() == authorization
        })
    }

    /// Takes lifecycle ownership before validating a candidate registry transaction.
    pub(crate) fn retain_candidates(&mut self, registrations: &[Registration]) {
        for registration in registrations {
            let executor = &registration.binding.executor;
            if !self
                .owned_instances
                .iter()
                .any(|owned| Arc::ptr_eq(owned, executor))
            {
                self.owned_instances.push(executor.clone());
            }
        }
    }

    pub(crate) fn replace(
        &mut self,
        registrations: Vec<Registration>,
    ) -> Result<(), RegistryError> {
        self.retain_candidates(&registrations);
        let mut tools = BTreeMap::new();
        for registration in registrations {
            let id = registration.binding.declaration.tool_id.clone();
            if tools.insert(id.clone(), registration.binding).is_some() {
                return Err(RegistryError::DuplicateIdentity(id));
            }
        }
        self.revealed.retain(|id, declaration| {
            tools
                .get(id)
                .is_some_and(|tool| tool.deferred && &tool.declaration == declaration)
        });
        self.tools = tools;
        Ok(())
    }
    pub(crate) fn patch(
        &mut self,
        remove: &[String],
        registrations: Vec<Registration>,
    ) -> Result<(), RegistryError> {
        self.retain_candidates(&registrations);
        let mut changed = BTreeMap::new();
        for registration in registrations {
            let id = registration.binding.declaration.tool_id.clone();
            if changed.insert(id.clone(), registration).is_some() {
                return Err(RegistryError::DuplicateIdentity(id));
            }
        }
        let mut next = self
            .tools
            .iter()
            .filter(|(id, _)| !remove.contains(id) && !changed.contains_key(*id))
            .map(|(_, binding)| Registration {
                binding: binding.clone(),
            })
            .collect::<Vec<_>>();
        next.extend(changed.into_values());
        self.replace(next)
    }

    pub(crate) async fn close(&mut self) -> Result<(), ToolError> {
        while let Some(tool) = self.owned_instances.get(self.closed_instances) {
            crate::error_record::catch_boundary("tool close", async { tool.close().await })
                .await
                .map_err(ToolError::new)??;
            self.closed_instances += 1;
        }
        self.tools.clear();
        self.revealed.clear();
        self.owned_instances.clear();
        self.closed_instances = 0;
        Ok(())
    }

    pub(crate) fn catalog(&self) -> Arc<[ModelToolDeclaration]> {
        self.tools
            .values()
            .map(|tool| tool.declaration.clone())
            .collect::<Vec<_>>()
            .into()
    }

    pub(crate) fn with_discovery(declarations: &[ModelToolDeclaration]) -> Self {
        Self {
            revealed: declarations
                .iter()
                .map(|declaration| (declaration.tool_id.clone(), declaration.clone()))
                .collect(),
            ..Default::default()
        }
    }

    pub(crate) fn validate_reveal(&self, ids: &[String]) -> Result<(), RegistryError> {
        for id in ids {
            if !self.tools.contains_key(id) {
                return Err(RegistryError::UnknownIdentity(id.clone()));
            }
        }
        Ok(())
    }

    pub(crate) fn reveal(&mut self, ids: &[String]) -> Result<(), RegistryError> {
        self.validate_reveal(ids)?;
        for id in ids {
            if let Some(tool) = self.tools.get(id).filter(|tool| tool.deferred) {
                self.revealed.insert(id.clone(), tool.declaration.clone());
            }
        }
        Ok(())
    }
    pub(crate) fn discovery(&self) -> Arc<[ModelToolDeclaration]> {
        self.revealed.values().cloned().collect::<Vec<_>>().into()
    }

    pub(crate) fn freeze(&self) -> ToolPlan {
        ToolPlan {
            tools: self
                .tools
                .iter()
                .filter(|(id, tool)| !tool.deferred || self.revealed.contains_key(*id))
                .map(|(id, tool)| (id.clone(), tool.clone()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ToolPlan {
    tools: BTreeMap<String, FrozenTool>,
}
impl ToolPlan {
    pub(crate) fn call_mode(&self) -> crate::model::ToolCallMode {
        if !self.tools.is_empty() && self.tools.values().all(FrozenTool::requires_solo_call) {
            crate::model::ToolCallMode::Sequential
        } else {
            crate::model::ToolCallMode::Parallel
        }
    }

    pub(crate) fn solo_tool_ids(&self) -> Arc<[String]> {
        self.tools
            .iter()
            .filter(|(_, tool)| tool.requires_solo_call())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
            .into()
    }

    pub(crate) fn remains_authorized(&self, manager: &ToolManager) -> bool {
        self.tools
            .values()
            .all(|tool| tool.remains_authorized(manager))
    }

    pub(crate) fn retry_compatible(&self, current: &Self) -> bool {
        self.tools.len() == current.tools.len()
            && self.tools.iter().all(|(id, previous)| {
                current.tools.get(id).is_some_and(|next| {
                    previous.declaration == next.declaration
                        && previous.authorization == next.authorization
                        && super::execution_policy::same_policy(
                            previous.execution_policy.as_ref(),
                            next.execution_policy.as_ref(),
                        )
                        && previous.may_end_turn == next.may_end_turn
                        && previous.may_update_extensions == next.may_update_extensions
                        && previous.may_reveal_tools == next.may_reveal_tools
                        && previous.may_request_interaction == next.may_request_interaction
                        && previous.may_wait_tasks == next.may_wait_tasks
                        && previous.may_cancel_tasks == next.may_cancel_tasks
                        && previous.scheduling == next.scheduling
                })
            })
    }

    pub(crate) fn declarations(&self) -> Arc<[ModelToolDeclaration]> {
        self.tools
            .values()
            .map(|registration| registration.declaration.clone())
            .collect::<Vec<_>>()
            .into()
    }
    pub(crate) fn get(&self, id: &str) -> Option<&FrozenTool> {
        self.tools.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Echo(&'static str);
    impl Tool for Echo {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new(OpaquePayload::text(self.0), Vec::new()))
        }
    }
    #[tokio::test]
    async fn frozen_plan_retains_executor_after_registry_replacement() {
        let declaration = OpaquePayload::text("opaque schema");
        let mut manager = ToolManager::default();
        manager
            .replace(vec![
                Registration::new("tool".into(), declaration.clone(), Echo("first")).unwrap(),
            ])
            .unwrap();
        let old = manager.freeze();
        manager
            .replace(vec![
                Registration::new("tool".into(), declaration, Echo("second")).unwrap(),
            ])
            .unwrap();
        let current = manager.freeze();
        assert_eq!(old.declarations(), current.declarations());
        let context = CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: CancellationToken::new(),
            extensions: Arc::new(BTreeMap::new()),
            extension_sequence: 0,
            catalog: Arc::from([]),
        };
        let result = old
            .get("tool")
            .unwrap()
            .execute(OpaquePayload::text("raw"), context.clone())
            .await
            .unwrap();
        assert_eq!(result.payload().content(), "first");
        let result = current
            .get("tool")
            .unwrap()
            .execute(OpaquePayload::text("raw"), context)
            .await
            .unwrap();
        assert_eq!(result.payload().content(), "second");
    }
    #[derive(Debug)]
    struct ClosingTool {
        calls: Arc<std::sync::atomic::AtomicUsize>,
        fail_first: bool,
    }
    impl Tool for ClosingTool {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::new(OpaquePayload::text("result"), Vec::new()))
        }
        async fn close(&self) -> Result<(), ToolError> {
            let count = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_first && count == 0 {
                Err(ToolError::new(std::io::Error::other("retry close")))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn closing_includes_retired_instances_and_retries_only_unfinished_ones() {
        let first = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let second = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut manager = ToolManager::default();
        manager
            .replace(vec![
                Registration::new(
                    "tool".into(),
                    OpaquePayload::text("schema"),
                    ClosingTool {
                        calls: first.clone(),
                        fail_first: false,
                    },
                )
                .unwrap(),
            ])
            .unwrap();
        manager
            .replace(vec![
                Registration::new(
                    "tool".into(),
                    OpaquePayload::text("schema"),
                    ClosingTool {
                        calls: second.clone(),
                        fail_first: true,
                    },
                )
                .unwrap(),
            ])
            .unwrap();
        assert!(manager.close().await.is_err());
        manager.close().await.unwrap();
        assert_eq!(first.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(second.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(manager.freeze().declarations().is_empty());
    }
    #[test]
    fn invalid_replacement_keeps_the_existing_thread_registry_and_other_threads_isolated() {
        let registration = || {
            Registration::new("tool".into(), OpaquePayload::text("schema"), Echo("value")).unwrap()
        };
        let mut first = ToolManager::default();
        let mut second = ToolManager::default();
        first.replace(vec![registration()]).unwrap();
        second.replace(vec![registration()]).unwrap();
        let before = first.freeze();
        assert!(matches!(
            first.replace(vec![registration(), registration()]),
            Err(RegistryError::DuplicateIdentity(_))
        ));
        assert_eq!(first.freeze().declarations(), before.declarations());
        assert!(!Arc::ptr_eq(
            &first.freeze().get("tool").unwrap().executor,
            &second.freeze().get("tool").unwrap().executor
        ));
        first.replace(Vec::new()).unwrap();
        assert!(first.freeze().declarations().is_empty());
        assert_eq!(second.freeze().declarations().len(), 1);
    }
    #[test]
    fn frozen_catalog_preserves_parallel_ordinary_calls_and_separately_marks_solo_tools() {
        let mut manager = ToolManager::default();
        let ordinary = || {
            Registration::new("read".into(), OpaquePayload::text("schema"), Echo("result")).unwrap()
        };
        manager.replace(vec![ordinary()]).unwrap();
        let parallel = manager.freeze();
        assert_eq!(parallel.call_mode(), crate::model::ToolCallMode::Parallel);
        manager
            .replace(vec![ordinary().with_turn_completion()])
            .unwrap();
        assert_eq!(
            manager.freeze().call_mode(),
            crate::model::ToolCallMode::Sequential
        );
        assert_eq!(parallel.call_mode(), crate::model::ToolCallMode::Parallel);
        manager
            .patch(
                &[],
                vec![
                    Registration::new(
                        "control".into(),
                        OpaquePayload::text("schema"),
                        Echo("result"),
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
        assert_eq!(
            manager.freeze().call_mode(),
            crate::model::ToolCallMode::Parallel
        );
        assert_eq!(manager.freeze().solo_tool_ids().as_ref(), &["read"]);
        assert!(parallel.solo_tool_ids().is_empty());
    }

    #[test]
    fn dynamic_payload_cannot_grant_turn_completion_and_control_requires_registration() {
        let output = ToolOutput::new(
            OpaquePayload::text(r#"{"endTurn":true,"approved":true}"#),
            Vec::new(),
        );
        let ordinary =
            Registration::new("tool".into(), OpaquePayload::text("schema"), Echo("result"))
                .unwrap();
        ordinary.binding.validate_control(&output).unwrap();
        assert_eq!(output.control(), super::super::ToolControl::Continue);
        assert!(
            ordinary
                .binding
                .validate_control(&output.clone().ending_turn())
                .is_err()
        );
        let permitted = ordinary.with_turn_completion();
        permitted
            .binding
            .validate_control(&output.ending_turn())
            .unwrap();
        assert!(permitted.binding.requires_solo_call());
    }
    #[test]
    fn permission_revocation_invalidates_retry_even_when_declaration_is_unchanged() {
        let registration = || {
            Registration::new(
                "tool".into(),
                OpaquePayload::text("same schema"),
                Echo("value"),
            )
            .unwrap()
        };
        let mut manager = ToolManager::default();
        manager
            .replace(vec![registration().with_turn_completion()])
            .unwrap();
        let old = manager.freeze();
        manager
            .replace(vec![registration().with_turn_completion()])
            .unwrap();
        assert!(old.retry_compatible(&manager.freeze()));
        manager.replace(vec![registration()]).unwrap();
        assert_eq!(old.declarations(), manager.freeze().declarations());
        assert!(!old.retry_compatible(&manager.freeze()));
    }
    #[test]
    fn revealed_tools_survive_reconnection_but_not_declaration_changes_or_removal() {
        let registration = |schema: &str| {
            Registration::new(
                "deferred".into(),
                OpaquePayload::text(schema.to_owned()),
                Echo("result"),
            )
            .unwrap()
            .deferred()
        };
        let mut manager = ToolManager::default();
        manager.replace(vec![registration("schema")]).unwrap();
        assert!(manager.freeze().declarations().is_empty());
        assert!(
            manager
                .reveal(&["deferred".into(), "missing".into()])
                .is_err()
        );
        assert!(manager.freeze().declarations().is_empty());
        manager.reveal(&["deferred".into()]).unwrap();
        let visible = manager.freeze().declarations();
        let saved = manager.discovery();
        manager.replace(vec![registration("schema")]).unwrap();
        assert_eq!(manager.freeze().declarations(), visible);
        let mut restored = ToolManager::with_discovery(&saved);
        restored.replace(vec![registration("schema")]).unwrap();
        assert_eq!(restored.freeze().declarations(), visible);
        restored.replace(vec![registration("new schema")]).unwrap();
        assert!(restored.freeze().declarations().is_empty());
        manager.replace(Vec::new()).unwrap();
        assert!(manager.discovery().is_empty());
    }
    #[test]
    fn authorization_changes_revoke_execution_without_changing_model_declarations() {
        let authority = ToolAuthorization::unique();
        let old_policy = authority.scoped(1);
        let new_policy = authority.scoped(2);
        assert_eq!(old_policy, authority.scoped(1));
        assert_ne!(old_policy, ToolAuthorization::unique().scoped(1));
        let registration = |authorization| {
            Registration::new(
                "tool".into(),
                OpaquePayload::text("stable declaration"),
                Echo("result"),
            )
            .unwrap()
            .with_task_cancellation()
            .with_authorization(authorization)
        };
        let mut manager = ToolManager::default();
        manager
            .replace(vec![registration(old_policy.clone())])
            .unwrap();
        let old = manager.freeze();
        manager
            .replace(vec![registration(old_policy.clone())])
            .unwrap();
        assert!(old.retry_compatible(&manager.freeze()));
        assert!(old.remains_authorized(&manager));
        manager
            .replace(vec![registration(new_policy.clone())])
            .unwrap();
        assert_eq!(old.declarations(), manager.freeze().declarations());
        assert!(!old.retry_compatible(&manager.freeze()));
        assert!(!old.remains_authorized(&manager));
        assert!(!manager.permits_task_cancellation("tool", Some(&old_policy)));
        assert!(manager.permits_task_cancellation("tool", Some(&new_policy)));
    }
    #[derive(Debug)]
    struct DenyExecution;
    impl super::super::execution_policy::ExecutionPolicy for DenyExecution {
        async fn authorize(
            &self,
            _: &OpaquePayload,
            _: &CallContext,
        ) -> Result<super::super::execution_policy::ExecutionGrant, ToolError> {
            Err(ToolError::new(std::io::Error::other(
                "host denied execution",
            )))
        }
    }

    #[derive(Debug)]
    struct CountExecutions(Arc<std::sync::atomic::AtomicUsize>);
    impl Tool for CountExecutions {
        async fn execute(&self, _: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ToolOutput::new(OpaquePayload::text("executed"), Vec::new()))
        }
    }

    #[tokio::test]
    async fn payload_approval_cannot_bypass_registered_execution_policy() {
        let executions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let registration = Registration::new(
            "guarded".into(),
            OpaquePayload::text("declaration"),
            CountExecutions(executions.clone()),
        )
        .unwrap()
        .with_execution_policy(super::super::execution_policy::ExecutionPolicyHandle::new(
            DenyExecution,
        ));
        let context = CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: CancellationToken::new(),
            extensions: Arc::new(BTreeMap::new()),
            extension_sequence: 0,
            catalog: Arc::from([]),
        };
        assert!(
            registration
                .binding
                .execute(OpaquePayload::text(r#"{"approved":true}"#), context)
                .await
                .is_err()
        );
        assert_eq!(executions.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn execution_policy_replacement_revokes_a_frozen_plan_without_changing_its_schema() {
        use super::super::execution_policy::ExecutionPolicyHandle;
        let policy = ExecutionPolicyHandle::new(DenyExecution);
        let registration = |policy| {
            Registration::new(
                "guarded".into(),
                OpaquePayload::text("same declaration"),
                Echo("result"),
            )
            .unwrap()
            .with_execution_policy(policy)
        };
        let mut manager = ToolManager::default();
        manager.replace(vec![registration(policy.clone())]).unwrap();
        let original = manager.freeze();
        manager.replace(vec![registration(policy)]).unwrap();
        assert!(original.remains_authorized(&manager));
        manager
            .replace(vec![registration(ExecutionPolicyHandle::new(
                DenyExecution,
            ))])
            .unwrap();
        assert_eq!(original.declarations(), manager.freeze().declarations());
        assert!(!original.remains_authorized(&manager));
        assert!(!original.retry_compatible(&manager.freeze()));
    }
}
