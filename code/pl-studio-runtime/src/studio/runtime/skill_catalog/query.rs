//! Project Skills catalog 的只读查询：缓存读取、检索与共享 registry 组合。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use pl_tool::skill::SkillsConfig;
use pl_tool::skill::{
    SkillProviderRegistration, SkillRegistry, SkillSelectionRequest, SkillSelector,
};

use super::remote;
use super::types::empty_snapshot;
use super::{SkillCatalogRuntime, SkillSearchResult, SkillsStateSnapshot};

impl SkillCatalogRuntime {
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

    /// Freezes one Thread's workspace catalog without publishing a second project refresh event.
    pub(in crate::studio) async fn freeze_thread_catalog(
        &self,
        root: &Path,
        config: &SkillsConfig,
        cancellation: tokio_util::sync::CancellationToken,
        remote: Option<Arc<pl_tool::remote::RemoteWorkspaceFileBackend>>,
    ) -> Result<Arc<pl_tool::skill::FrozenSkillCatalog>> {
        let request = pl_tool::skill::SkillProviderRequest {
            workspace_root: root.to_owned(),
            config: config.clone(),
            system_dir: self.system_skills_dir(),
            cancellation,
        };
        let catalog = if let Some(remote) = remote {
            let (registry, registrations) =
                self.remote_workspace_registry(config, request.system_dir.as_deref(), remote)?;
            let result = registry
                .discover(pl_tool::skill::SkillProviderRequest {
                    system_dir: None,
                    ..request
                })
                .await;
            drop(registrations);
            result?
        } else {
            self.registry.discover(request).await?
        };
        anyhow::ensure!(
            catalog.snapshot().warnings.is_empty(),
            "Skill directory discovery is incomplete: {}",
            catalog.snapshot().warnings.join("; ")
        );
        Ok(Arc::new(catalog))
    }

    pub(in crate::studio) fn system_skills_dir(&self) -> Option<PathBuf> {
        self.system_skills_dir
            .as_ref()
            .map(|path| path.as_ref().clone())
    }

    /// 组合远端 workspace 与本地只读目录的 Skill registry。
    ///
    /// Turn 执行与 Settings 显式发现共用这一组合，保证两边看到同一份
    /// 远端 Project 与本地 user/system 目录。
    pub(in crate::studio) fn remote_workspace_registry(
        &self,
        config: &SkillsConfig,
        system_skills_dir: Option<&Path>,
        remote_backend: Arc<pl_tool::remote::RemoteWorkspaceFileBackend>,
    ) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
        remote::remote_workspace_registry(config, system_skills_dir, remote_backend)
    }
}
