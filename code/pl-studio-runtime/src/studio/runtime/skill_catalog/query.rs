//! Project Skills catalog 的只读查询：缓存读取、检索与共享 registry 组合。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use pl_core::config::SkillsConfig;
use pl_core::skill::{
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
        remote_backend: Arc<pl_core::remote::RemoteWorkspaceFileBackend>,
    ) -> Result<(SkillRegistry, Vec<Arc<SkillProviderRegistration>>)> {
        remote::remote_workspace_registry(config, system_skills_dir, remote_backend)
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::ObservedResourceKind;

    use super::*;

    #[tokio::test]
    async fn unknown_project_reads_as_uninitialized_without_discovery() {
        let runtime = SkillCatalogRuntime::default();

        let snapshot = runtime.read("project").await;

        assert_eq!(snapshot.state.kind(), ObservedResourceKind::Uninitialized);
        assert_eq!(runtime.read("project").await, snapshot);
    }

    #[tokio::test]
    async fn search_uses_cached_full_catalog_without_discovery_or_revision_change() {
        let root = tempfile::tempdir().unwrap();
        let release_dir = root.path().join(".agents/skills/release-build-triage");
        let slide_dir = root.path().join(".agents/skills/slide-deck-authoring");
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
