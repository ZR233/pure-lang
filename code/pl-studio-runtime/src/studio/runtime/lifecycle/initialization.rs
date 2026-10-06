use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::config::{ConfigRuntime, ConfigStore};
use crate::studio::runtime_lock::{ResolvedRuntimeOptions, RuntimeLock, RuntimeLockOwner};
use crate::studio::startup::{StartupBackup, input_error};
use crate::{StudioStartupError, StudioStartupErrorKind, StudioStartupStage, StudioStore};

use super::super::StudioRuntime;
use super::super::background_task::{self, BackgroundTask};

type Observer = Arc<dyn Fn(StudioStartupStage) + Send + Sync>;

impl StudioRuntime {
    /// Initializes all global state before returning a usable runtime.
    pub async fn initialize(
        options: crate::StudioRuntimeOptions,
    ) -> Result<Self, StudioStartupError> {
        Self::initialize_with_observer(options, Arc::new(|_| {})).await
    }

    /// The observer is nonblocking and must not reenter initialization.
    /// Cancellation requests cleanup; the owned task retains the process lock until cleanup ends.
    pub async fn initialize_with_observer(
        options: crate::StudioRuntimeOptions,
        observer: Observer,
    ) -> Result<Self, StudioStartupError> {
        let cancellation = CancellationToken::new();
        let guard = cancellation.clone().drop_guard();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let (accepted, acceptance) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = Self::initialize_owned(options, observer.clone(), cancellation).await;
            if result.is_err() {
                observer(StudioStartupStage::Failed);
            }
            let retained = result.as_ref().ok().cloned();
            let delivered = sender.send(result).is_ok();
            if let Some(runtime) = retained {
                if delivered && acceptance.await.is_ok() {
                    observer(StudioStartupStage::Ready);
                    return;
                }
                if let Err(error) = runtime.dispose_startup().await {
                    tracing::error!(step = error.step, kind = ?error.kind, "cancelled startup cleanup failed");
                }
                observer(StudioStartupStage::Failed);
            }
        });
        let result = receiver
            .await
            .map_err(|error| internal("startup_owner", error))?;
        let _ = accepted.send(());
        guard.disarm();
        result
    }

    async fn initialize_owned(
        options: crate::StudioRuntimeOptions,
        observer: Observer,
        cancellation: CancellationToken,
    ) -> Result<Self, StudioStartupError> {
        let _timing = crate::startup_timing::Stage::new("initialize_studio");
        let resolved = options
            .resolve()
            .map_err(|error| environment("resolve_paths", error))?;
        let lock_path = resolved.paths.runtime_lock();
        let host = resolved.host;
        let lock = tokio::task::spawn_blocking(move || RuntimeLock::acquire(&lock_path, host))
            .await
            .map_err(|error| internal("acquire_lock", error))?
            .map_err(|error| environment("acquire_lock", error))?;
        let home = resolved.paths.home().to_owned();
        let mut backup = tokio::task::spawn_blocking(move || StartupBackup::resume(&home))
            .await
            .map_err(|error| internal("resume_backup", error))?
            .map_err(|error| environment("resume_backup", error))?;
        if backup.is_some() {
            observer(StudioStartupStage::Resetting);
        }
        loop {
            check_cancelled(&cancellation)?;
            match Self::prepare_attempt(&resolved, &observer, &cancellation).await {
                Ok(mut runtime) => {
                    if let Err(error) = check_cancelled(&cancellation) {
                        runtime.dispose_startup().await?;
                        return Err(error);
                    }
                    runtime.startup_recovery = backup.as_ref().map(StartupBackup::report);
                    if let Some(mut completed) = backup.take() {
                        let completion =
                            tokio::task::spawn_blocking(move || completed.complete()).await;
                        if let Err(error) = completion
                            .map_err(|error| internal("complete_backup", error))
                            .and_then(|result| {
                                result.map_err(|error| environment("complete_backup", error))
                            })
                        {
                            runtime.dispose_startup().await?;
                            return Err(error);
                        }
                    }
                    runtime.instance_lock = RuntimeLockOwner::new(Some(lock));
                    if let Err(error) = check_cancelled(&cancellation) {
                        runtime.dispose_startup().await?;
                        return Err(error);
                    }
                    return Ok(runtime);
                }
                Err(error)
                    if error.kind == StudioStartupErrorKind::PersistentData && backup.is_none() =>
                {
                    check_cancelled(&cancellation)?;
                    observer(StudioStartupStage::BackingUp);
                    let home = resolved.paths.home().to_owned();
                    let reason = match error.step {
                        "prepare_configuration" => "配置数据损坏、版本不受支持或引用无效",
                        "prepare_storage" => "本地存储或调用日志数据损坏、版本不受支持",
                        "directories" => "产品目录数据损坏或引用无效",
                        "worktree_leases" => "资源归属记录损坏或引用无效",
                        _ => "持久化缓存数据损坏或版本不受支持",
                    }
                    .to_owned();
                    backup = Some(
                        tokio::task::spawn_blocking(move || StartupBackup::begin(&home, reason))
                            .await
                            .map_err(|error| internal("backup", error))?
                            .map_err(|error| environment("backup", error))?,
                    );
                    observer(StudioStartupStage::Resetting);
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn prepare_attempt(
        resolved: &ResolvedRuntimeOptions,
        observer: &Observer,
        cancellation: &CancellationToken,
    ) -> Result<Self, StudioStartupError> {
        observer(StudioStartupStage::Preparing);
        let config_store = match resolved.host {
            crate::StudioHostKind::Test => ConfigStore::new(resolved.paths.config_paths()),
            crate::StudioHostKind::Desktop | crate::StudioHostKind::HttpServer => {
                ConfigStore::for_studio_home(resolved.paths.home().to_owned())
            }
        };
        observer(StudioStartupStage::WaitingForSteps);
        // join! deliberately waits for both results: a failing config cannot detach a database open.
        let (store, config) = tokio::join!(
            async {
                let _timing = crate::startup_timing::Stage::new("prepare_storage");
                StudioStore::prepare_database(&resolved.paths.database())
                    .await
                    .map_err(|error| StudioStartupError::input("prepare_storage", error))
            },
            async {
                let _timing = crate::startup_timing::Stage::new("prepare_configuration");
                tokio::task::spawn_blocking(move || ConfigRuntime::initialize(config_store))
                    .await
                    .map_err(|error| internal("prepare_configuration", error))?
                    .map_err(|error| {
                        StudioStartupError::input("prepare_configuration", input_error(error))
                    })
            }
        );
        let (store, config) = match (store, config) {
            (Ok(store), Ok(config)) => (store, config),
            (Ok(store), Err(error)) => {
                observer(StudioStartupStage::ClosingResources);
                store
                    .close()
                    .await
                    .map_err(|error| environment("close_storage", error))?;
                return Err(error);
            }
            (Err(storage), Err(config)) => return Err(prefer_environment(storage, config)),
            (Err(error), Ok(_)) => return Err(error),
        };
        let assembled = Self::assemble(
            store.clone(),
            config,
            resolved.paths.system_skills_dir(),
            resolved.helper_source.clone(),
        )
        .await;
        let runtime = match assembled {
            Ok(runtime) => runtime,
            Err(error) => {
                store
                    .close()
                    .await
                    .map_err(|error| environment("close_storage", error))?;
                return Err(StudioStartupError::input("assemble", error));
            }
        };
        let prepared = runtime.prepare_global(observer, cancellation).await;
        if let Err(error) = prepared {
            observer(StudioStartupStage::ClosingResources);
            runtime.dispose_startup().await?;
            return Err(error);
        }
        Ok(runtime)
    }

    async fn prepare_global(
        &self,
        observer: &Observer,
        cancellation: &CancellationToken,
    ) -> Result<(), StudioStartupError> {
        let settings = self
            .config_runtime
            .read()
            .map_err(|error| internal("read_configuration", error))?;
        self.runtime_state
            .apply(crate::studio::StudioRuntimeCommand::BeginInitialize {
                expected_revision: self.runtime_state.snapshot().revision,
                at: crate::studio::unix_seconds(),
            })
            .map_err(|error| internal("begin_initialize", error))?;
        observer(StudioStartupStage::ReadingProjects);
        let (usage, performance, update, directory, leases, ssh, skills) = tokio::join!(
            timed_step("prepare_provider_usage", self.provider_usage.load_cache()),
            timed_step(
                "prepare_model_performance",
                self.model_performance.load_cache()
            ),
            timed_step("prepare_update_cache", self.updater.load_cache()),
            timed_step(
                "prepare_directories",
                self.agent_facility.product_events.initialize_directories()
            ),
            timed_step(
                "prepare_resource_ownership",
                crate::studio::agent_host::worktree_lease::load_leases(&self.store)
            ),
            timed_step("prepare_remote_configuration", self.hydrate_ssh_servers()),
            timed_step(
                "prepare_system_skills",
                self.skills.refresh_system_skills(&settings.config.skills)
            )
        );
        let mut failures = Vec::new();
        for (step, result) in [
            ("provider_usage", usage),
            ("model_performance", performance),
            ("update_cache", update),
            ("directories", directory),
            ("system_skills", skills),
        ] {
            if let Err(error) = result {
                failures.push(StudioStartupError::input(step, error));
            }
        }
        match leases {
            Ok(leases) => self.agent_facility.worktrees.restore(leases),
            Err(error) => failures.push(StudioStartupError::input("worktree_leases", error)),
        }
        // Global SSH config is outside the product backup boundary; its diagnostic is independent.
        if let Err(error) = ssh {
            failures.push(environment("remote_configuration", error));
        }
        if let Some(error) = failures.into_iter().reduce(prefer_environment) {
            return Err(error);
        }
        check_cancelled(cancellation)?;
        self.publish_settings_state(settings)
            .map_err(|error| internal("publish_settings", error))?;
        observer(StudioStartupStage::StartingServices);
        self.store.calls().start().await;
        let writer = self
            .agent_facility
            .persistence
            .lock()
            .await
            .clone()
            .ok_or_else(|| {
                internal(
                    "start_persistence",
                    anyhow::anyhow!("missing persistence owner"),
                )
            })?;
        *self.persistence_observer.lock().await = Some(BackgroundTask::new(
            self.agent_facility
                .product_events
                .observe_persistence(writer.subscribe_state()),
        ));
        self.start_model_refresh().await;
        self.start_model_catalog_probes()
            .await
            .map_err(|error| internal("start_model_catalogs", error))?;
        self.start_tool_refresh().await;
        self.start_mcp_health_watcher().await;
        self.start_lsp_state_watcher().await;
        self.start_mcp_reconcile_background()
            .await
            .map_err(|error| internal("start_mcp", error))?;
        check_cancelled(cancellation)?;
        self.runtime_state
            .apply(crate::studio::StudioRuntimeCommand::FinishInitialize {
                expected_revision: self.runtime_state.snapshot().revision,
                at: crate::studio::unix_seconds(),
            })
            .map_err(|error| internal("finish_initialize", error))?;
        self.start_recovery_scan().await;
        Ok(())
    }

    async fn dispose_startup(&self) -> Result<(), StudioStartupError> {
        let catalogs = self.stop_model_catalog_probes().await;
        // All independent cleanup runs even when one task reports an error.
        let results = tokio::join!(
            self.stop_recovery_scan(),
            self.stop_model_refresh(),
            self.stop_tool_refresh(),
            self.stop_mcp_startup_reconcile(),
            self.stop_mcp_health_watcher(),
            self.stop_lsp_state_watcher()
        );
        let observer = background_task::stop(&self.persistence_observer).await;
        let mcp = self.external_runtimes.mcp.shutdown().await;
        let lsp = self.external_runtimes.lsp.shutdown().await;
        let ssh = self.ssh_manager.shutdown().await;
        // Clone (never `take`) the retained writer first, then stop it, and only drop the retained
        // owner once the writer actually stopped; a failed stop keeps the owner out of circulation.
        let writer = self.agent_facility.persistence.lock().await.clone();
        let persistence = match writer {
            Some(writer) => {
                let result = writer.shutdown().await;
                if result.is_ok() {
                    self.agent_facility.persistence.lock().await.take();
                }
                result
            }
            None => Ok(()),
        };
        let storage = self.store.close().await;
        catalogs.map_err(|error| internal("stop_model_catalogs", error))?;
        for result in [
            results.0, results.1, results.2, results.3, results.4, results.5,
        ] {
            result.map_err(|error| internal("stop_services", error))?;
        }
        observer.map_err(|error| {
            internal(
                "stop_persistence_observer",
                anyhow::anyhow!(error.to_string()),
            )
        })?;
        mcp.map_err(|error| internal("close_mcp", error))?;
        lsp.map_err(|error| internal("close_lsp", error))?;
        ssh.map_err(|error| environment("close_ssh", error))?;
        persistence.map_err(|error| environment("close_writer", error))?;
        storage.map_err(|error| environment("close_storage", error))?;
        self.instance_lock.release();
        Ok(())
    }
}

fn prefer_environment(first: StudioStartupError, second: StudioStartupError) -> StudioStartupError {
    tracing::error!(step = second.step, kind = ?second.kind, "additional startup step failed");
    if first.kind == StudioStartupErrorKind::PersistentData
        && second.kind != StudioStartupErrorKind::PersistentData
    {
        second
    } else {
        first
    }
}

fn internal(step: &'static str, source: impl Into<anyhow::Error>) -> StudioStartupError {
    StudioStartupError::new(step, StudioStartupErrorKind::Internal, source)
}

fn environment(step: &'static str, source: impl Into<anyhow::Error>) -> StudioStartupError {
    StudioStartupError::new(step, StudioStartupErrorKind::Environment, source)
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), StudioStartupError> {
    if cancellation.is_cancelled() {
        Err(StudioStartupError::new(
            "cancelled",
            StudioStartupErrorKind::Cancelled,
            anyhow::anyhow!("startup cancelled"),
        ))
    } else {
        Ok(())
    }
}

async fn timed_step<F: std::future::Future>(name: &'static str, future: F) -> F::Output {
    let _timing = crate::startup_timing::Stage::new(name);
    future.await
}
