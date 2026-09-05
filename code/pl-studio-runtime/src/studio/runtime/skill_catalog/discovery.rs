//! Project Skills catalog 的显式发现：扫描 provider、原子发布并维护 revision 与指纹。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use crate::studio::ids::unix_seconds;
use anyhow::{Error, Result};
use pl_core::config::SkillsConfig;
use pl_core::skill::{SkillCatalog, SkillProviderRequest};
use pl_protocol::{ObservedResourceCommand, ObservedResourceKind, StateError, StateOperation};

use super::remote;
use super::{SkillCatalogRuntime, SkillsStateData, SkillsStateSnapshot};

/// 一次显式发现的 provider 来源。
enum DiscoverySource {
    Local,
    Remote(Arc<pl_core::remote::RemoteWorkspaceFileBackend>),
}

impl SkillCatalogRuntime {
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
    /// 远端 provider 贡献 Project 源；本地 user/system 目录与
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

fn catalog_content_eq(left: &SkillCatalog, right: &SkillCatalog) -> bool {
    left.project_dir == right.project_dir && left.skills == right.skills
}

pub(in crate::studio::runtime) fn skills_fingerprint(config: &SkillsConfig) -> Result<String> {
    let serialized = serde_json::to_vec(config)?;
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let skill_dir = root.path().join(".agents/skills/demo");
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
        let skill_dir = root.path().join(".agents/skills/demo");
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
}
