//! 进程级 branch lease 的获取、替换与释放。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct BranchKey {
    project_id: String,
}

impl BranchKey {
    pub(super) fn new(project_id: &str) -> Self {
        Self {
            project_id: project_id.to_string(),
        }
    }
}

pub(super) fn process_leases() -> &'static Mutex<HashMap<BranchKey, String>> {
    static LEASES: OnceLock<Mutex<HashMap<BranchKey, String>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn acquire_process_lease(key: &BranchKey, owner: &str) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = leases.get(key) {
        bail!("project is already owned by task {existing}");
    }
    leases.insert(key.clone(), owner.to_string());
    Ok(())
}

pub(super) fn replace_process_lease_owner(
    key: &BranchKey,
    current: &str,
    next: &str,
) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let owner = leases
        .get_mut(key)
        .context("process project lease disappeared")?;
    if owner != current {
        bail!("process project lease owner changed unexpectedly");
    }
    *owner = next.to_string();
    Ok(())
}

pub(super) fn release_process_lease(key: &BranchKey, owner: &str) {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if leases.get(key).is_some_and(|current| current == owner) {
        leases.remove(key);
    }
}
