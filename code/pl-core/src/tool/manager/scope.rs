//! Published tool scopes: atomic group replacement and conflict validation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use pl_protocol::{PureError, Result, ToolSpec};

use super::plan::ToolBinding;
use super::registration::{ToolRegistration, ToolRegistrationEntry};
use crate::tool::{DynTool, ToolExecution};

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

impl std::fmt::Display for ToolGroupId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

#[derive(Clone)]
pub(super) struct ToolScope {
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

pub(super) struct ToolScopeSnapshot {
    pub(super) bindings: Arc<[ToolBinding]>,
}

impl ToolScope {
    pub(super) fn new(label: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(ToolScopeInner {
                label: label.into(),
                state: RwLock::new(ToolScopeState::default()),
            }),
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.read().revision
    }

    pub(super) fn replace_batch(
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

    pub(super) fn snapshot(&self) -> ToolScopeSnapshot {
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

    pub(super) fn remove_generation(&self, group: &ToolGroupId, generation: u64) {
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
