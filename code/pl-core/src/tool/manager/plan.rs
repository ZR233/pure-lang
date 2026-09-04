//! Immutable model-step plans frozen from one agent tool scope.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use pl_protocol::{ToolDiscoveryState, ToolSpec};

use super::fingerprint_json;
use super::scope::{ToolExposure, ToolGroupId, ToolScopeSnapshot};
use super::search_tool::tool_search_binding;
use crate::tool::{DynTool, ToolExecution};

/// An immutable model-step snapshot.
#[derive(Clone)]
pub struct ToolPlan {
    pub(super) manager_id: u64,
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
    pub(super) fn freeze(
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
        self.binding(name).map(ToolBinding::generation)
    }

    pub(crate) fn binding(&self, name: &str) -> Option<&ToolBinding> {
        self.bindings
            .binary_search_by(|binding| binding.name().cmp(name))
            .ok()
            .map(|index| &self.bindings[index])
    }
}

/// One frozen tool definition with its exact executor generation.
///
/// Construction only happens inside the manager boundary while publishing a
/// scope or freezing the deferred search tool.
#[derive(Clone)]
pub(crate) struct ToolBinding {
    pub(super) spec: ToolSpec,
    pub(super) tool: DynTool,
    pub(super) execution: ToolExecution,
    pub(super) programmatic_eligible: bool,
    pub(super) generation: u64,
    pub(super) group: ToolGroupId,
    pub(super) exposure: ToolExposure,
    pub(super) developer_instructions: Option<Arc<str>>,
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
