//! Persistent per-agent tool scopes and global inheritance policy.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::{fmt, sync};

use pl_protocol::{Result, ToolDiscoveryState};

use super::registration::ToolRegistration;
use super::scope::{ToolInstallGroup, ToolScope};
use super::{ToolManager, ToolPlan};

/// Whether an agent sees the manager's global tool scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalToolInheritance {
    /// Merge global tools into each frozen agent plan.
    Inherit,
    /// Freeze only the agent-local scope.
    Isolated,
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
    owned_registrations: sync::Mutex<BTreeMap<crate::tool::ToolGroupId, ToolRegistration>>,
}

impl AgentToolSet {
    pub(super) fn new(
        manager: ToolManager,
        agent_id: String,
        inheritance: GlobalToolInheritance,
    ) -> Self {
        Self {
            inner: Arc::new(AgentToolSetInner {
                local: ToolScope::new(format!("agent:{agent_id}")),
                manager,
                agent_id,
                inheritance,
                owned_registrations: sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }
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
    /// Returns [`pl_protocol::PureError::ConfigError`] for invalid names or
    /// same-scope conflicts.
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
    /// Returns [`pl_protocol::PureError::ConfigError`] when any group or
    /// definition is invalid, or when the resulting local scope contains a
    /// duplicate visible name.
    pub fn replace_batch(&self, groups: Vec<ToolInstallGroup>) -> Result<ToolRegistration> {
        let generation = self.inner.manager.next_generation();
        self.inner.local.replace_batch(groups, generation)
    }

    /// Atomically replaces a group whose lifetime is owned by this agent set.
    ///
    /// This is the normal installer API for persistent agents. Use [`Self::replace`]
    /// when an external owner needs explicit RAII unregistration.
    ///
    /// # Errors
    ///
    /// Returns [`pl_protocol::PureError::ConfigError`] under the same conditions
    /// as [`Self::replace_batch`].
    pub fn install(&self, group: ToolInstallGroup) -> Result<()> {
        self.install_batch(vec![group])?;
        Ok(())
    }

    /// Atomically replaces multiple groups owned by this persistent agent set.
    ///
    /// # Errors
    ///
    /// Returns [`pl_protocol::PureError::ConfigError`] under the same conditions as
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
    pub fn uninstall(&self, group: &crate::tool::ToolGroupId) -> bool {
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
