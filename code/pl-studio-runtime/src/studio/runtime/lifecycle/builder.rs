use anyhow::Result;

use crate::config::{ConfigPaths, ConfigRuntime, ConfigStore};
use crate::studio::agent_host::ThreadWriteBehindWriter;
use crate::studio::runtime_lock::{RuntimeLock, RuntimeLockOwner};
use crate::studio::{ProductEventBus, StudioRuntimeState, StudioStore};
use pl_tool::mcp::{McpConnector, McpRuntime};

use super::super::StudioRuntime;
use super::super::attachment_drafts::AttachmentDraftRuntime;
use super::super::lsp_state::LspStateRuntime;
use super::super::mcp_health::McpStateRuntime;
use super::super::residency::ThreadResidency;
use super::super::{
    ModelPerformanceOwner, ProviderUsageRuntime, ShutdownProgressBus, SkillCatalogRuntime,
    StudioAgentFacility, StudioExternalRuntimes, StudioUpdateRuntime,
};

impl StudioRuntime {
    pub async fn default_app() -> pl_protocol::studio::StudioResult<Self> {
        Self::with_options(crate::StudioRuntimeOptions::desktop()).await
    }

    /// Creates the one Studio runtime owning the resolved home and its process lock.
    pub async fn with_options(
        options: crate::StudioRuntimeOptions,
    ) -> pl_protocol::studio::StudioResult<Self> {
        let resolved = options.resolve()?;
        let lock_path = resolved.paths.runtime_lock();
        let system_skills_dir = resolved.paths.system_skills_dir();
        let host = resolved.host;
        let instance_lock =
            tokio::task::spawn_blocking(move || RuntimeLock::acquire(&lock_path, host))
                .await
                .map_err(|error| {
                    tracing::error!(error = %error, "Studio runtime lock task failed");
                    pl_protocol::studio::StudioError::internal()
                })??;
        let reset_path = resolved.paths.database();
        // Once reset starts, its task retains the exclusive owner through every backup/marker IO.
        // Dropping the startup waiter must not release the lock while blocking filesystem work runs.
        let instance_lock = tokio::spawn(async move {
            crate::studio::session_reset::prepare(&reset_path, &instance_lock).await?;
            Ok::<_, anyhow::Error>(instance_lock)
        })
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "session recovery owner task failed");
            pl_protocol::studio::StudioError::internal()
        })?
        .map_err(|error| {
            tracing::error!(error = %error, "failed to coordinate session storage recovery");
            pl_protocol::studio::StudioError::storage()
        })?;
        let store = StudioStore::open(resolved.paths.database())
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "failed to open Studio storage");
                pl_protocol::studio::StudioError::storage()
            })?;
        let config_store = match host {
            crate::StudioHostKind::Test => ConfigStore::new(ConfigPaths::from_config_dir(
                resolved.paths.home().to_path_buf(),
            )),
            crate::StudioHostKind::Desktop | crate::StudioHostKind::HttpServer => {
                ConfigStore::for_studio_home(resolved.paths.home().to_path_buf())
            }
        };
        let runtime = Self::with_runtime_state_and_lock(
            store,
            config_store,
            StudioRuntimeState::new(),
            Some(instance_lock),
            Some(system_skills_dir),
        )
        .map_err(|error| {
            tracing::error!(error = %error, "failed to initialize Studio runtime");
            pl_protocol::studio::StudioError::internal()
        })?;
        runtime.hydrate_ssh_servers().await.map_err(|error| {
            tracing::error!(error = %error, "failed to initialize SSH server registry");
            pl_protocol::studio::StudioError::storage()
        })?;
        Ok(runtime)
    }

    fn with_runtime_state_and_lock(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
        instance_lock: Option<RuntimeLock>,
        system_skills_dir: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        let config_runtime = ConfigRuntime::initialize(config_store)?;
        let (settings_updates, _) = tokio::sync::watch::channel(config_runtime.read()?);
        // 进程级共享 writer 先于所有 owner 构造：ProductEventBus 与
        // ThreadRepository 共用同一 write-behind 队列。
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let model_performance =
            ModelPerformanceOwner::new(store.clone(), writer.clone(), product_events.clone());
        let ssh_manager = std::sync::Arc::new(crate::worker_assets::ssh_manager());
        let worktrees =
            crate::studio::agent_host::worktree_lease::WorktreeLeaseOwner::new(writer.clone());
        let persistence = writer;
        product_events.observe_persistence(persistence.subscribe_state());
        let provider_usage = ProviderUsageRuntime::new(store.clone(), product_events.clone());
        let updater = StudioUpdateRuntime::new(store.clone(), product_events.clone())?;
        let mcp_state = McpStateRuntime::new();
        let lsp_state = LspStateRuntime::new(product_events.clone());
        let attachment_drafts =
            AttachmentDraftRuntime::new(store.attachments_dir().join("drafts"))?;
        let thread_modes = crate::mode::ThreadModeManager::default();
        crate::studio::thread::register_builtins(&thread_modes)?;
        let mcp = McpRuntime::new(McpConnector::default()).handle();
        let lsp = pl_lsp::runtime::LspRuntimeRegistry::new();
        let skills = match system_skills_dir {
            Some(system_skills_dir) => {
                SkillCatalogRuntime::new(product_events.clone(), system_skills_dir)
            }
            None => SkillCatalogRuntime::default(),
        };
        let thread_factory = crate::studio::thread_factory::StudioThreadFactory::new(
            crate::studio::thread_factory::StudioThreadServices {
                store: store.clone(),
                worktrees: worktrees.clone(),
                model_performance: model_performance.clone(),
                product_events: product_events.clone(),
                config_runtime: config_runtime.clone(),
                mcp_runtime: mcp.clone(),
                lsp_runtime: lsp.clone(),
                skills: skills.clone(),
                thread_modes: thread_modes.clone(),
                ssh_manager: ssh_manager.clone(),
            },
        );
        let threads = crate::thread_assembler::StudioThreadAssembler::default();
        threads.set_agent_queries(
            config_runtime.clone(),
            store.clone(),
            product_events.clone(),
        )?;
        threads.set_child_factory(crate::thread_assembler::ConfiguredChildFactory::new(
            config_runtime.clone(),
            thread_factory.clone(),
        ))?;
        let thread_observations = super::super::thread_observation::ThreadObservations::new(
            super::super::thread_observation::ObservationServices {
                store: store.clone(),
                events: product_events.clone(),
                performance: model_performance.clone(),
                threads: threads.clone(),
                writer: persistence.clone(),
            },
        );
        thread_observations.install(thread_factory.clone())?;
        Ok(Self {
            thread_observations,
            settings_updates,
            settings_refresh: Default::default(),
            tool_refresh: Default::default(),
            rejected_tools: Default::default(),
            tool_catalog_updates: Default::default(),
            threads,
            thread_factory,
            instance_lock: RuntimeLockOwner::new(instance_lock),
            store,
            residency: ThreadResidency::new(),
            shutdown_progress: ShutdownProgressBus::new(),
            config_runtime,
            external_runtimes: StudioExternalRuntimes {
                mcp,
                mcp_state,
                mcp_startup_reconcile: Default::default(),
                mcp_health_watcher: Default::default(),
                lsp,
                lsp_state,
                lsp_state_watcher: Default::default(),
            },
            agent_facility: StudioAgentFacility {
                worktrees,
                persistence: std::sync::Arc::new(tokio::sync::Mutex::new(Some(persistence))),
                product_events: product_events.clone(),
            },
            runtime_state,
            recovery: crate::studio::StudioRecoveryRegistry::new(),
            skills,
            thread_modes,
            provider_usage,
            model_performance,
            updater,
            activation: Default::default(),
            attachment_drafts,
            ssh_manager,
            lifecycle_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            title_tasks: Default::default(),
        })
    }
}
