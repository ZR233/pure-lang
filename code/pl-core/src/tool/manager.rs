//! Per-agent tool ownership, registration and immutable model-step plans.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use pl_protocol::{PureError, Result, ToolDiscoveryState, ToolSpec};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    DynTool, StaticTool, StaticToolDefinition, ToolCallContext, ToolDirective, ToolExecution,
    ToolInput, ToolInvocation, ToolName, ToolPolicy, ToolResult,
};

type RefreshHandler =
    dyn Fn(ModelStepToolContext) -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync;

/// Identity and mutable registration window exposed immediately before one model step.
#[derive(Debug, Clone)]
pub struct ModelStepToolContext {
    /// Persistent registration scope that may be atomically refreshed.
    pub agent_tools: AgentToolSet,
    /// Current session identity.
    pub session_id: String,
    /// Current turn identity.
    pub turn_id: String,
    /// Zero-based model-step index within the turn.
    pub step: u32,
}

/// Host callback used to refresh an agent's tools before each model step.
#[derive(Clone)]
pub struct BeforeModelStepHook {
    handler: Arc<RefreshHandler>,
}

impl fmt::Debug for BeforeModelStepHook {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BeforeModelStepHook")
            .finish_non_exhaustive()
    }
}

impl BeforeModelStepHook {
    /// Creates a host refresh hook.
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(ModelStepToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            handler: Arc::new(move |context| Box::pin(handler(context))),
        }
    }

    /// Runs the hook for one model-step registration window.
    ///
    /// # Errors
    ///
    /// Propagates the host refresh error and aborts the model step.
    pub async fn refresh(&self, context: ModelStepToolContext) -> Result<()> {
        (self.handler)(context).await
    }
}

/// A stable registration group within one tool scope.
///
/// Hosts normally use one group per installer or external generation, for example
/// `builtin`, `lsp` or `mcp:<server>`. Replacing a group is atomic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolGroupId(Arc<str>);

impl ToolGroupId {
    /// Creates a stable source-owned group identity.
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(Arc::from(value.as_ref()))
    }

    /// Returns the group identity as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolGroupId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Whether a registered group is immediately visible to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExposure {
    /// Send the group's tools on every model step.
    Direct,
    /// Keep the group's tools in the searchable catalog until revealed.
    Deferred,
}

/// One atomic source-owned installation unit.
#[derive(Debug, Clone)]
pub struct ToolInstallGroup {
    id: ToolGroupId,
    tools: Vec<DynTool>,
    exposure: ToolExposure,
    developer_instructions: Option<Arc<str>>,
}

impl ToolInstallGroup {
    /// Creates an immediately visible group.
    pub fn direct(id: ToolGroupId, tools: Vec<DynTool>) -> Self {
        Self::new(id, tools, ToolExposure::Direct)
    }

    /// Creates a searchable deferred group.
    pub fn deferred(id: ToolGroupId, tools: Vec<DynTool>) -> Self {
        Self::new(id, tools, ToolExposure::Deferred)
    }

    /// Creates a group with an explicit exposure policy.
    pub fn new(id: ToolGroupId, tools: Vec<DynTool>, exposure: ToolExposure) -> Self {
        Self {
            id,
            tools,
            exposure,
            developer_instructions: None,
        }
    }

    /// Adds workflow guidance injected only while this group is visible.
    pub fn with_developer_instructions(mut self, instructions: impl Into<Arc<str>>) -> Self {
        let instructions = instructions.into();
        if !instructions.trim().is_empty() {
            self.developer_instructions = Some(instructions);
        }
        self
    }

    /// Returns the stable source-owned identity.
    pub fn id(&self) -> &ToolGroupId {
        &self.id
    }

    /// Returns the type-erased tools in installation order.
    pub fn tools(&self) -> &[DynTool] {
        &self.tools
    }

    /// Returns the model exposure policy.
    pub fn exposure(&self) -> ToolExposure {
        self.exposure
    }

    /// Returns non-empty group workflow guidance, when configured.
    pub fn developer_instructions(&self) -> Option<&str> {
        self.developer_instructions.as_deref()
    }
}

/// Whether an agent sees the manager's global tool scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalToolInheritance {
    /// Merge global tools into each frozen agent plan.
    Inherit,
    /// Freeze only the agent-local scope.
    Isolated,
}

/// The sole owner of global tools and the factory for isolated per-agent tool sets.
#[derive(Clone)]
pub struct ToolManager {
    inner: Arc<ToolManagerInner>,
}

struct ToolManagerInner {
    id: u64,
    global: ToolScope,
    next_generation: AtomicU64,
}

