use std::path::Path;

use anyhow::{Context, Result};

use crate::resolve_workspace_root;

use super::super::SkillsStateSnapshot;
use super::super::StudioRuntime;
use super::super::lsp_state::health;

impl StudioRuntime {
    pub async fn activate_project(&self, project_id: &str) -> Result<()> {
        if self.recovery_issues().iter().any(|issue| {
            issue.scope == crate::StudioRecoveryIssueScope::Project
                && issue.project_id.as_deref() == Some(project_id)
        }) {
            anyhow::bail!("selected project is blocked by a recovery issue");
        }
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let settings = self.config_runtime.read()?;
        // 自定义 LSP server 与 catalog 冲突在配置加载时已 fail-loud；此处再合并一次，
        // 保证 registry catalog 与当前配置一致，并让 fingerprint 覆盖检测结果。
        self.external_runtimes
            .lsp
            .apply_user_servers(&settings.config.lsp.servers)
            .await
            .map_err(|error| anyhow::anyhow!("invalid [lsp.servers] configuration: {error}"))?;
        let fingerprint = format!(
            "{}:{}:{}",
            workspace_root.display(),
            self.external_runtimes
                .lsp
                .membership_fingerprint(&workspace_root)
                .await,
            super::super::skill_catalog::skills_fingerprint(&settings.config.skills)?,
        );
        let _activation_command = self.activation.command_lock.lock().await;
        if self
            .activation
            .applied
            .read()
            .await
            .as_ref()
            .is_some_and(|applied| {
                applied.project_id == project_id && applied.fingerprint == fingerprint
            })
        {
            return Ok(());
        }
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Activate)
            .await?;
        self.external_runtimes
            .lsp
            .reconcile_workspace_membership(&workspace_root)
            .await;
        self.external_runtimes
            .lsp
            .probe_lsp_server(&workspace_root)
            .await;
        let health = health(&self.external_runtimes.lsp).await;
        self.external_runtimes.lsp_state.ready(health, true).await?;
        let _ = self
            .skills
            .discover(project_id, &workspace_root, &settings.config.skills)
            .await?;
        *self.activation.applied.write().await = Some(super::super::ProjectActivation {
            project_id: project_id.to_string(),
            fingerprint,
        });
        Ok(())
    }

    pub async fn discover_skills(&self, project_id: &str) -> Result<SkillsStateSnapshot> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        let settings = self.config_runtime.read()?;
        self.skills
            .discover(project_id, &workspace_root, &settings.config.skills)
            .await
    }

    pub async fn probe_lsp_server(&self, project_id: &str) -> Result<()> {
        let workspace_root = self.project_workspace_root(project_id).await?;
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Probe)
            .await?;
        self.external_runtimes
            .lsp
            .probe_lsp_server(workspace_root)
            .await;
        let health = health(&self.external_runtimes.lsp).await;
        self.external_runtimes.lsp_state.ready(health, true).await?;
        Ok(())
    }

    pub async fn repair_lsp_server(&self, project_id: &str, server_id: &str) -> Result<()> {
        let workspace_root = self.project_workspace_root(project_id).await?;
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Repair)
            .await?;
        let result = self
            .external_runtimes
            .lsp
            .repair_lsp_server(workspace_root, server_id)
            .await
            .map_err(anyhow::Error::from);
        match result {
            Ok(()) => {
                let health = health(&self.external_runtimes.lsp).await;
                self.external_runtimes.lsp_state.ready(health, true).await?;
                Ok(())
            }
            Err(error) => {
                self.external_runtimes.lsp_state.failed(&error).await?;
                Err(error)
            }
        }
    }

    pub async fn reset_lsp(&self, scope: pl_lsp::LspScope) -> Result<()> {
        let _command = self.external_runtimes.lsp_state.command().await;
        self.external_runtimes
            .lsp_state
            .begin(pl_protocol::StateOperation::Reset)
            .await?;
        let result = self
            .external_runtimes
            .lsp
            .reset_lsp(scope)
            .await
            .map_err(anyhow::Error::from);
        match result {
            Ok(()) => {
                let health = health(&self.external_runtimes.lsp).await;
                self.external_runtimes
                    .lsp_state
                    .ready(health, false)
                    .await?;
                Ok(())
            }
            Err(error) => {
                self.external_runtimes.lsp_state.failed(&error).await?;
                Err(error)
            }
        }
    }

    /// Resolves transport-neutral Project identities into an internal LSP reset scope.
    pub async fn reset_lsp_request(
        &self,
        request: pl_protocol::studio::LspResetRequest,
    ) -> Result<()> {
        let scope = match request {
            pl_protocol::studio::LspResetRequest::Server {
                project_id,
                server_id,
            } => pl_lsp::LspScope::Server {
                workspace_root: self.project_workspace_root(&project_id).await?,
                server_id,
            },
            pl_protocol::studio::LspResetRequest::Workspace { project_id } => {
                pl_lsp::LspScope::Workspace {
                    workspace_root: self.project_workspace_root(&project_id).await?,
                }
            }
            pl_protocol::studio::LspResetRequest::All => pl_lsp::LspScope::All,
        };
        self.reset_lsp(scope).await
    }

    async fn project_workspace_root(&self, project_id: &str) -> Result<std::path::PathBuf> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        Ok(resolve_workspace_root(Path::new(&project.path))?)
    }
}
