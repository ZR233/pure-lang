use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{ModelRole, ReasoningEffort, RoleConfig};
use crate::skill::SkillCatalog;
use crate::studio::mappers::default_session_runtime_record;
use crate::studio::records::{ProjectRecord, SessionRecord, SessionRuntimeRecord};
use crate::{CompileMode, PureConfig, resolve_workspace_root};

use super::StudioRuntime;

impl StudioRuntime {
    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        self.store.upsert_project(path).await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.store.list_projects().await
    }

    pub async fn ensure_project_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        let mut sessions = self.store.list_sessions(project_id).await?;
        if sessions.is_empty() {
            sessions.push(
                self.store
                    .create_session(project_id, "新会话", CompileMode::Simple)
                    .await?,
            );
        }
        Ok(sessions)
    }

    pub async fn create_session(&self, project_id: &str, title: &str) -> Result<SessionRecord> {
        let session = self
            .store
            .create_session(project_id, title, CompileMode::Simple)
            .await?;
        self.events.emit_session_list(project_id).await?;
        Ok(session)
    }

    pub async fn archive_session(&self, session_id: String) -> Result<Option<SessionRecord>> {
        if self.active_turns.contains(&session_id).await {
            bail!("session has an active turn");
        }
        let emitter = self.interaction_emitter(session_id.clone());
        self.interactions
            .cancel_session(&session_id, "session archived", emitter)
            .await?;
        let archived = self.store.archive_session(&session_id).await?;
        if let Some(session) = &archived {
            self.events.emit_session_list(&session.project_id).await?;
        }
        Ok(archived)
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        let session_ids = self.store.list_project_session_ids(project_id).await?;
        if self.active_turns.contains_any(&session_ids).await {
            bail!("project has an active turn");
        }
        for session_id in session_ids {
            let emitter = self.interaction_emitter(session_id.clone());
            self.interactions
                .cancel_session(&session_id, "project archived", emitter)
                .await?;
        }
        let archived = self.store.archive_project(project_id).await?;
        if archived.is_some() {
            self.events.emit_session_list(project_id).await?;
        }
        Ok(archived)
    }

    pub async fn set_session_mode(&self, session_id: &str, mode: CompileMode) -> Result<()> {
        self.store.set_session_mode(session_id, mode).await?;
        let Some(session) = self.store.read_session(session_id).await? else {
            return Ok(());
        };
        self.events.emit_session_list(&session.project_id).await?;
        Ok(())
    }

    pub fn set_model_role(
        &self,
        role: ModelRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<PureConfig> {
        let provider_id = provider_id.trim();
        let model_slug = model_slug.trim();
        let mut config = self.config_store.load_or_default()?;
        let resolved_effort = {
            let provider = config.providers.get(provider_id).with_context(|| {
                format!(
                    "role {} references missing provider: {provider_id}",
                    role.key()
                )
            })?;
            let model = provider
                .models
                .iter()
                .find(|model| model.slug == model_slug)
                .with_context(|| {
                    format!(
                        "role {} references missing model: {provider_id}.{model_slug}",
                        role.key()
                    )
                })?;
            match effort.map(str::trim).filter(|value| !value.is_empty()) {
                Some(value) => {
                    if !model
                        .supported_efforts()
                        .iter()
                        .any(|candidate| candidate == value)
                    {
                        bail!(
                            "role {} uses unsupported effort '{}' for model {provider_id}.{model_slug}",
                            role.key(),
                            value
                        );
                    }
                    value.to_string()
                }
                None => model.default_effort().with_context(|| {
                    format!(
                        "role {} model {provider_id}.{model_slug} must define effort",
                        role.key()
                    )
                })?,
            }
        };
        let next_role = RoleConfig {
            provider: provider_id.to_string(),
            model: model_slug.to_string(),
            effort: ReasoningEffort::new(resolved_effort),
        };
        match role {
            ModelRole::Explorer => config.roles.explorer = next_role,
            ModelRole::Planner => config.roles.planner = next_role,
            ModelRole::Executor => config.roles.executor = next_role,
            ModelRole::Reviewer => config.roles.reviewer = next_role,
        }
        config.validate()?;
        self.config_store.save(&config)?;
        Ok(config)
    }

    pub async fn session_runtime(&self, session_id: &str) -> Result<SessionRuntimeRecord> {
        if let Some(snapshot) = self.store.load_session_runtime(session_id).await? {
            return Ok(snapshot);
        }
        let config = self.config_store.load_or_default()?;
        let mode = self
            .store
            .read_session(session_id)
            .await?
            .map(|session| CompileMode::from_label(&session.mode))
            .unwrap_or_default();
        let role = match mode {
            CompileMode::Simple => ModelRole::Executor,
            CompileMode::Task => ModelRole::Planner,
        };
        let resolved = config.resolve_role(role)?;
        let model = resolved
            .models
            .iter()
            .find(|model| model.slug == resolved.role_config.model)
            .or_else(|| resolved.models.first());
        Ok(default_session_runtime_record(session_id, model))
    }

    pub async fn provider_usages(&self) -> Result<Vec<crate::ProviderUsageRecord>> {
        let config = self.config_store.load_or_default()?;
        Ok(crate::provider_usage_records(&config).await)
    }

    pub async fn discovered_skills(&self, project_id: &str) -> Result<SkillCatalog> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("selected project not found")?;
        let config = self.config_store.load_or_default()?;
        let workspace_root = resolve_workspace_root(Path::new(&project.path))?;
        Ok(SkillCatalog::discover(&workspace_root, &config.skills)?)
    }
}
