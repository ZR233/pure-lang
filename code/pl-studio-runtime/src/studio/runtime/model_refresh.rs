//! Current product bindings are queued without disturbing an in-flight Turn.
use super::{
    StudioRuntime,
    background_task::{self, BackgroundTask},
};
use anyhow::{Context, Result};
use pl_core::{
    model::{DynModelSession, Model, ModelError, ModelFactory, ModelFailureKind},
    thread::{DeferredModelUpdate, DeferredModelUpdatePrecondition, ThreadHandle},
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
        thread: &ThreadHandle,
    ) -> (
        DeferredModelUpdatePrecondition,
        Result<(ResolvedModelRoute, crate::config::StudioConfig)>,
    ) {
        let state = thread.snapshot();
        let extension_id = if state
            .extensions
            .contains_key(crate::studio::model_route::AGENT_PROFILE_EXTENSION)
        {
            crate::studio::model_route::AGENT_PROFILE_EXTENSION
        } else {
            crate::studio::model_route::MODEL_ROUTE_EXTENSION
        };
        let precondition = DeferredModelUpdatePrecondition::extension(
            extension_id,
            state
                .extensions
                .get(extension_id)
                .map(|record| record.revision),
        );
        let binding = async {
            let record = self.read_owned_thread(thread_id).await?;
            let is_child = record.parent_thread_id.is_some();
            let config = self.config_runtime.read()?.config;
            let (_, selector) = crate::studio::model_route::route_record(&state, is_child)?
                .context("Thread has no saved model route")?;
            let role = if is_child {
                pl_protocol::AgentRoleId::new(record.role)?
            } else {
                crate::config::StudioRole::Planner.id()
            };
            let route = config.models.resolve_route(role, &selector)?;
            if !is_child {
                let mode_id =
                    crate::studio::thread_projection::saved_mode(&state)?.unwrap_or(record.mode);
                let mode = self
                    .thread_modes
                    .snapshot()
                    .mode(&mode_id)
                    .context("current Thread Mode is unavailable")?;
                crate::mode::validate_thread_mode_model(Some(&mode), &route.model)?;
            }
            Ok((route, config))
        }
        .await;
        (precondition, binding)
    }

    pub(super) async fn queue_thread_model(
        &self,
        thread: &ThreadHandle,
        route: &ResolvedModelRoute,
        config: &crate::config::StudioConfig,
        precondition: DeferredModelUpdatePrecondition,
    ) -> Result<()> {
        thread
            .queue_deferred_model_update_if_current(
                Self::deferred_model_update(route, config)?,
                precondition,
                Vec::new(),
            )
            .await?;
        Ok(())
    }

    pub(super) fn deferred_model_update(
        route: &ResolvedModelRoute,
        config: &crate::config::StudioConfig,
    ) -> Result<DeferredModelUpdate> {
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
        Ok(DeferredModelUpdate::new(
            key,
            factory,
            crate::compaction::preparer(route, config.runtime.openai_compaction_mode)?,
        ))
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
                    let (precondition, binding) = runtime.model_binding(&id, &thread).await;
                    let result = match binding {
                        Ok((route, config)) => {
                            runtime
                                .queue_thread_model(&thread, &route, &config, precondition.clone())
                                .await
                        }
                        Err(error) => Err(error),
                    };
                    if let Err(error) = result {
                        // Invalid current bindings become an explicit failure on the next Turn,
                        // never silent reuse of an old provider or disabled Profile.
                        let update = DeferredModelUpdate::new(
                            format!("unavailable:{revision}"),
                            ModelFactory::new(UnavailableBinding(Arc::from(
                                error.into_boxed_dyn_error(),
                            ))),
                            None,
                        );
                        let _ = thread
                            .queue_deferred_model_update_if_current(
                                update,
                                precondition,
                                Vec::new(),
                            )
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