impl fmt::Debug for ToolManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolManager")
            .field("id", &self.inner.id)
            .field("global_revision", &self.inner.global.revision())
            .finish_non_exhaustive()
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        static NEXT_MANAGER_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_MANAGER_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::new(ToolManagerInner {
                id,
                global: ToolScope::new("global"),
                next_generation: AtomicU64::new(1),
            }),
        }
    }

    /// Creates a persistent tool set owned by one agent.
    pub fn agent_tool_set(
        &self,
        agent_id: impl Into<String>,
        inheritance: GlobalToolInheritance,
    ) -> AgentToolSet {
        let agent_id = agent_id.into();
        AgentToolSet {
            inner: Arc::new(AgentToolSetInner {
                manager: self.clone(),
                local: ToolScope::new(format!("agent:{agent_id}")),
                agent_id,
                inheritance,
                owned_registrations: std::sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Atomically replaces one global registration group.
    ///
    /// The returned guard owns that exact generation. Dropping an obsolete guard
    /// cannot unregister a newer replacement.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when the group is empty, a definition has
    /// an empty name, or two tools in the global scope have the same visible name.
    pub fn replace_global(&self, group: ToolInstallGroup) -> Result<ToolRegistration> {
        self.replace_global_batch(vec![group])
    }

    /// Atomically replaces multiple global registration groups.
    ///
    /// All definitions are validated against the final scope before one write is
    /// published. An empty tool vector removes the visible tools from that group
    /// while keeping the returned RAII ownership for the published generation.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when the batch is empty, repeats a group,
    /// contains an invalid definition, or creates any same-scope name conflict.
    pub fn replace_global_batch(&self, groups: Vec<ToolInstallGroup>) -> Result<ToolRegistration> {
        let generation = self.next_generation();
        self.inner.global.replace_batch(groups, generation)
    }

    /// Executes a local tool strictly through a frozen plan.
    ///
    /// # Errors
    ///
    /// Returns an error when the plan belongs to another manager, the name was not
    /// present in that step, or the definition is provider-hosted.
    pub async fn execute(
        &self,
        plan: &ToolPlan,
        name: &str,
        input: ToolInput,
        context: ToolCallContext,
    ) -> Result<ToolResult> {
        if plan.manager_id != self.inner.id {
            return Err(PureError::ConfigError(
                "tool plan belongs to a different ToolManager".to_string(),
            ));
        }
        let Some(binding) = plan.binding(name) else {
            return Err(PureError::ToolExecutionFailed {
                tool: name.to_string(),
                error: "tool is not present in the frozen model-step plan".to_string(),
            });
        };
        if binding.execution == ToolExecution::ProviderHosted {
            return Err(PureError::ToolExecutionFailed {
                tool: name.to_string(),
                error: "provider-hosted tool cannot be executed locally".to_string(),
            });
        }
        binding
            .tool
            .execute(ToolInvocation::new(input, context))
            .await
    }

    fn next_generation(&self) -> u64 {
        self.inner.next_generation.fetch_add(1, Ordering::Relaxed)
    }
}

/// A persistent, isolated set of tools visible to one agent.
#[derive(Clone)]
pub struct AgentToolSet {
    inner: Arc<AgentToolSetInner>,
}

struct AgentToolSetInner {
    manager: ToolManager,
    local: ToolScope,
    agent_id: String,
    inheritance: GlobalToolInheritance,
    owned_registrations: std::sync::Mutex<BTreeMap<ToolGroupId, ToolRegistration>>,
}

impl fmt::Debug for AgentToolSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentToolSet")
            .field("agent_id", &self.inner.agent_id)
            .field("inheritance", &self.inner.inheritance)
            .field("local_revision", &self.inner.local.revision())
            .finish_non_exhaustive()
    }
}

impl AgentToolSet {
    /// Returns the owner agent identity.
    pub fn agent_id(&self) -> &str {
        &self.inner.agent_id
    }

    /// Returns whether this scope inherits global groups.
    pub fn inheritance(&self) -> GlobalToolInheritance {
        self.inner.inheritance
    }

    /// Returns the manager that owns this scope.
    pub fn manager(&self) -> &ToolManager {
        &self.inner.manager
    }

    /// Atomically replaces one agent-local registration group.
    ///
    /// A local name may shadow an inherited global name. Duplicate names between
    /// two local groups reject the entire replacement and preserve the old group.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] for invalid names or same-scope conflicts.
    pub fn replace(&self, group: ToolInstallGroup) -> Result<ToolRegistration> {
        self.replace_batch(vec![group])
    }

    /// Atomically replaces multiple agent-local registration groups.
    ///
    /// The complete final local scope is validated before it is published. This is
    /// the registration window API for host refreshes that must update several
    /// dynamic sources without exposing a partial tool set.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when any group or definition is invalid,
    /// or when the resulting local scope contains a duplicate visible name.
    pub fn replace_batch(&self, groups: Vec<ToolInstallGroup>) -> Result<ToolRegistration> {
        let generation = self.inner.manager.next_generation();
        self.inner.local.replace_batch(groups, generation)
    }

    /// Atomically replaces a group whose lifetime is owned by this agent set.
    ///
    /// This is the normal installer API for persistent agents. Use [`Self::replace`]
    /// when an external owner needs explicit RAII unregistration.
    pub fn install(&self, group: ToolInstallGroup) -> Result<()> {
        self.install_batch(vec![group])?;
        Ok(())
    }

    /// Atomically replaces multiple groups owned by this persistent agent set.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] under the same conditions as
    /// [`Self::replace_batch`]. No owned registration is changed on failure.
    pub fn install_batch(&self, groups: Vec<ToolInstallGroup>) -> Result<()> {
        let registration = self.replace_batch(groups)?;
        let mut owned = self
            .inner
            .owned_registrations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for registration in registration.into_single_group_registrations() {
            owned.insert(registration.group().clone(), registration);
        }
        Ok(())
    }

    /// Removes a set-owned registration group. Returns whether it existed.
    pub fn uninstall(&self, group: &ToolGroupId) -> bool {
        self.inner
            .owned_registrations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(group)
            .is_some()
    }

    /// Freezes the exact definitions and executors used by one model step.
    ///
    /// Agent-local bindings shadow inherited global bindings by visible name. Both
    /// definitions and handlers remain alive until every clone of the plan drops.
    pub fn freeze(&self) -> ToolPlan {
        self.freeze_with_discovery(&ToolDiscoveryState::default())
    }

    /// Freezes one model-step snapshot using session-local deferred reveal state.
    pub fn freeze_with_discovery(&self, discovery: &ToolDiscoveryState) -> ToolPlan {
        let local = self.inner.local.snapshot();
        let global = match self.inner.inheritance {
            GlobalToolInheritance::Inherit => Some(self.inner.manager.inner.global.snapshot()),
            GlobalToolInheritance::Isolated => None,
        };
        ToolPlan::freeze(self.inner.manager.inner.id, global, local, discovery)
    }

    /// Returns immediately visible names without applying any deferred reveal state.
    pub fn tool_names(&self) -> Vec<String> {
        self.freeze().names().map(ToOwned::to_owned).collect()
    }
}

/// RAII ownership of one exact registration generation.
pub struct ToolRegistration {
    entries: Vec<ToolRegistrationEntry>,
}

struct ToolRegistrationEntry {
    scope: ToolScope,
    group: ToolGroupId,
    generation: u64,
}

impl fmt::Debug for ToolRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolRegistration")
            .field(
                "groups",
                &self
                    .entries
                    .iter()
                    .map(|entry| (&entry.group, entry.generation))
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl ToolRegistration {
    fn group(&self) -> &ToolGroupId {
        debug_assert_eq!(self.entries.len(), 1);
        &self.entries[0].group
    }

    fn into_single_group_registrations(mut self) -> Vec<Self> {
        std::mem::take(&mut self.entries)
            .into_iter()
            .map(|entry| Self {
                entries: vec![entry],
            })
            .collect()
    }
}

impl Drop for ToolRegistration {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        for entry in entries {
            entry
                .scope
                .remove_generation(&entry.group, entry.generation);
        }
    }
}

/// An immutable model-step snapshot.
#[derive(Clone)]
pub struct ToolPlan {
    manager_id: u64,
    bindings: Arc<[ToolBinding]>,
    specs: Arc<[ToolSpec]>,
    wire_fingerprint: Arc<str>,
    execution_fingerprint: Arc<str>,
    catalog_fingerprint: Option<Arc<str>>,
    deferred_names: Arc<[String]>,
    deferred_bindings: Arc<[ToolBinding]>,
    developer_instructions: Arc<[(ToolGroupId, Arc<str>)]>,
}

impl fmt::Debug for ToolPlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolPlan")
            .field("names", &self.names().collect::<Vec<_>>())
            .field("wire_fingerprint", &self.wire_fingerprint)
            .field("execution_fingerprint", &self.execution_fingerprint)
            .field("catalog_fingerprint", &self.catalog_fingerprint)
            .finish()
    }
}

