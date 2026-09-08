use crate::{AgentWorkspace, ExecutionEnvironment};

/// Physical execution scope frozen when a session's resources are assembled.
#[derive(Debug, Clone)]
pub struct SessionWorkspaceBinding {
    workspace: AgentWorkspace,
    environment: ExecutionEnvironment,
    host_identity: String,
}

impl SessionWorkspaceBinding {
    /// Binds a workspace and environment to a host-defined backend identity.
    pub fn new(
        workspace: AgentWorkspace,
        environment: ExecutionEnvironment,
        host_identity: String,
    ) -> Self {
        Self {
            workspace,
            environment,
            host_identity,
        }
    }

    /// Returns the workspace used by the session's executors.
    pub fn workspace(&self) -> &AgentWorkspace {
        &self.workspace
    }

    /// Returns the execution environment used by the session's executors.
    pub fn environment(&self) -> &ExecutionEnvironment {
        &self.environment
    }

    /// Returns the host fingerprint to validate before resolving new resources.
    pub fn host_identity(&self) -> &str {
        &self.host_identity
    }
}
