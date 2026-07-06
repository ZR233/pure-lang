use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn default_tools_register_bash_and_agent_tools() {
    let mut core = PureCore::default_provider().unwrap();

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    assert!(core.tools.get("bash").is_some());
    assert!(core.tools.get("write_stdin").is_some());
    assert!(core.tools.get("spawn_agent").is_some());
    assert!(core.tools.get("wait_agent").is_some());
    assert!(core.tools.get("list_agents").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("plan_exit").is_some());
    assert!(core.tools.get("subagent").is_none());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("apply_patch").is_some());
    assert!(core.tools.get("lsp_query").is_none());
    assert!(core.tools.get("git_status").is_none());
    assert!(core.tools.get("git_push").is_none());
    assert!(core.tools.get("docker").is_none());
    assert!(core.tools.get("container").is_none());
}

#[tokio::test]
async fn default_capabilities_keep_product_tools_disabled() {
    let capabilities = crate::config::ToolCapabilityConfig::default();

    assert!(!capabilities.git);
    assert!(!capabilities.docker);
    assert!(!capabilities.container);
}

#[tokio::test]
async fn tool_set_builder_can_disable_shell_and_subagents() {
    let mut core = PureCore::default_provider().unwrap();
    let capabilities = crate::config::ToolCapabilityConfig {
        bash: false,
        subagents: false,
        ..Default::default()
    };

    core.register_tools_with_capabilities(std::env::temp_dir(), None, capabilities)
        .await;

    assert!(core.tools.get("bash").is_none());
    assert!(core.tools.get("write_stdin").is_none());
    assert!(core.tools.get("spawn_agent").is_none());
    assert!(core.tools.get("wait_agent").is_none());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("plan_exit").is_some());
}

#[test]
fn register_git_tools_exposes_git_pack_explicitly() {
    let mut core = PureCore::default_provider().unwrap();

    core.register_git_tools(
        crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
        std::sync::Arc::new(crate::tool::LocalExecutionBackend),
        std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
    );

    assert!(core.tools.get("git_status").is_some());
    assert!(core.tools.get("git_diff").is_some());
    assert!(core.tools.get("git_branch").is_some());
    assert!(core.tools.get("git_fetch").is_some());
    assert!(core.tools.get("git_commit").is_some());
    assert!(core.tools.get("git_push").is_some());
    assert!(core.tools.get("git_workspace_info").is_some());
}

#[tokio::test]
async fn profiled_local_workspace_registers_default_tools() {
    let runtime = CoreRuntimeProfile::local_workspace(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
        .unwrap()
        .with_runtime_profile(runtime)
        .build();

    core.register_profile_tools().await;

    assert!(core.tools.get("bash").is_some());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("spawn_agent").is_some());
}

#[tokio::test]
async fn profiled_host_tools_do_not_register_local_workspace_tools() {
    let runtime = CoreRuntimeProfile::host_provided(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = PureCoreBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
        .unwrap()
        .with_runtime_profile(runtime)
        .build();

    core.register_profile_tools().await;

    assert!(core.tools.is_empty());
}

#[tokio::test]
async fn default_tools_register_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = PureCore::default_provider()
        .unwrap()
        .with_lsp_runtime(registry.clone());

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    // 空注册表没有可用语言，不应注册任何 LSP 工具。
    assert!(core.tools.get("lsp_query_rust").is_none());
    assert!(
        core.tools
            .names()
            .iter()
            .all(|name| !name.starts_with("lsp_query_"))
    );
}

#[tokio::test]
async fn enabled_tools_snapshot_records_mode_filtered_tools() {
    let mut core = PureCore::default_provider().unwrap();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Plan);
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert_eq!(event.mode, "plan");
    assert!(event.tools.contains(&"bash".to_string()));
    assert!(event.tools.contains(&"read_file".to_string()));
    assert!(event.tools.contains(&"plan_exit".to_string()));
    assert!(!event.tools.contains(&"write_file".to_string()));
    assert!(!event.tools.contains(&"apply_patch".to_string()));
}

#[tokio::test]
async fn enabled_tools_snapshot_includes_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = PureCore::default_provider()
        .unwrap()
        .with_lsp_runtime(registry);
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1", CompileMode::Auto);
    let event = enabled_tools_event(&events);

    // 空注册表没有可用语言，不应出现任何 LSP 工具。
    assert!(event.tools.iter().all(|t| !t.starts_with("lsp_query_")));
    assert!(!event.tools.contains(&"plan_exit".to_string()));
}
