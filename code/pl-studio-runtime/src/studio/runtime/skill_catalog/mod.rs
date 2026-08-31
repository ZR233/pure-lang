use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::studio::ids::unix_seconds;
use anyhow::{Context, Error, Result};
use pl_core::config::SkillsConfig;
use pl_core::skill::{
    FileSystemSkillProvider, FrozenSkillCatalog, SkillCatalog, SkillProviderRegistration,
    SkillProviderRequest, SkillRegistry, SkillSelectionRequest, SkillSelector, SkillSummary,
};
use pl_protocol::{
    ObservedResource, ObservedResourceCommand, ObservedResourceKind, StateError, StateOperation,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

mod remote;
mod system;

/// 一次显式发现的 provider 来源。
enum DiscoverySource {
    Local,
    Remote(Arc<pl_core::remote::RemoteWorkspaceFileBackend>),
}

/// Project skills catalog 的唯一内存 owner。
#[derive(Clone)]
pub struct SkillCatalogRuntime {
    command_lock: Arc<Mutex<()>>,
    states: Arc<RwLock<BTreeMap<String, SkillsStateSnapshot>>>,
    events: Option<crate::ProductEventBus>,
    system_skills_dir: Option<Arc<PathBuf>>,
    registry: SkillRegistry,
    _provider_registrations: Vec<Arc<SkillProviderRegistration>>,
}

/// 某 Project 已发布且可被未来 Turn 冻结的 catalog。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsStateSnapshot {
    pub project_id: String,
    pub state: ObservedResource<SkillsStateData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsStateData {
    pub config_fingerprint: String,
    pub catalog_revision: u64,
    pub catalog: Arc<FrozenSkillCatalog>,
}

/// Cached Studio Skill search result over one published catalog revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchResult {
    pub project_id: String,
    pub catalog_revision: u64,
    pub matches: Vec<SkillSummary>,
    pub truncated: bool,
}

impl SkillsStateSnapshot {
    pub(in crate::studio) fn catalog_for_turn(&self) -> Option<Arc<FrozenSkillCatalog>> {
        self.state.value().map(|data| data.catalog.clone())
    }
}

impl SkillCatalogRuntime {
    /// Creates the Studio catalog owner with its product-owned system Skills directory.
    pub fn new(events: crate::ProductEventBus, system_skills_dir: PathBuf) -> Self {
        let (registry, provider_registrations) = local_registry(Some(&system_skills_dir));
        Self {
            command_lock: Arc::new(Mutex::new(())),
            states: Arc::new(RwLock::new(BTreeMap::new())),
            events: Some(events),
            system_skills_dir: Some(Arc::new(system_skills_dir)),
            registry,
            _provider_registrations: provider_registrations,
        }
    }