impl ToolPlan {
    fn freeze(
        manager_id: u64,
        global: Option<ToolScopeSnapshot>,
        local: ToolScopeSnapshot,
        discovery: &ToolDiscoveryState,
    ) -> Self {
        let mut by_name = BTreeMap::<String, ToolBinding>::new();
        if let Some(global) = global {
            for binding in global.bindings.iter() {
                by_name.insert(binding.name().to_string(), binding.clone());
            }
        }
        for binding in local.bindings.iter() {
            by_name.insert(binding.name().to_string(), binding.clone());
        }
        let all_bindings = by_name.into_values().collect::<Vec<_>>();
        let deferred = all_bindings
            .iter()
            .filter(|binding| binding.exposure == ToolExposure::Deferred)
            .cloned()
            .collect::<Vec<_>>();
        let catalog_fingerprint = deferred_catalog_fingerprint(&deferred);
        let revealed = if catalog_fingerprint.as_deref().is_some_and(|fingerprint| {
            discovery.catalog_fingerprint.as_deref() == Some(fingerprint)
        }) {
            discovery
                .revealed_tool_names
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        } else {
            BTreeSet::new()
        };
        let mut visible = all_bindings
            .into_iter()
            .filter(|binding| {
                binding.exposure == ToolExposure::Direct || revealed.contains(binding.name())
            })
            .collect::<Vec<_>>();
        if let Some(fingerprint) = catalog_fingerprint.as_deref() {
            visible.push(tool_search_binding(&deferred, fingerprint));
            visible.sort_by(|left, right| left.name().cmp(right.name()));
        }
        Self::from_bindings(manager_id, visible, catalog_fingerprint, deferred)
    }

