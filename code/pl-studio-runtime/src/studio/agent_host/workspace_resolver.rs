use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use pl_core::{AgentIdentity, AgentWorkspace, resolve_workspace_root};
use pl_protocol::{AgentWorkspaceAssignmentSnapshot, AgentWorkspaceMode};

use crate::studio::records::{ProjectRecord, ThreadRecord};

/// 从 child canonical session 中消费 spawn 时冻结的 workspace assignment。
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
        assignment: Option<&AgentWorkspaceAssignmentSnapshot>,
    ) -> Result<AgentWorkspace> {
        let root = if project.ssh_server_id.is_some() {
            normalized_remote_path(&project.path)?
        } else {
            resolve_workspace_root(&PathBuf::from(&project.path))?
        };
        if identity.parent_id.is_none() {
            return Ok(AgentWorkspace::local(root));
        }
        let assignment = assignment
            .ok_or_else(|| anyhow::anyhow!("child Agent has no frozen workspace assignment"))?;
        if Path::new(&assignment.project_root) != root {
            bail!("child Agent workspace assignment does not match its Studio project");
        }
        let assigned_root = PathBuf::from(&assignment.root);
        match assignment.mode {
            AgentWorkspaceMode::Unrestricted => {
                if assigned_root != root {
                    bail!("unrestricted Agent root must equal the Studio project root");
                }
                Ok(AgentWorkspace::local(root))
            }
            AgentWorkspaceMode::Directory => {
                if assigned_root != root || assignment.worktree.is_some() {
                    bail!("directory Agent has an invalid frozen workspace assignment");
                }
                Ok(AgentWorkspace::directory(
                    root,
                    assignment
                        .writable_paths
                        .as_ref()
                        .map(|paths| paths.iter().map(PathBuf::from).collect::<Vec<_>>()),
                ))
            }
            AgentWorkspaceMode::Worktree => {
                let worktree = assignment.worktree.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("worktree Agent assignment has no worktree receipt")
                })?;
                if Path::new(&worktree.path) != assigned_root {
                    bail!("worktree Agent root does not match its receipt");
                }
                Ok(AgentWorkspace::worktree(root, assigned_root))
            }
        }
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