    pub(super) async fn refresh_system_skills(&self, config: &SkillsConfig) -> Result<()> {
        let system_skills_dir = self
            .system_skills_dir
            .as_ref()
            .context("system Skills directory is not configured")?
            .as_ref()
            .clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            system::refresh_system_skills(&system_skills_dir, &config)
        })
        .await
        .map_err(|error| anyhow::anyhow!("system Skills refresh task failed: {error}"))?
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

    /// Searches the last published catalog without performing discovery.
    pub async fn search(
        &self,
        project_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<SkillSearchResult> {
        anyhow::ensure!(
            !query.trim().is_empty(),
            "skill search query must not be empty"
        );
        anyhow::ensure!(
            (1..=50).contains(&limit),
            "skill search limit must be between 1 and 50"
        );
        let snapshot = self.read(project_id).await;
        let data = snapshot
            .state
            .value()
            .context("skills catalog is not initialized for the selected project")?;
        let selection = SkillSelector.select(
            &data.catalog.snapshot().skills,
            SkillSelectionRequest {
                query,
                limit,
                category: None,
                excluded_names: &[],
                model_invocable_only: false,
            },
        );
        let truncated = selection.truncated();
        Ok(SkillSearchResult {
            project_id: project_id.to_string(),
            catalog_revision: data.catalog_revision,
            matches: selection
                .matches
                .into_iter()
                .cloned()
                .map(Into::into)
                .collect(),
            truncated,
        })
    }

    pub(in crate::studio) fn system_skills_dir(&self) -> Option<PathBuf> {
        self.system_skills_dir
            .as_ref()
            .map(|path| path.as_ref().clone())
    }

    /// 组合远端 workspace 与本地只读目录的 Skill registry。
    ///
    /// Turn 执行与 Settings 显式发现共用这一组合，保证两边看到同一份
    /// 远端 Project、本地 user/system 与内置 Mode Skill 目录。
    pub(in crate::studio) fn remote_workspace_registry(
        &self,
        config: &SkillsConfig,
        system_skills_dir: Option<&Path>,
        remote_backend: Arc<pl_core::remote::RemoteWorkspaceFileBackend>,
    ) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
        remote::remote_workspace_registry(config, system_skills_dir, remote_backend)
    }

    /// 显式扫描并原子发布 Project catalog。
    pub async fn discover(
        &self,
        project_id: &str,
        workspace_root: &Path,
        config: &SkillsConfig,
    ) -> Result<SkillsStateSnapshot> {
        self.discover_with_cancellation(
            project_id,
            workspace_root,
            config,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    }

    /// 对一个远端 workspace 执行显式扫描并发布 Project catalog。
    ///
    /// 远端 provider 贡献 Project 源；本地 user/system 目录与内置 Mode Skill 与
    /// Turn 使用同一套组合，保证设置页与 Turn 看到一致的技能目录。
    pub async fn discover_remote(
        &self,
        project_id: &str,
        workspace_root: &Path,
        config: &SkillsConfig,
        cancellation: tokio_util::sync::CancellationToken,
        remote_backend: Arc<pl_core::remote::RemoteWorkspaceFileBackend>,
    ) -> Result<SkillsStateSnapshot> {
        self.discover_from_source(
            project_id,
            workspace_root,
            config,
            cancellation,
            DiscoverySource::Remote(remote_backend),
        )
        .await
    }

    pub async fn discover_with_cancellation(
        &self,
        project_id: &str,
        workspace_root: &Path,
        config: &SkillsConfig,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<SkillsStateSnapshot> {
        self.discover_from_source(
            project_id,
            workspace_root,
            config,
            cancellation,
            DiscoverySource::Local,
        )
        .await
    }

    async fn discover_from_source(
        &self,
        project_id: &str,
        workspace_root: &Path,
        config: &SkillsConfig,
        cancellation: tokio_util::sync::CancellationToken,
        source: DiscoverySource,
    ) -> Result<SkillsStateSnapshot> {
        let _command = self.command_lock.lock().await;
        let fingerprint = skills_fingerprint(config)?;
        let previous = self.read(project_id).await;
        let revision = previous.state.revision();
        let operation_id = format!("skills-discover-{}", revision.saturating_add(1));
        let running = SkillsStateSnapshot {
            project_id: project_id.to_string(),
            state: previous
                .state
                .decide(ObservedResourceCommand::Begin {
                    expected_revision: revision,
                    operation: StateOperation::Discover,
                    operation_id,
                    started_at: unix_seconds(),
                })
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .next_state,
        };
        self.publish(project_id, running.clone()).await;

        let workspace_root = workspace_root.to_path_buf();
        let config = config.clone();
        let discovered = match source {
            DiscoverySource::Local => {
                let system_skills_dir = self
                    .system_skills_dir
                    .as_ref()
                    .map(|path| path.as_ref().clone());
                self.registry
                    .discover(SkillProviderRequest {
                        workspace_root,
                        config,
                        system_dir: system_skills_dir,
                        cancellation,
                    })
                    .await
                    .map_err(Error::from)
            }
            DiscoverySource::Remote(remote_backend) => {
                // 系统目录通过 explicit provider 注册，request 不再携带 system_dir。
                let system_skills_dir = self.system_skills_dir();
                let (registry, registrations) = remote::remote_workspace_registry(
                    &config,
                    system_skills_dir.as_deref(),
                    remote_backend,
                )?;
                let discovered = registry
                    .discover(SkillProviderRequest {
                        workspace_root,
                        config,
                        system_dir: None,
                        cancellation,
                    })
                    .await
                    .map_err(Error::from);
                drop(registrations);
                discovered
            }
        };
        let catalog = match discovered {
            Ok(catalog) => catalog,
            Err(error) => {
                let checked_at = unix_seconds();
                let failed = SkillsStateSnapshot {
                    project_id: project_id.to_string(),
                    state: running
                        .state
                        .decide(ObservedResourceCommand::Fail {
                            expected_revision: running.state.revision(),
                            failed_at: checked_at,
                            error: StateError {
                                code: "skillsDiscoveryFailed".to_string(),
                                message: format!("{error:#}"),
                                retryable: true,
                            },
                        })
                        .map_err(|transition| anyhow::anyhow!(transition.to_string()))?
                        .next_state,
                };
                self.publish(project_id, failed).await;
                return Err(error);
            }
        };
        let checked_at = unix_seconds();
        if !catalog.snapshot().complete {
            let failed = SkillsStateSnapshot {
                project_id: project_id.to_string(),
                state: running
                    .state
                    .decide(ObservedResourceCommand::Fail {
                        expected_revision: running.state.revision(),
                        failed_at: checked_at,
                        error: StateError {
                            code: "skillsDiscoveryIncomplete".to_string(),
                            message: catalog.snapshot().warnings.join("; "),
                            retryable: true,
                        },
                    })
                    .map_err(|transition| anyhow::anyhow!(transition.to_string()))?
                    .next_state,
            };
            self.publish(project_id, failed.clone()).await;
            return Ok(failed);
        }
        let catalog_revision = previous.state.value().map_or(1, |data| {
            if catalog_content_eq(data.catalog.snapshot(), catalog.snapshot()) {
                data.catalog_revision
            } else {
                data.catalog_revision.saturating_add(1)
            }
        });
        let snapshot = SkillsStateSnapshot {
            project_id: project_id.to_string(),
            state: running
                .state
                .decide(ObservedResourceCommand::Succeed {
                    expected_revision: running.state.revision(),
                    updated_at: checked_at,
                    last_checked_at: Some(checked_at),
                    value: SkillsStateData {
                        config_fingerprint: fingerprint,
                        catalog_revision,
                        catalog: Arc::new(catalog),
                    },
                })
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .next_state,
        };
        self.publish(project_id, snapshot.clone()).await;
        Ok(snapshot)
    }

    /// Settings 变化只标记旧 payload stale，不执行扫描。
    pub async fn mark_all_stale(&self) {
        let _command = self.command_lock.lock().await;
        let mut states = self.states.write().await;
        let mut changed = Vec::new();
        for state in states.values_mut() {
            if state.state.kind() == ObservedResourceKind::Ready {
                let Some(value) = state.state.value().cloned() else {
                    continue;
                };
                let decision = state.state.decide(ObservedResourceCommand::MarkStale {
                    expected_revision: state.state.revision(),
                    stale_at: unix_seconds(),
                    value,
                });
                if let Ok(decision) = decision {
                    state.state = decision.next_state;
                    changed.push(state.clone());
                }
            }
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
        let (registry, provider_registrations) = local_registry(None);
        Self {
            command_lock: Arc::new(Mutex::new(())),
            states: Arc::new(RwLock::new(BTreeMap::new())),
            events: None,
            system_skills_dir: None,
            registry,
            _provider_registrations: provider_registrations,
        }
    }
}

fn empty_snapshot(project_id: &str) -> SkillsStateSnapshot {
    SkillsStateSnapshot {
        project_id: project_id.to_string(),
        state: ObservedResource::uninitialized(unix_seconds()),
    }
}

fn local_registry(
    system_skills_dir: Option<&Path>,
) -> (SkillRegistry, Vec<Arc<SkillProviderRegistration>>) {
    let registry = SkillRegistry::new();
    let mut registrations = Vec::new();
    if let Some(system_skills_dir) = system_skills_dir {
        let provider = FileSystemSkillProvider::from_directories(
            pl_core::skill::BUILTIN_MODE_PROVIDER_ID,
            vec![pl_core::skill::SkillDirectorySource::new(
                // The materialized system directory is flattened so the stable
                // `mode.*` directory sits beside the other bundled Skills.
                system_skills_dir,
                pl_core::skill::SkillSourceKind::System,
            )],
        );
        match provider.and_then(|provider| registry.register(Arc::new(provider))) {
            Ok(registration) => registrations.push(Arc::new(registration)),
            Err(error) => {
                tracing::error!(%error, "failed to register built-in Mode Skill provider")
            }
        }
    }
    if let Ok(registration) = registry
        .register(Arc::new(FileSystemSkillProvider::new()))
        .map(Arc::new)
        .map_err(|error| tracing::error!(%error, "failed to register filesystem Skill provider"))
    {
        registrations.push(registration);
    }
    (registry, registrations)
}

fn catalog_content_eq(left: &SkillCatalog, right: &SkillCatalog) -> bool {
    left.project_dir == right.project_dir
        && left.skills == right.skills
        && left.modes == right.modes
}

pub(super) fn skills_fingerprint(config: &SkillsConfig) -> Result<String> {
    let serialized = serde_json::to_vec(config)?;
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
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
        assert_eq!(first.state.kind(), ObservedResourceKind::Uninitialized);
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
        assert_eq!(failed.state.revision(), previous.state.revision() + 2);
        assert_eq!(failed.state.kind(), ObservedResourceKind::Failed);
        assert_eq!(failed.state.value(), None);
    }

    #[tokio::test]
    async fn every_discovery_rescans_but_unchanged_content_keeps_catalog_revision() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("skills/demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: First\n---\nFirst body\n",
        )
        .unwrap();
        let runtime = SkillCatalogRuntime::default();
        let config = SkillsConfig::default();

        let first = runtime
            .discover("project", root.path(), &config)
            .await
            .unwrap();
        let second = runtime
            .discover("project", root.path(), &config)
            .await
            .unwrap();
        assert_eq!(
            first.state.value().unwrap().catalog_revision,
            second.state.value().unwrap().catalog_revision
        );

        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Second\n---\nSecond body\n",
        )
        .unwrap();
        let third = runtime
            .discover("project", root.path(), &config)
            .await
            .unwrap();
        assert_eq!(
            third.state.value().unwrap().catalog_revision,
            second.state.value().unwrap().catalog_revision + 1
        );
        assert_eq!(
            third
                .state
                .value()
                .unwrap()
                .catalog
                .snapshot()
                .find("demo")
                .expect("project Skill remains discoverable")
                .description,
            "Second"
        );
    }

    #[tokio::test]
    async fn globally_disabled_skills_are_still_discovered_for_settings() {
        let root = tempfile::tempdir().unwrap();
        let skill_dir = root.path().join("skills/demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\nBody\n",
        )
        .unwrap();
        let runtime = SkillCatalogRuntime::default();
        let config = SkillsConfig {
            enabled: false,
            ..SkillsConfig::default()
        };

        let snapshot = runtime
            .discover("project", root.path(), &config)
            .await
            .unwrap();

        let skill = snapshot
            .state
            .value()
            .unwrap()
            .catalog
            .snapshot()
            .find("demo")
            .expect("global disable must not hide the project Skill from settings");
        assert_eq!(skill.source, pl_core::skill::SkillSourceKind::Project);
    }

    #[tokio::test]
    async fn search_uses_cached_full_catalog_without_discovery_or_revision_change() {
        let root = tempfile::tempdir().unwrap();
        let release_dir = root.path().join("skills/release-build-triage");
        let slide_dir = root.path().join("skills/slide-deck-authoring");
        std::fs::create_dir_all(&release_dir).unwrap();
        std::fs::create_dir_all(&slide_dir).unwrap();
        std::fs::write(
            release_dir.join("SKILL.md"),
            "---\nname: release-build-triage\ndescription: Diagnose Rust release linker failures\n---\nBody\n",
        )
        .unwrap();
        std::fs::write(
            slide_dir.join("SKILL.md"),
            "---\nname: slide-deck-authoring\ndescription: Create presentations and speaker notes\ndisable-model-invocation: true\n---\nBody\n",
        )
        .unwrap();
        let runtime = SkillCatalogRuntime::default();
        let published = runtime
            .discover("project", root.path(), &SkillsConfig::default())
            .await
            .unwrap();
        let published_revision = published.state.revision();
        let catalog_revision = published.state.value().unwrap().catalog_revision;

        std::fs::write(
            release_dir.join("SKILL.md"),
            "---\nname: release-build-triage\ndescription: changed on disk\n---\nBody\n",
        )
        .unwrap();
        let release = runtime
            .search("project", "Rust release linker", 10)
            .await
            .unwrap();
        let slide = runtime
            .search("project", "presentation speaker notes", 10)
            .await
            .unwrap();
        let after = runtime.read("project").await;

        assert_eq!(release.catalog_revision, catalog_revision);
        assert_eq!(release.matches[0].name, "release-build-triage");
        assert!(release.matches[0].description.contains("Diagnose Rust"));
        let slide = slide
            .matches
            .iter()
            .find(|skill| skill.name == "slide-deck-authoring")
            .expect("description search must return the model-disabled project Skill");
        assert!(!slide.invocation.model_invocable);
        assert_eq!(after.state.revision(), published_revision);
        assert_eq!(
            after.state.value().unwrap().catalog_revision,
            catalog_revision
        );
    }
}
