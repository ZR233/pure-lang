use std::path::PathBuf;

use anyhow::{Result, bail};
use pl_core::{AgentIdentity, AgentWorkspace, WorkspaceMutability, resolve_workspace_root};

use crate::studio::records::{ProjectRecord, ThreadRecord};

/// 所有模式和 Agent Profile 共享同一个项目 workspace，不创建工作树或分支隔离。
#[derive(Clone)]
pub(super) struct AgentWorkspaceResolver;

impl AgentWorkspaceResolver {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) async fn resolve(
        &self,
        identity: &AgentIdentity,
        _thread: &ThreadRecord,
        project: &ProjectRecord,
    ) -> Result<AgentWorkspace> {
        let root = if project.ssh_server_id.is_some() {
            normalized_remote_path(&project.path)?
        } else {
            resolve_workspace_root(&PathBuf::from(&project.path))?
        };
        if identity.parent_id.is_none() && project.ssh_server_id.is_none() {
            return Ok(AgentWorkspace::local(root));
        }
        Ok(AgentWorkspace::confined(
            root,
            WorkspaceMutability::ReadWrite,
        ))
    }
}

fn normalized_remote_path(raw: &str) -> Result<PathBuf> {
    let value = raw.trim().replace('\\', "/");
    if value.is_empty() || value == "/" || value.split('/').any(|part| part == "..") {
        bail!("invalid remote project workspace: {raw}");
    }
    Ok(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_workspace_rejects_parent_traversal() {
        assert!(normalized_remote_path("/workspace/../secret").is_err());
        assert_eq!(
            normalized_remote_path("/workspace/project").unwrap(),
            PathBuf::from("/workspace/project")
        );
    }
}
