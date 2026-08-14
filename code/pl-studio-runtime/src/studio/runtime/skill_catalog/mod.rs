use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Error, Result};
use pl_core::config::SkillsConfig;
use pl_core::skill::SkillCatalog;
use pl_protocol::{ObservedStateMeta, ObservedStatePhase, StateOperation};
use tokio::sync::{Mutex, RwLock};

/// Project skills catalog 的唯一内存 owner。
#[derive(Clone)]
pub struct SkillCatalogRuntime {
    command_lock: Arc<Mutex<()>>,
    states: Arc<RwLock<BTreeMap<String, SkillsStateSnapshot>>>,
    events: Option<crate::StudioProductEventRuntime>,
}

/// 某 Project 已发布且可被未来 Turn 冻结的 catalog。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsStateSnapshot {
    pub meta: ObservedStateMeta,
    pub project_id: String,
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: Arc<SkillCatalog>,
}

impl SkillCatalogRuntime {
    pub fn new(events: crate::StudioProductEventRuntime) -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            states: Arc::new(RwLock::new(BTreeMap::new())),
            events: Some(events),
        }
    }

    /// 只读缓存；不存在时返回 authoritative empty，不访问文件系统。
    pub async fn read(&self, project_id: &str) -> SkillsStateSnapshot {
        self.states
            .read()
            .await
            .get(project_id)
            .cloned()
            .unwrap_or_else(|| empty_snapshot(project_id))
    }

    /// 显式扫描并原子发布 Project catalog。
    pub async fn discover(
        &self,
        project_id: &str,
        workspace_root: &Path,
        config: &SkillsConfig,
    ) -> Result<SkillsStateSnapshot> {
        let _command = self.command_lock.lock().await;
        let fingerprint = skills_fingerprint(config)?;
        let previous = self.read(project_id).await;
        let operation_id = format!(
            "skills-discover-{}",
            previous.meta.revision.saturating_add(1)
        );
        let running = SkillsStateSnapshot {
            meta: ObservedStateMeta {
                revision: previous.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Running {
                    operation: StateOperation::Discover,
                    operation_id,
                },
                updated_at: unix_seconds(),
                last_checked_at: previous.meta.last_checked_at,
                stale: previous.meta.stale,
            },
            ..previous.clone()
        };
        self.publish(project_id, running.clone()).await;

        let workspace_root = workspace_root.to_path_buf();
        let config = config.clone();
        let discovered =
            tokio::task::spawn_blocking(move || SkillCatalog::discover(&workspace_root, &config))
                .await
                .map_err(|error| anyhow::anyhow!("Skills discovery task failed: {error}"))
                .and_then(|catalog| catalog.map_err(Error::from));
        let catalog = match discovered {
            Ok(catalog) => catalog,
            Err(error) => {
                let checked_at = unix_seconds();
                let failed = SkillsStateSnapshot {
                    meta: ObservedStateMeta {
                        revision: running.meta.revision.saturating_add(1),
                        phase: ObservedStatePhase::Failed {
                            operation: StateOperation::Discover,
                            error: pl_protocol::StateError {
                                code: "skillsDiscoveryFailed".to_string(),
                                message: format!("{error:#}"),
                                retryable: true,
                            },
                        },
                        updated_at: checked_at,
                        last_checked_at: Some(checked_at),
                        stale: true,
                    },
                    ..previous
                };
                self.publish(project_id, failed).await;
                return Err(error);
            }
        };
        let snapshot = SkillsStateSnapshot {
            meta: ObservedStateMeta {
                revision: running.meta.revision.saturating_add(1),
                phase: ObservedStatePhase::Ready,
                updated_at: unix_seconds(),
                last_checked_at: Some(unix_seconds()),
                stale: false,
            },
            project_id: project_id.to_string(),
            config_fingerprint: fingerprint,
            catalog_revision: previous.catalog_revision.saturating_add(1),
            catalog: Arc::new(catalog),
        };
        self.publish(project_id, snapshot.clone()).await;
        Ok(snapshot)
    }

    /// Settings 变化只标记旧 payload stale，不执行扫描。
    pub async fn mark_all_stale(&self) {
        let mut states = self.states.write().await;
        let mut changed = Vec::new();
        for state in states.values_mut() {
            state.meta.revision = state.meta.revision.saturating_add(1);
            state.meta.updated_at = unix_seconds();
            state.meta.stale = true;
            changed.push(state.clone());
        }
        drop(states);
        if let Some(events) = &self.events {
            for state in changed {
                events.emit_skills_state(state);
            }
        }
    }

    async fn publish(&self, project_id: &str, snapshot: SkillsStateSnapshot) {
        self.states
            .write()
            .await
            .insert(project_id.to_string(), snapshot.clone());
        if let Some(events) = &self.events {
            events.emit_skills_state(snapshot);
        }
    }
}

impl Default for SkillCatalogRuntime {
    fn default() -> Self {
        Self {
            command_lock: Arc::new(Mutex::new(())),
            states: Arc::new(RwLock::new(BTreeMap::new())),
            events: None,
        }
    }
}

fn empty_snapshot(project_id: &str) -> SkillsStateSnapshot {
    SkillsStateSnapshot {
        meta: ObservedStateMeta::uninitialized(unix_seconds()),
        project_id: project_id.to_string(),
        config_fingerprint: String::new(),
        catalog_revision: 0,
        catalog: Arc::new(SkillCatalog {
            project_dir: PathBuf::new(),
            skills: Vec::new(),
            warnings: Vec::new(),
        }),
    }
}

pub(super) fn skills_fingerprint(config: &SkillsConfig) -> Result<String> {
    let serialized = serde_json::to_vec(config)?;
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn repeated_reads_do_not_scan_or_change_revision() {
        let runtime = SkillCatalogRuntime::default();

        let first = runtime.read("project").await;
        let second = runtime.read("project").await;

        assert_eq!(first, second);
        assert_eq!(first.catalog_revision, 0);
    }

    #[tokio::test]
    async fn discovery_failure_keeps_last_payload_and_publishes_failed_state() {
        let runtime = SkillCatalogRuntime::default();
        let previous = runtime.read("project").await;
        let config = SkillsConfig {
            project_dir: "../outside".to_string(),
            ..SkillsConfig::default()
        };

        let result = runtime.discover("project", Path::new("."), &config).await;

        assert!(result.is_err());
        let failed = runtime.read("project").await;
        assert_eq!(failed.meta.revision, previous.meta.revision + 2);
        assert!(failed.meta.stale);
        assert!(matches!(
            failed.meta.phase,
            ObservedStatePhase::Failed {
                operation: StateOperation::Discover,
                ..
            }
        ));
        assert_eq!(failed.catalog_revision, previous.catalog_revision);
        assert_eq!(failed.catalog, previous.catalog);
    }
}