    fn from_bindings(
        manager_id: u64,
        bindings: Vec<ToolBinding>,
        catalog_fingerprint: Option<String>,
        deferred: Vec<ToolBinding>,
    ) -> Self {
        let programmatic_enabled = bindings
            .iter()
            .any(|binding| binding.spec.is_programmatic_tool_calling());
        let specs = crate::stable_tool_schemas(
            bindings
                .iter()
                .map(|binding| binding.model_spec(programmatic_enabled))
                .collect(),
        );
        let wire_fingerprint = fingerprint_json(&serde_json::to_value(&specs).unwrap_or_default());
        let execution_identity = bindings
            .iter()
            .map(|binding| {
                serde_json::json!({
                    "name": binding.name(),
                    "generation": binding.generation,
                })
            })
            .collect::<Vec<_>>();
        let execution_fingerprint = fingerprint_json(&serde_json::Value::Array(execution_identity));
        let mut instructions = BTreeMap::<ToolGroupId, Arc<str>>::new();
        for binding in &bindings {
            if let Some(instruction) = &binding.developer_instructions {
                instructions.insert(binding.group.clone(), instruction.clone());
            }
        }
        let deferred_names = deferred
            .iter()
            .map(|binding| binding.name().to_string())
            .collect::<Vec<_>>();
        Self {
            manager_id,
            bindings: bindings.into(),
            specs: specs.into(),
            wire_fingerprint: Arc::from(wire_fingerprint),
            execution_fingerprint: Arc::from(execution_fingerprint),
            catalog_fingerprint: catalog_fingerprint.map(Arc::from),
            deferred_names: deferred_names.into(),
            deferred_bindings: deferred.into(),
            developer_instructions: instructions.into_iter().collect::<Vec<_>>().into(),
        }
    }

    /// 只向模型暴露当前执行策略实际允许调用的工具。
    pub(crate) fn allowed_by(&self, policy: Option<&crate::AgentExecutionPolicy>) -> Self {
        let Some(policy) = policy else {
            return self.clone();
        };
        let bindings = self
            .bindings
            .iter()
            .filter(|binding| policy.allows_effect(binding.tool().policy().effect()))
            .cloned()
            .collect::<Vec<_>>();
        if bindings.len() == self.bindings.len() {
            return self.clone();
        }
        let deferred = self
            .deferred_bindings
            .iter()
            .filter(|binding| policy.allows_effect(binding.tool().policy().effect()))
            .cloned()
            .collect();
        Self::from_bindings(
            self.manager_id,
            bindings,
            self.catalog_fingerprint.as_deref().map(ToOwned::to_owned),
            deferred,
        )
    }

    /// Returns the stable provider-facing specifications for this model step.
    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// Iterates provider-visible names in stable order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.bindings.iter().map(ToolBinding::name)
    }

    /// Returns the fingerprint of provider-visible schemas.
    pub fn wire_fingerprint(&self) -> &str {
        &self.wire_fingerprint
    }

    /// Returns the fingerprint including frozen executor generations.
    pub fn execution_fingerprint(&self) -> &str {
        &self.execution_fingerprint
    }

    /// Returns the current deferred catalog fingerprint, when present.
    pub fn catalog_fingerprint(&self) -> Option<&str> {
        self.catalog_fingerprint.as_deref()
    }

    /// Removes reveals that do not belong to this exact deferred generation.
    pub fn normalized_discovery_state(&self, discovery: &ToolDiscoveryState) -> ToolDiscoveryState {
        let Some(fingerprint) = self.catalog_fingerprint() else {
            return ToolDiscoveryState::default();
        };
        if discovery.catalog_fingerprint.as_deref() != Some(fingerprint) {
            return ToolDiscoveryState {
                catalog_fingerprint: Some(fingerprint.to_string()),
                revealed_tool_names: Vec::new(),
            };
        }
        let deferred = self
            .deferred_names
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        ToolDiscoveryState {
            catalog_fingerprint: Some(fingerprint.to_string()),
            revealed_tool_names: discovery
                .revealed_tool_names
                .iter()
                .filter(|name| deferred.contains(name.as_str()))
                .cloned()
                .collect(),
        }
    }

    /// Iterates guidance for groups that are visible in this frozen plan.
    pub fn developer_instructions(&self) -> impl Iterator<Item = (&ToolGroupId, &str)> {
        self.developer_instructions
            .iter()
            .map(|(group, instructions)| (group, instructions.as_ref()))
    }

    /// Returns the frozen executor generation for a visible tool.
    pub fn executor_generation(&self, name: &str) -> Option<u64> {
        self.binding(name).map(|binding| binding.generation)
    }

    pub(crate) fn binding(&self, name: &str) -> Option<&ToolBinding> {
        self.bindings
            .binary_search_by(|binding| binding.name().cmp(name))
            .ok()
            .map(|index| &self.bindings[index])
    }
}

#[derive(Clone)]
pub(crate) struct ToolBinding {
    spec: ToolSpec,
    tool: DynTool,
    execution: ToolExecution,
    programmatic_eligible: bool,
    generation: u64,
    group: ToolGroupId,
    exposure: ToolExposure,
    developer_instructions: Option<Arc<str>>,
}

