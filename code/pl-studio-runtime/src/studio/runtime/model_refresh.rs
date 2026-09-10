//! Current product bindings are queued without disturbing an in-flight Turn.
use super::{
    StudioRuntime,
    background_task::{self, BackgroundTask},
};
use anyhow::Result;
use pl_core::{
    model::{DynModelSession, Model, ModelError, ModelFactory, ModelFailureKind},
    thread::ThreadHandle,
};
use pl_model::{
    config::ResolvedModelRoute,
    runtime::{ModelRuntime, ThreadModel},
};
use std::sync::Arc;

impl StudioRuntime {
    pub(super) async fn model_binding(
        &self,
        thread_id: &str,
    ) -> Result<(ResolvedModelRoute, crate::config::StudioConfig)> {
        let record = self.read_owned_thread(thread_id).await?;
        let config = self.config_runtime.clone();
        tokio::task::spawn_blocking(move || -> Result<_> {
            if record.parent_thread_id.is_none() {
                let config = config.read()?.config;
                Ok((
                    config
                        .models
                        .resolve(&crate::config::StudioRole::Planner.id())?,
                    config,
                ))
            } else {
                let profile = config.resolve_agent_profile(&record.role)?;
                Ok((profile.route, profile.config))
            }
        })
        .await?
    }

    pub(super) async fn queue_thread_model(
        &self,
        thread: &ThreadHandle,
        route: &ResolvedModelRoute,
        config: &crate::config::StudioConfig,
    ) -> Result<()> {
        let search = crate::search::plan_web_searches(
            &config.models,
            route,
            &config.web_search,
            config.deepseek_web_search.enabled,
        )?;
        let mut hosted = search.hosted_tools(&config.web_search)?;
        if search.visibility() != crate::search::ToolVisibilityConstraint::Exclusive {
            hosted.extend(crate::programmatic::hosted_tool(route));
        }
        // Hash sorted structured inputs in memory. Credentials never enter logs, storage or model context.
        let key = crate::hash::canonical_json_hash(&serde_json::json!({
            "provider": route.provider_id, "endpoint": route.endpoint, "model": route.model,
            "effort": route.effort, "pricing": route.pricing_mode,
            "compaction": config.runtime.openai_compaction_mode,
            "hosted": hosted.iter().map(|tool| match tool {
                pl_model::runtime::HostedTool::WebSearch(options) => pl_protocol::ToolSpec::WebSearch { options: options.clone() },
                pl_model::runtime::HostedTool::ProgrammaticToolCalling => pl_protocol::ToolSpec::ProgrammaticToolCalling,
            }).collect::<Vec<_>>(),
        }));
        let factory = ModelFactory::new(
            ThreadModel::new(ModelRuntime::from_route(route)?, route.reasoning_config())
                .with_hosted_tools(hosted),
        );
        thread
            .queue_model_update(
                key,
                factory,
                crate::compaction::preparer(route, config.runtime.openai_compaction_mode)?,
            )
            .await?;
        Ok(())
    }

    pub(in crate::studio::runtime) async fn start_model_refresh(&self) {
        let mut slot = self.settings_refresh.lock().await;
        if slot.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let mut updates = self.settings_updates.subscribe();
        let runtime = self.clone();
        *slot = Some(BackgroundTask::new(tokio::spawn(async move {
            while updates.changed().await.is_ok() {
                let revision = updates.borrow_and_update().revision;
                for (id, thread) in runtime.threads.observed_threads() {
                    let result = match runtime.model_binding(&id).await {
                        Ok((route, config)) => {
                            runtime.queue_thread_model(&thread, &route, &config).await
                        }
                        Err(error) => Err(error),
                    };
                    if let Err(error) = result {
                        // Invalid current bindings become an explicit failure on the next Turn,
                        // never silent reuse of an old provider or disabled Profile.
                        let factory = ModelFactory::new(UnavailableBinding(Arc::from(
                            error.into_boxed_dyn_error(),
                        )));
                        let _ = thread
                            .queue_model_update(format!("unavailable:{revision}"), factory, None)
                            .await;
                    }
                }
            }
        })));
    }

    pub(in crate::studio::runtime) async fn stop_model_refresh(&self) -> Result<()> {
        background_task::stop(&self.settings_refresh)
            .await
            .map_err(anyhow::Error::new)
    }
}

#[derive(Debug)]
struct UnavailableBinding(Arc<dyn std::error::Error + Send + Sync>);
impl Model for UnavailableBinding {
    async fn open_session(&self) -> Result<DynModelSession, ModelError> {
        Err(ModelError {
            kind: ModelFailureKind::Unavailable,
            details: None,
            usage: Default::default(),
            source: Some(Box::new(BindingFailure(self.0.clone()))),
        })
    }
}
#[derive(Debug, thiserror::Error)]
#[error("current model binding is unavailable: {0}")]
struct BindingFailure(#[source] Arc<dyn std::error::Error + Send + Sync>);
