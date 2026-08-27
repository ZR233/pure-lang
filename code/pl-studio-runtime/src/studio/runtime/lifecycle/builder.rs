use anyhow::Result;

use crate::config::{ConfigPaths, ConfigRuntime, ConfigStore};
use crate::studio::agent_host::{
    StudioAgentRepository, StudioAgentResources, ThreadWriteBehindWriter,
};
use crate::studio::runtime_lock::{RuntimeLock, RuntimeLockOwner};
use crate::studio::task_coordinator::TaskCoordinator;
use crate::studio::{
    InteractionService, ProductEventBus, StudioRuntimeState, StudioStore, TaskRuntime,
};
use crate::{McpConnector, McpRuntime};

use super::super::StudioRuntime;
use super::super::attachment_drafts::AttachmentDraftRuntime;
use super::super::lsp_state::LspStateRuntime;
use super::super::mcp_health::McpStateRuntime;
use super::super::residency::ThreadResidency;
use super::super::{
    ProviderUsageRuntime, ShutdownProgressBus, SkillCatalogRuntime, StudioAgentFacility,
    StudioExternalRuntimes, StudioUpdateRuntime,
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
        Self::with_runtime_state_and_lock(
            store,
            config_store,
            StudioRuntimeState::new(),
            Some(instance_lock),
            Some(system_skills_dir),
        )
        .map_err(|error| {
            tracing::error!(error = %error, "failed to initialize Studio runtime");
            pl_protocol::studio::StudioError::internal()
        })
    }

    #[cfg(test)]
    pub(crate) fn new(store: StudioStore, config_store: ConfigStore) -> Result<Self> {
        Self::with_runtime_state(store, config_store, StudioRuntimeState::ready())
    }

    #[cfg(test)]
    pub(in crate::studio::runtime) fn with_runtime_state(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
    ) -> Result<Self> {
        Self::with_runtime_state_and_lock(store, config_store, runtime_state, None, None)
    }

    fn with_runtime_state_and_lock(
        store: StudioStore,
        config_store: ConfigStore,
        runtime_state: StudioRuntimeState,
        instance_lock: Option<RuntimeLock>,
        system_skills_dir: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        let config_runtime = ConfigRuntime::initialize(config_store)?;
        let interactions = InteractionService::new(store.clone());
        // 进程级共享 writer 先于所有 owner 构造：ProductEventBus 的目录提交、
        // TaskRuntime 与 ThreadRepository 必须共用同一 write-behind 队列。
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let task_runtime = TaskRuntime::with_writer(store.clone(), product_events.clone(), writer);
        let persistence = StudioAgentRepository::with_writer(store.clone(), task_runtime.writer());
        product_events.observe_persistence(persistence.writer().subscribe_state());
        let task_coordinator = std::sync::Arc::new(TaskCoordinator::new(
            store.clone(),
            task_runtime.clone(),
            interactions.clone(),
        ));
        let provider_usage = ProviderUsageRuntime::new(store.clone(), product_events.clone());
        let updater = StudioUpdateRuntime::new(store.clone(), product_events.clone())?;
        let tool_manager = pl_core::ToolManager::new();
        let mcp_state = McpStateRuntime::new();
        let lsp_state = LspStateRuntime::new(product_events.clone());
        let attachment_drafts =
            AttachmentDraftRuntime::new(store.attachments_dir().join("drafts"))?;
        Ok(Self {
            instance_lock: RuntimeLockOwner::new(instance_lock),
            store,
            residency: ThreadResidency::new(),
            shutdown_progress: ShutdownProgressBus::new(),
            config_runtime,
            external_runtimes: StudioExternalRuntimes {
                mcp: McpRuntime::new(McpConnector::default()).handle(),
                mcp_state,
                mcp_startup_reconcile: Default::default(),
                mcp_health_watcher: Default::default(),
                lsp: pl_lsp::LspRuntimeRegistry::new(),
                lsp_state,
                lsp_state_watcher: Default::default(),
            },
            agent_facility: StudioAgentFacility {
                framework: Default::default(),
                resources: StudioAgentResources::default(),
                tool_manager,
                interactions,
                persistence: std::sync::Arc::new(tokio::sync::Mutex::new(Some(persistence))),
                product_events: product_events.clone(),
            },
            runtime_state,
            recovery: crate::studio::StudioRecoveryRegistry::new(),
            skills: match system_skills_dir {
                Some(system_skills_dir) => {
                    SkillCatalogRuntime::new(product_events.clone(), system_skills_dir)
                }
                None => SkillCatalogRuntime::default(),
            },
            provider_usage,
            updater,
            activation: Default::default(),
            task_runtime,
            task_coordinator,
            attachment_drafts,
            lifecycle_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            #[cfg(test)]
            initialization_entry_barrier: None,
        })
    }
}