impl fmt::Debug for ToolBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolBinding")
            .field("name", &self.name())
            .field("execution", &self.execution)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl ToolBinding {
    pub(crate) fn name(&self) -> &str {
        self.spec.name()
    }

    pub(crate) fn tool(&self) -> &DynTool {
        &self.tool
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    fn model_spec(&self, programmatic_enabled: bool) -> ToolSpec {
        if !programmatic_enabled || !self.programmatic_eligible {
            return self.spec.clone();
        }
        let output_schema = match &self.spec {
            ToolSpec::Function { output_schema, .. } | ToolSpec::Custom { output_schema, .. } => {
                output_schema.clone().unwrap_or_else(|| {
                    serde_json::json!({
                        "type": "object",
                        "additionalProperties": true,
                    })
                })
            }
            ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => {
                return self.spec.clone();
            }
        };
        self.spec.clone().allow_programmatic(output_schema)
    }
}

#[derive(Clone)]
struct ToolScope {
    inner: Arc<ToolScopeInner>,
}

struct ToolScopeInner {
    label: String,
    state: RwLock<ToolScopeState>,
}

#[derive(Default)]
struct ToolScopeState {
    revision: u64,
    groups: BTreeMap<ToolGroupId, PublishedGroup>,
}

struct PublishedGroup {
    generation: u64,
    bindings: Arc<[ToolBinding]>,
}

struct ToolScopeSnapshot {
    bindings: Arc<[ToolBinding]>,
}

impl ToolScope {
    fn new(label: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(ToolScopeInner {
                label: label.into(),
                state: RwLock::new(ToolScopeState::default()),
            }),
        }
    }

    fn revision(&self) -> u64 {
        self.read().revision
    }

    fn replace_batch(
        &self,
        groups: Vec<ToolInstallGroup>,
        generation: u64,
    ) -> Result<ToolRegistration> {
        if groups.is_empty() {
            return Err(PureError::ConfigError(
                "tool registration batch cannot be empty".to_string(),
            ));
        }
        let mut group_ids = BTreeSet::new();
        let mut prepared = Vec::with_capacity(groups.len());
        for install_group in groups {
            let ToolInstallGroup {
                id: group,
                tools,
                exposure,
                developer_instructions,
            } = install_group;
            if group.as_str().trim().is_empty() {
                return Err(PureError::ConfigError(
                    "tool registration group cannot be empty".to_string(),
                ));
            }
            if !group_ids.insert(group.clone()) {
                return Err(PureError::ConfigError(format!(
                    "tool registration batch repeats group `{group}`"
                )));
            }
            let bindings = freeze_bindings(
                tools,
                generation,
                group.clone(),
                exposure,
                developer_instructions,
            )?;
            prepared.push((group, bindings));
        }
        let mut state = self.write();
        validate_batch_scope_conflicts(&self.inner.label, &prepared, &state.groups)?;
        for (group, bindings) in &prepared {
            state.groups.insert(
                group.clone(),
                PublishedGroup {
                    generation,
                    bindings: bindings.clone().into(),
                },
            );
        }
        state.revision = state.revision.saturating_add(1);
        drop(state);
        Ok(ToolRegistration {
            entries: prepared
                .into_iter()
                .map(|(group, _)| ToolRegistrationEntry {
                    scope: self.clone(),
                    group,
                    generation,
                })
                .collect(),
        })
    }

    fn snapshot(&self) -> ToolScopeSnapshot {
        let state = self.read();
        let mut bindings = state
            .groups
            .values()
            .flat_map(|group| group.bindings.iter().cloned())
            .collect::<Vec<_>>();
        drop(state);
        bindings.sort_by(|left, right| left.name().cmp(right.name()));
        ToolScopeSnapshot {
            bindings: bindings.into(),
        }
    }

    fn remove_generation(&self, group: &ToolGroupId, generation: u64) {
        let mut state = self.write();
        if state
            .groups
            .get(group)
            .is_some_and(|published| published.generation == generation)
        {
            state.groups.remove(group);
            state.revision = state.revision.saturating_add(1);
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, ToolScopeState> {
        self.inner
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, ToolScopeState> {
        self.inner
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn freeze_bindings(
    tools: Vec<DynTool>,
    generation: u64,
    group: ToolGroupId,
    exposure: ToolExposure,
    developer_instructions: Option<Arc<str>>,
) -> Result<Vec<ToolBinding>> {
    let mut names = BTreeSet::new();
    let mut bindings = Vec::with_capacity(tools.len());
    for tool in tools {
        let spec = tool.definition().spec().clone();
        let policy = tool.policy();
        validate_registered_tool(&tool, &spec)?;
        let programmatic_eligible = policy.supports_programmatic_calls();
        if programmatic_eligible {
            if tool.execution() != ToolExecution::Local
                || policy.effect() != Some(crate::ToolEffect::Read)
            {
                return Err(PureError::ConfigError(format!(
                    "tool `{}` declares programmatic callers without a local Read effect",
                    spec.name()
                )));
            }
            if matches!(
                spec,
                ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. }
            ) {
                return Err(PureError::ConfigError(format!(
                    "provider-hosted tool `{}` cannot declare programmatic callers",
                    spec.name()
                )));
            }
        }
        let name = spec.name();
        if name.trim().is_empty() {
            return Err(PureError::ConfigError(
                "tool definition contains an empty visible name".to_string(),
            ));
        }
        if !names.insert(name.to_string()) {
            return Err(PureError::ConfigError(format!(
                "tool replacement contains duplicate name `{name}`"
            )));
        }
        bindings.push(ToolBinding {
            execution: tool.execution(),
            spec,
            tool,
            programmatic_eligible,
            generation,
            group: group.clone(),
            exposure,
            developer_instructions: developer_instructions.clone(),
        });
    }
    bindings.sort_by(|left, right| left.name().cmp(right.name()));
    Ok(bindings)
}

fn validate_registered_tool(tool: &DynTool, spec: &ToolSpec) -> Result<()> {
    match (tool.execution(), spec) {
        (
            ToolExecution::Local,
            ToolSpec::Function {
                description,
                input_schema,
                ..
            },
        ) => {
            if description.trim().is_empty() {
                return Err(PureError::ConfigError(format!(
                    "function tool `{}` must have a non-empty description",
                    spec.name()
                )));
            }
            if !input_schema.as_object().is_some_and(|schema| {
                schema.get("type").and_then(serde_json::Value::as_str) == Some("object")
            }) {
                return Err(PureError::ConfigError(format!(
                    "function tool `{}` input schema must have an object root",
                    spec.name()
                )));
            }
        }
        (ToolExecution::Local, ToolSpec::Custom { description, .. }) => {
            if description.trim().is_empty() {
                return Err(PureError::ConfigError(format!(
                    "custom tool `{}` must have a non-empty description",
                    spec.name()
                )));
            }
        }
        (ToolExecution::Local, ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. }) => {
            return Err(PureError::ConfigError(format!(
                "provider-hosted tool `{}` cannot use local execution",
                spec.name()
            )));
        }
        (
            ToolExecution::ProviderHosted,
            ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. },
        ) => {}
        (ToolExecution::ProviderHosted, ToolSpec::Function { .. } | ToolSpec::Custom { .. }) => {
            return Err(PureError::ConfigError(format!(
                "function tool `{}` must use local execution",
                spec.name()
            )));
        }
    }
    Ok(())
}

fn validate_batch_scope_conflicts(
    scope: &str,
    replacements: &[(ToolGroupId, Vec<ToolBinding>)],
    groups: &BTreeMap<ToolGroupId, PublishedGroup>,
) -> Result<()> {
    let replacing_groups = replacements
        .iter()
        .map(|(group, _)| group)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeMap::<&str, &ToolGroupId>::new();
    for (group, published) in groups {
        if replacing_groups.contains(group) {
            continue;
        }
        for binding in published.bindings.iter() {
            owners.insert(binding.name(), group);
        }
    }
    for (group, bindings) in replacements {
        for binding in bindings {
            if let Some(owner) = owners.insert(binding.name(), group) {
                return Err(PureError::ConfigError(format!(
                    "tool `{}` conflicts in {scope} scope between groups `{owner}` and `{group}`",
                    binding.name()
                )));
            }
        }
    }
    Ok(())
}

fn deferred_catalog_fingerprint(bindings: &[ToolBinding]) -> Option<String> {
    if bindings.is_empty() {
        return None;
    }
    let entries = bindings
        .iter()
        .map(|binding| {
            serde_json::json!({
                "name": binding.name(),
                "spec": binding.spec,
                "generation": binding.generation,
                "group": binding.group.as_str(),
            })
        })
        .collect::<Vec<_>>();
    Some(fingerprint_json(&serde_json::Value::Array(entries)))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolSearchInput {
    /// Search terms matched against deferred tool names and descriptions.
    #[schemars(length(min = 1, max = 512))]
    query: String,
    /// Maximum number of matching tools to reveal for the next model step.
    #[schemars(range(min = 1, max = 20))]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolSearchMatch {
    name: String,
    description: String,
}

#[derive(Debug)]
struct ToolSearchTool {
    catalog_fingerprint: String,
    entries: Vec<ToolSearchMatch>,
}

impl StaticTool for ToolSearchTool {
    type Input = ToolSearchInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("tool_search"),
            "Search deferred tool metadata and reveal matching tools for the next model step.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only().with_parallel_tool_calls()
    }

    fn execute(
        &self,
        input: Self::Input,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            let query = input.query.trim().to_ascii_lowercase();
            if query.is_empty() {
                return Err(PureError::ToolExecutionFailed {
                    tool: "tool_search".to_string(),
                    error: "query cannot be empty".to_string(),
                });
            }
            let terms = query.split_whitespace().collect::<Vec<_>>();
            let mut matches = self
                .entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.to_ascii_lowercase();
                    let description = entry.description.to_ascii_lowercase();
                    let score = terms.iter().fold(0_u64, |score, term| {
                        score
                            + u64::from(name.contains(term)) * 4
                            + u64::from(description.contains(term))
                    });
                    (score > 0).then_some((score, entry))
                })
                .collect::<Vec<_>>();
            matches.sort_by(|(left_score, left), (right_score, right)| {
                right_score
                    .cmp(left_score)
                    .then_with(|| left.name.cmp(&right.name))
            });
            let matches = matches
                .into_iter()
                .take(input.limit.unwrap_or(8).min(20))
                .map(|(_, entry)| entry.clone())
                .collect::<Vec<_>>();
            let mut result = ToolResult::json(serde_json::json!({
                "matches": matches,
                "revealedForNextModelStep": true,
            }))?;
            result.runtime_events.push(ToolDirective::RevealTools {
                catalog_fingerprint: self.catalog_fingerprint.clone(),
                tool_names: matches.into_iter().map(|entry| entry.name).collect(),
            });
            Ok(result)
        }
    }
}

