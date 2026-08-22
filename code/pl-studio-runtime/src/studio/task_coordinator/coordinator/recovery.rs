use super::*;

pub(super) struct WorktreeRecoveryPreflight {
    pub(super) groups: HashMap<String, WorktreeRecoveryGroup>,
    pub(super) run_groups: HashMap<String, String>,
    pub(super) failures: Vec<WorktreeRecoveryPreflightFailure>,
}

pub(super) struct WorktreeRecoveryPreflightFailure {
    pub(super) runs: Vec<TaskRun>,
    pub(super) message: String,
}

pub(super) async fn resolve_worktree_recovery_groups(
    owners: Vec<TaskWorktreeOwnerSnapshot>,
) -> WorktreeRecoveryPreflight {
    let mut owners_by_group = HashMap::<String, Vec<TaskWorktreeOwnerSnapshot>>::new();
    let mut run_groups = HashMap::new();
    for owner in owners {
        let group_key = owner.run.project_id.clone();
        run_groups.insert(owner.run.id.clone(), group_key.clone());
        owners_by_group.entry(group_key).or_default().push(owner);
    }

    let mut groups = HashMap::new();
    let failures = Vec::new();
    for (group_key, group_owners) in owners_by_group {
        let mut repositories = Vec::<PathBuf>::new();
        let mut inspected_workspaces = HashSet::new();
        for owner in &group_owners {
            let workspace = workspace_key(&owner.run.workspace_root);
            if !inspected_workspaces.insert(workspace) {
                continue;
            }
            let repository = PathBuf::from(&owner.run.workspace_root);
            let repository_key = canonical_path_key(&repository);
            if !repositories
                .iter()
                .any(|repository| canonical_path_key(repository) == repository_key)
            {
                repositories.push(repository);
            }
        }
        groups.insert(
            group_key,
            WorktreeRecoveryGroup {
                repositories,
                owners: group_owners,
            },
        );
    }
    WorktreeRecoveryPreflight {
        groups,
        run_groups,
        failures,
    }
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
    let path = crate::agent::worktree::git_compatible_path(path.to_path_buf());
    workspace_key(&path.to_string_lossy())
}
