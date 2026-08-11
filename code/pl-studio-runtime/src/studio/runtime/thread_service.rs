use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::{ModelRouteConfig, ProviderId, ReasoningEffort, StudioConfig, StudioRole};
use crate::skill::SkillCatalog;
use crate::studio::agent_host::root_agent_id;
use crate::studio::records::{ProjectRecord, ThreadRecord};
use crate::{StudioMode, resolve_workspace_root};

use super::StudioRuntime;

impl StudioRuntime {
    pub async fn open_project(&self, path: impl AsRef<Path>) -> Result<ProjectRecord> {
        let path = path.as_ref();
        let _ = resolve_workspace_root(path)?;
        self.store.upsert_project(path).await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.store.list_projects().await
    }

    pub async fn ensure_project_threads(&self, project_id: &str) -> Result<Vec<ThreadRecord>> {
        let mut threads = self.store.list_root_threads(project_id).await?;
        if threads.is_empty() {
            threads.push(
                self.store
                    .create_thread(project_id, "新会话", StudioMode::Simple)
                    .await?,
            );
        }
        Ok(threads)
    }

    pub async fn create_thread(&self, project_id: &str, title: &str) -> Result<ThreadRecord> {
        let thread = self
            .store
            .create_thread(project_id, title, StudioMode::Simple)
            .await?;
        self.agent_facility
            .product_events
            .emit_thread_directory(project_id)
            .await?;
        Ok(thread)
    }

    pub async fn archive_thread(&self, thread_id: String) -> Result<Option<ThreadRecord>> {
        let Some(thread) = self.store.read_thread(&thread_id).await? else {
            return Ok(None);
        };
        if thread.parent_thread_id.is_some() {
            bail!("only a root Thread can be archived");
        }
        if self
            .store
            .find_active_task_run_for_root_thread(&thread_id)
            .await?
            .is_some()
        {
            bail!("thread cannot be archived while a task is active");
        }
        let thread_tree = self
            .store
            .list_threads(&thread.project_id)
            .await?
            .into_iter()
            .filter(|candidate| candidate.root_thread_id == thread_id)
            .collect::<Vec<_>>();
        for candidate in &thread_tree {
            if self.thread_is_busy(&candidate.id).await? {
                bail!("thread tree has an active turn or pending input");
            }
        }
        for candidate in &thread_tree {
            let emitter = self.interaction_emitter(candidate.id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(&candidate.id, "thread archived", emitter)
                .await?;
        }
        let archived = self.store.archive_thread(&thread_id).await?;
        if let Some(thread) = &archived {
            self.agent_facility
                .product_events
                .emit_thread_directory(&thread.project_id)
                .await?;
        }
        Ok(archived)
    }

    pub async fn archive_project(&self, project_id: &str) -> Result<Option<ProjectRecord>> {
        if self
            .store
            .list_task_runs_for_project(project_id)
            .await?
            .iter()
            .any(|run| !run.phase.is_terminal())
        {
            bail!("project has an active task");
        }
        let thread_ids = self.store.list_project_thread_ids(project_id).await?;
        for thread_id in &thread_ids {
            if self.thread_is_busy(thread_id).await? {
                bail!("project has an active turn");
            }
        }
        for thread_id in thread_ids {
            let emitter = self.interaction_emitter(thread_id.clone());
            self.agent_facility
                .interactions
                .cancel_thread(&thread_id, "project archived", emitter)
                .await?;
        }
        let archived = self.store.archive_project(project_id).await?;
        if archived.is_some() {
            self.agent_facility
                .product_events
                .emit_thread_directory(project_id)
                .await?;
        }
        Ok(archived)
    }

    pub async fn set_thread_mode(&self, thread_id: &str, mode: StudioMode) -> Result<()> {
        let thread = self
            .store
            .read_thread(thread_id)
            .await?
            .context("selected Thread not found")?;
        if thread.parent_thread_id.is_some() {
            bail!("only a root Thread can change mode");
        }
        if self
            .store
            .find_active_task_run_for_root_thread(thread_id)
            .await?
            .is_some()
        {
            bail!("thread mode cannot change while a task is active");
        }
        let desired_role = match mode {
            StudioMode::Simple => StudioRole::Executor.id(),
            StudioMode::Task => StudioRole::Planner.id(),
        };
        let (handle, agent_id) = self.ensure_thread_agent(thread_id).await?;
        let snapshot = handle
            .snapshot(agent_id.clone())
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        let previous_role = snapshot.identity.role;
        let role_changed = previous_role != desired_role;
        if role_changed {
            handle
                .reconfigure_idle_role(agent_id.clone(), desired_role)
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
        }
        if let Err(error) = self.store.set_thread_mode(thread_id, mode).await {
            if role_changed
                && let Err(rollback_error) =
                    handle.reconfigure_idle_role(agent_id, previous_role).await
            {
                bail!(
                    "failed to persist Thread mode: {error}; actor role rollback failed: {rollback_error}"
                );
            }
            return Err(error);
        }
        self.agent_facility
            .product_events
            .emit_thread_directory(&thread.project_id)
            .await?;
        Ok(())
    }

    pub fn set_model_role(
        &self,
        role: StudioRole,
        provider_id: &str,
        model_slug: &str,
        effort: Option<&str>,
    ) -> Result<StudioConfig> {
        let provider_id = provider_id.trim();
        let model_slug = model_slug.trim();
        let mut config = self.config_store.load_or_default()?;
        let provider_key = ProviderId::new(provider_id)?;
        let resolved_effort = {
            let provider = config
                .models
                .providers
                .get(&provider_key)
                .with_context(|| {
                    format!(
                        "role {} references missing provider: {provider_id}",
                        role.key()
                    )
                })?;
            let models = provider.effective_models()?;
            let model = models
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
                    Some(value.to_string())
                }
                None => model.default_effort(),
            }
        };
        let next_route = ModelRouteConfig {
            provider: provider_key,
            model: model_slug.to_string(),
            effort: resolved_effort.map(ReasoningEffort::new),
        };
        config.models.routes.insert(role.id(), next_route);
        config.validate()?;
        self.config_store.save(&config)?;
        Ok(config)
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

    pub(super) async fn thread_is_busy(&self, thread_id: &str) -> Result<bool> {
        let runtime = self.agent_framework().await?;
        match runtime.handle().snapshot(root_agent_id(thread_id)).await {
            Ok(snapshot) => Ok(snapshot.active_turn_id.is_some() || snapshot.pending_inputs > 0),
            Err(crate::AgentRuntimeError::NotFound(_)) => Ok(false),
            Err(error) => Err(anyhow::anyhow!(error)),
        }
    }
}