fn tool_search_binding(deferred: &[ToolBinding], catalog_fingerprint: &str) -> ToolBinding {
    let entries = deferred
        .iter()
        .map(|binding| ToolSearchMatch {
            name: binding.name().to_string(),
            description: tool_spec_description(&binding.spec).to_string(),
        })
        .collect();
    let tool: DynTool = ToolSearchTool {
        catalog_fingerprint: catalog_fingerprint.to_string(),
        entries,
    }
    .into();
    ToolBinding {
        spec: tool.definition().spec().clone(),
        execution: tool.execution(),
        programmatic_eligible: false,
        generation: deferred
            .iter()
            .map(|binding| binding.generation)
            .max()
            .unwrap_or_default(),
        group: ToolGroupId::new("tool-search"),
        exposure: ToolExposure::Direct,
        developer_instructions: None,
        tool,
    }
}

fn tool_spec_description(spec: &ToolSpec) -> &str {
    match spec {
        ToolSpec::Function { description, .. } | ToolSpec::Custom { description, .. } => {
            description
        }
        ToolSpec::ProgrammaticToolCalling | ToolSpec::WebSearch { .. } => "Provider-hosted tool",
    }
}

fn fingerprint_json(value: &serde_json::Value) -> String {
    crate::canonical_json_hash(value)
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::turn::ToolEffect;

    #[derive(Debug)]
    struct NamedTool {
        name: &'static str,
        output: &'static str,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EmptyPolicyInput {}

    impl NamedTool {
        fn arc(name: &'static str, output: &'static str) -> DynTool {
            Self { name, output }.into()
        }
    }

    impl StaticTool for NamedTool {
        type Input = serde_json::Value;

        fn definition(&self) -> StaticToolDefinition {
            StaticToolDefinition::new(ToolName::builtin(self.name), self.name)
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only()
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl Future<Output = Result<ToolResult>> + Send {
            async move { Ok(ToolResult::success(self.output)) }.boxed()
        }
    }

    fn policy_tool(name: &'static str, policy: ToolPolicy) -> DynTool {
        crate::tool::static_tool::<EmptyPolicyInput>(StaticToolDefinition::new(
            ToolName::bare(name).unwrap(),
            name,
        ))
        .policy(policy)
        .build(|_input, _context| async { Ok(ToolResult::success("ok")) })
    }

    fn group(id: &'static str, tools: Vec<DynTool>) -> ToolInstallGroup {
        ToolInstallGroup::direct(ToolGroupId::new(id), tools)
    }

    #[test]
    fn local_tools_shadow_inherited_globals() {
        let manager = ToolManager::new();
        let _global = manager
            .replace_global(group("global", vec![NamedTool::arc("shared", "global")]))
            .unwrap();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Inherit);
        let _local = tools
            .replace(group("local", vec![NamedTool::arc("shared", "local")]))
            .unwrap();

        let plan = tools.freeze();

        assert_eq!(plan.names().collect::<Vec<_>>(), vec!["shared"]);
        assert_ne!(
            plan.executor_generation("shared"),
            manager
                .agent_tool_set("other", GlobalToolInheritance::Inherit)
                .freeze()
                .executor_generation("shared")
        );
    }

    #[test]
    fn global_inheritance_is_explicit_and_agents_are_isolated() {
        let manager = ToolManager::new();
        let _global = manager
            .replace_global(group("global", vec![NamedTool::arc("shared", "global")]))
            .expect("publish global tool");
        let inherited = manager.agent_tool_set("inherited", GlobalToolInheritance::Inherit);
        let isolated = manager.agent_tool_set("isolated", GlobalToolInheritance::Isolated);
        let other = manager.agent_tool_set("other", GlobalToolInheritance::Inherit);
        inherited
            .install(group("local", vec![NamedTool::arc("private", "inherited")]))
            .expect("install private tool");

        assert_eq!(inherited.tool_names(), vec!["private", "shared"]);
        assert!(isolated.tool_names().is_empty());
        assert_eq!(other.tool_names(), vec!["shared"]);
    }

    #[tokio::test]
    async fn dropping_a_registration_unpublishes_only_new_plans() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let registration = tools
            .replace(group(
                "ephemeral",
                vec![NamedTool::arc("ephemeral", "old handler")],
            ))
            .expect("register ephemeral tool");
        let old_plan = tools.freeze();
        drop(registration);

        assert!(tools.freeze().specs().is_empty());
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let output = manager
            .execute(
                &old_plan,
                "ephemeral",
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                ToolCallContext::test(event_tx),
            )
            .await
            .expect("old immutable plan retains its handler");
        assert_eq!(output.canonical_output(), "old handler");
    }

    #[test]
    fn conflict_rolls_back_the_whole_group() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        let _first = tools
            .replace(group("first", vec![NamedTool::arc("alpha", "old")]))
            .unwrap();
        let _second = tools
            .replace(group("second", vec![NamedTool::arc("beta", "old")]))
            .unwrap();

        let error = tools
            .replace(group(
                "second",
                vec![
                    NamedTool::arc("alpha", "conflict"),
                    NamedTool::arc("gamma", "partial"),
                ],
            ))
            .unwrap_err();

        assert!(error.to_string().contains("alpha"));
        assert_eq!(tools.tool_names(), vec!["alpha", "beta"]);
    }

    #[test]
    fn multi_group_batch_is_atomic_and_its_registration_is_raii_owned() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        tools
            .install_batch(vec![
                group("first", vec![NamedTool::arc("alpha", "old alpha")]),
                group("second", vec![NamedTool::arc("beta", "old beta")]),
            ])
            .expect("install initial groups");

        let error = tools
            .replace_batch(vec![
                group("first", vec![NamedTool::arc("gamma", "new gamma")]),
                group("third", vec![NamedTool::arc("beta", "conflict")]),
            ])
            .expect_err("cross-group conflict rejects the whole batch");
        assert!(error.to_string().contains("beta"));
        assert_eq!(tools.tool_names(), vec!["alpha", "beta"]);

        let registration = tools
            .replace_batch(vec![
                group("first", vec![NamedTool::arc("gamma", "new gamma")]),
                group("second", Vec::new()),
            ])
            .expect("replace both groups atomically");
        assert_eq!(tools.tool_names(), vec!["gamma"]);
        drop(registration);
        assert!(tools.tool_names().is_empty());
    }

    #[test]
    fn registration_order_does_not_change_wire_fingerprint() {
        let manager = ToolManager::new();
        let first = manager.agent_tool_set("first", GlobalToolInheritance::Isolated);
        let second = manager.agent_tool_set("second", GlobalToolInheritance::Isolated);
        let _first = first
            .replace(group(
                "tools",
                vec![NamedTool::arc("zeta", "z"), NamedTool::arc("alpha", "a")],
            ))
            .unwrap();
        let _second = second
            .replace(group(
                "tools",
                vec![NamedTool::arc("alpha", "a"), NamedTool::arc("zeta", "z")],
            ))
            .unwrap();

        assert_eq!(
            first.freeze().wire_fingerprint(),
            second.freeze().wire_fingerprint()
        );
        assert_ne!(
            first.freeze().execution_fingerprint(),
            second.freeze().execution_fingerprint()
        );
    }

    #[test]
    fn execution_policy_removes_disallowed_tools_from_the_model_plan() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("reviewer", GlobalToolInheritance::Isolated);
        let read = policy_tool("read", ToolPolicy::read_only());
        let write = policy_tool(
            "write",
            ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite),
        );
        let _registration = tools.replace(group("tools", vec![read, write])).unwrap();
        let policy = crate::AgentExecutionPolicy {
            allowed_effects: crate::ToolEffectSet::from_effects([ToolEffect::Read]),
            ..Default::default()
        };

        let plan = tools.freeze().allowed_by(Some(&policy));

        assert_eq!(plan.names().collect::<Vec<_>>(), vec!["read"]);
        assert_eq!(
            plan.specs().iter().map(ToolSpec::name).collect::<Vec<_>>(),
            vec!["read"]
        );
    }

    #[test]
    fn programmatic_callers_are_projected_only_with_the_hosted_coordinator() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let read = policy_tool("lookup", ToolPolicy::read_only().with_programmatic_calls());
        tools
            .install(group("read", vec![read]))
            .expect("install eligible read tool");

        let direct = tools.freeze();
        let ToolSpec::Function {
            allowed_callers, ..
        } = &direct.specs()[0]
        else {
            panic!("lookup must remain a function tool");
        };
        assert!(allowed_callers.is_empty());

        tools
            .install(group(
                "programmatic",
                vec![DynTool::new_executor(
                    crate::tool::ProgrammaticToolCallingTool::default(),
                )],
            ))
            .expect("install hosted coordinator");
        let hosted = tools.freeze();
        let ToolSpec::Function {
            allowed_callers,
            output_schema,
            ..
        } = hosted
            .specs()
            .iter()
            .find(|spec| spec.name() == "lookup")
            .expect("lookup spec")
        else {
            panic!("lookup must remain a function tool");
        };
        assert_eq!(
            allowed_callers,
            &[
                pl_protocol::ToolCallerMode::Direct,
                pl_protocol::ToolCallerMode::Programmatic
            ]
        );
        assert!(output_schema.is_some());
    }

    #[test]
    fn non_read_tool_cannot_claim_programmatic_eligibility() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let write = policy_tool(
            "write",
            ToolPolicy::default()
                .with_effect(ToolEffect::WorkspaceWrite)
                .with_programmatic_calls(),
        );

        let error = tools
            .replace(group("write", vec![write]))
            .expect_err("write tool must not be programmatic");

        assert!(error.to_string().contains("local Read effect"));
        assert!(tools.freeze().specs().is_empty());
    }

    #[test]
    fn obsolete_guard_cannot_unregister_a_replacement() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        let old = tools
            .replace(group("tools", vec![NamedTool::arc("old", "old")]))
            .unwrap();
        let new = tools
            .replace(group("tools", vec![NamedTool::arc("new", "new")]))
            .unwrap();

        drop(old);
        assert_eq!(tools.tool_names(), vec!["new"]);
        drop(new);
        assert!(tools.tool_names().is_empty());
    }
}
