//! Project Skills catalog 的核心类型：运行时 owner、状态快照与本地 registry 构造。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::studio::ids::unix_seconds;
use anyhow::{Context, Result};
use pl_core::config::SkillsConfig;
use pl_core::skill::{
    FileSystemSkillProvider, FrozenSkillCatalog, SkillProviderRegistration, SkillRegistry,
    SkillSummary,
};
use pl_protocol::ObservedResource;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use super::system;

/// Project skills catalog 的唯一内存 owner。
#[derive(Clone)]
pub struct SkillCatalogRuntime {
    pub(super) command_lock: Arc<Mutex<()>>,
    pub(super) states: Arc<RwLock<BTreeMap<String, SkillsStateSnapshot>>>,
    pub(super) events: Option<crate::ProductEventBus>,
    pub(super) system_skills_dir: Option<Arc<PathBuf>>,
    pub(super) registry: SkillRegistry,
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

    pub(in crate::studio::runtime) async fn refresh_system_skills(
        &self,
        config: &SkillsConfig,
    ) -> Result<()> {
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

pub(super) fn empty_snapshot(project_id: &str) -> SkillsStateSnapshot {
    SkillsStateSnapshot {
        project_id: project_id.to_string(),
        state: ObservedResource::uninitialized(unix_seconds()),
    }
}

fn local_registry(
    _system_skills_dir: Option<&Path>,
) -> (SkillRegistry, Vec<Arc<SkillProviderRegistration>>) {
    let registry = SkillRegistry::new();
    let mut registrations = Vec::new();
    if let Ok(registration) = registry
        .register(Arc::new(FileSystemSkillProvider::new()))
        .map(Arc::new)
        .map_err(|error| tracing::error!(%error, "failed to register filesystem Skill provider"))
    {
        registrations.push(registration);
    }
    (registry, registrations)
}
