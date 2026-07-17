use super::*;

pub(super) async fn resolve_worktree_recovery_groups(
    owners: Vec<TaskWorktreeOwnerSnapshot>,
) -> Result<(
    HashMap<String, WorktreeRecoveryGroup>,
    HashMap<String, String>,
)> {
    let mut repositories = HashMap::new();
    for owner in &owners {
        let workspace = workspace_key(&owner.run.workspace_root);
        if repositories.contains_key(&workspace) {
            continue;
        }
        let snapshot = inspect_repository(&owner.run.workspace_root, false)
            .await
            .with_context(|| {
                format!(
                    "failed to resolve Git common directory for known task workspace {}",
                    owner.run.workspace_root
                )
            })?;
        repositories.insert(workspace, snapshot);
    }

    let mut groups = HashMap::<String, WorktreeRecoveryGroup>::new();
    let mut run_groups = HashMap::new();
    for owner in owners {
        let workspace = workspace_key(&owner.run.workspace_root);
        let snapshot = repositories
            .get(&workspace)
            .context("known task workspace inspection disappeared")?;
        let group_key = canonical_path_key(&snapshot.git_common_dir);
        let group = groups
            .entry(group_key.clone())
            .or_insert_with(|| WorktreeRecoveryGroup {
                repositories: Vec::new(),
                owners: Vec::new(),
            });
        let repository_key = canonical_path_key(&snapshot.workspace_root);
        if !group
            .repositories
            .iter()
            .any(|repository| canonical_path_key(repository) == repository_key)
        {
            group.repositories.push(snapshot.workspace_root.clone());
        }
        run_groups.insert(owner.run.id.clone(), group_key);
        group.owners.push(owner);
    }
    Ok((groups, run_groups))
}

fn workspace_key(workspace: &str) -> String {
    let workspace = workspace.replace('\\', "/");
    if cfg!(windows) {
        workspace.to_lowercase()
    } else {
        workspace
    }
}

fn canonical_path_key(path: &Path) -> String {
    workspace_key(&path.to_string_lossy())
}
