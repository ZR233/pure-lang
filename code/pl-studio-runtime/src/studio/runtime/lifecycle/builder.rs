use anyhow::Result;

use crate::config::ConfigRuntime;
use crate::studio::agent_host::ThreadWriteBehindWriter;
use crate::studio::runtime_lock::RuntimeLockOwner;
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
    pub(super) fn assemble(
        store: StudioStore,
        config_runtime: ConfigRuntime,
        system_skills_dir: std::path::PathBuf,
        helper_source: crate::worker_assets::RemoteHelperSource,
    ) -> Result<Self> {
        let (settings_updates, _) = tokio::sync::watch::channel(config_runtime.read()?);
        // 进程级共享 writer 先于所有 owner 构造：ProductEventBus 与
        // ThreadRepository 共用同一 write-behind 队列。
        let writer = ThreadWriteBehindWriter::new(store.clone());
        let product_events = ProductEventBus::new(store.clone(), writer.clone());
        let model_performance = ModelPerformanceOwner::new(store.clone(), product_events.clone());
        let ssh_manager = std::sync::Arc::new(crate::worker_assets::ssh_manager(&helper_source));
        let worktrees =
            crate::studio::agent_host::worktree_lease::WorktreeLeaseOwner::new(writer.clone());
        let persistence = writer;
        let provider_usage = ProviderUsageRuntime::new(store.clone(), product_events.clone());
        let updater = StudioUpdateRuntime::new(store.clone(), product_events.clone())?;
        let mcp_state = McpStateRuntime::new();
        let lsp_state = LspStateRuntime::new(product_events.clone());
        let attachment_drafts = AttachmentDraftRuntime::new(store.attachment_drafts_dir())?;
        let thread_modes = crate::mode::ThreadModeManager::default();
        crate::studio::thread::register_builtins(&thread_modes)?;
        let mcp = McpRuntime::new(McpConnector::default()).handle();
        let lsp = pl_lsp::runtime::LspRuntimeRegistry::new();
        let skills = SkillCatalogRuntime::new(product_events.clone(), system_skills_dir);
        let thread_factory = crate::studio::thread_factory::StudioThreadFactory::new(
            crate::studio::thread_factory::StudioThreadServices {
                store: store.clone(),
                worktrees: worktrees.clone(),
                product_events: product_events.clone(),
                config_runtime: config_runtime.clone(),
                mcp_runtime: mcp.clone(),
                lsp_runtime: lsp.clone(),
                skills: skills.clone(),
                thread_modes: thread_modes.clone(),
                ssh_manager: ssh_manager.clone(),
                helper_source,
            },
        );
        let threads = crate::thread_assembler::StudioThreadAssembler::default();
        threads.set_agent_services(
            config_runtime.clone(),
            store.clone(),
            product_events.clone(),
            thread_factory.clone(),
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
            startup_recovery: None,
            persistence_observer: Default::default(),
            thread_observations,
            settings_updates,
            settings_refresh: Default::default(),
            model_catalog_tasks: Default::default(),
            tool_refresh: Default::default(),
            rejected_tools: Default::default(),
            tool_catalog_updates: Default::default(),
            threads,
            thread_factory,
            instance_lock: RuntimeLockOwner::new(None),
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
            runtime_state: StudioRuntimeState::new(),
            recovery: crate::studio::StudioRecoveryRegistry::new(),
            recovery_task: Default::default(),
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
