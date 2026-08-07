use super::*;

pub(super) struct WorktreeRecoveryPreflight {
    pub(super) groups: HashMap<String, WorktreeRecoveryGroup>,
    pub(super) run_groups: HashMap<String, String>,
    pub(super) failures: Vec<WorktreeRecoveryPreflightFailure>,
}

pub(super) struct WorktreeRecoveryPreflightFailure {
    pub(super) runs: Vec<TaskRunRecord>,
    pub(super) message: String,
}

pub(super) async fn resolve_worktree_recovery_groups(
    owners: Vec<TaskWorktreeOwnerSnapshot>,
) -> WorktreeRecoveryPreflight {
    let mut owners_by_group = HashMap::<String, Vec<TaskWorktreeOwnerSnapshot>>::new();
    let mut run_groups = HashMap::new();
    for owner in owners {
        let group_key = canonical_path_key(Path::new(&owner.run.git_common_dir));
        run_groups.insert(owner.run.id.clone(), group_key.clone());
        owners_by_group.entry(group_key).or_default().push(owner);
    }

    let mut groups = HashMap::new();
    let mut failures = Vec::new();
    for (group_key, group_owners) in owners_by_group {
        let mut repositories = Vec::<PathBuf>::new();
        let mut inspected_workspaces = HashSet::new();
        let mut failure = None;
        for owner in &group_owners {
            let workspace = workspace_key(&owner.run.workspace_root);
            if !inspected_workspaces.insert(workspace) {
                continue;
            }
            let snapshot = match inspect_repository(&owner.run.workspace_root, false).await {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    failure = Some(format!(
                        "failed to resolve Git common directory for known task workspace {}: {error}",
                        owner.run.workspace_root
                    ));
                    break;
                }
            };
            if canonical_path_key(&snapshot.git_common_dir) != group_key {
                failure = Some(format!(
                    "known task workspace {} no longer belongs to its durable Git common directory",
                    owner.run.workspace_root
                ));
                break;
            }
            let repository_key = canonical_path_key(&snapshot.workspace_root);
            if !repositories
                .iter()
                .any(|repository| canonical_path_key(repository) == repository_key)
            {
                repositories.push(snapshot.workspace_root);
            }
        }
        if let Some(message) = failure {
            failures.push(WorktreeRecoveryPreflightFailure {
                runs: group_owners.iter().map(|owner| owner.run.clone()).collect(),
                message,
            });
            continue;
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
