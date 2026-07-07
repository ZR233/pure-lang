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
    assert!(core.tools.get("send_input").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("update_todo_list").is_some());
    assert!(core.tools.get("plan_exit").is_some());
    assert!(core.tools.get("send_message").is_none());
    assert!(core.tools.get("followup_task").is_none());
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
async fn shared_tools_expose_only_canonical_codex_shape_names() {
    let mut core = PureCore::default_provider().unwrap();
    let capabilities = crate::config::ToolCapabilityConfig {
        container: true,
        git: true,
        ..Default::default()
    };

    ToolSetBuilder::from_capabilities(capabilities)
        .with_container_tools(std::sync::Arc::new(FakeContainerBackend))
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    let names = core.tools.names();
    for canonical in [
        "send_input",
        "git_workspace_info",
        "container_copy",
        "read_file",
        "list_files",
        "search_files",
        "apply_patch",
        "request_user_input",
        "update_todo_list",
    ] {
        assert!(
            names.contains(&canonical),
            "missing canonical tool `{canonical}` in {names:?}"
        );
    }
    for removed in [
        "send_message",
        "followup_task",
        "git_worktree_info",
        "github_api_get",
        "container_cp_upload",
        "container_cp_download",
    ] {
        assert!(
            !names.contains(&removed),
            "removed tool `{removed}` is still exposed in {names:?}"
        );
    }
}

#[test]
fn workspace_file_schemas_use_codex_camel_case_fields() {
    let read_schema = crate::tool::WorkspaceFileToolKind::ReadFile.input_schema();
    assert!(read_schema.pointer("/properties/lineStart").is_some());
    assert!(read_schema.pointer("/properties/lineCount").is_some());
    assert!(read_schema.pointer("/properties/maxBytes").is_some());
    assert!(read_schema.pointer("/properties/line_start").is_none());
    assert!(read_schema.pointer("/properties/line_count").is_none());
    assert!(read_schema.pointer("/properties/max_bytes").is_none());

    let list_schema = crate::tool::WorkspaceFileToolKind::ListFiles.input_schema();
    assert!(list_schema.pointer("/properties/maxFiles").is_some());
    assert!(list_schema.pointer("/properties/includeDirs").is_some());
    assert!(list_schema.pointer("/properties/max_files").is_none());
    assert!(list_schema.pointer("/properties/include_dirs").is_none());

    let search_schema = crate::tool::WorkspaceFileToolKind::SearchFiles.input_schema();
    assert!(search_schema.pointer("/properties/caseSensitive").is_some());
    assert!(search_schema.pointer("/properties/maxMatches").is_some());
    assert!(search_schema.pointer("/properties/contextLines").is_some());
    assert!(
        search_schema
            .pointer("/properties/case_sensitive")
            .is_none()
    );
    assert!(search_schema.pointer("/properties/max_matches").is_none());
    assert!(search_schema.pointer("/properties/context_lines").is_none());
}

#[test]
fn agent_control_schemas_use_codex_camel_case_fields() {
    let spawn_schema = crate::tool::AgentControlToolKind::SpawnAgent.input_schema();
    assert!(spawn_schema.pointer("/properties/taskName").is_some());
    assert!(spawn_schema.pointer("/properties/agentType").is_some());
    assert!(
        spawn_schema
            .pointer("/properties/reasoningEffort")
            .is_some()
    );
    assert!(spawn_schema.pointer("/properties/forkTurns").is_some());
    assert!(spawn_schema.pointer("/properties/name").is_none());
    assert!(spawn_schema.pointer("/properties/agent_type").is_none());
    assert!(
        spawn_schema
            .pointer("/properties/reasoning_effort")
            .is_none()
    );

    let wait_schema = crate::tool::AgentControlToolKind::WaitAgent.input_schema();
    assert!(wait_schema.pointer("/properties/targets").is_some());
    assert!(wait_schema.pointer("/properties/timeoutMs").is_some());
    assert!(wait_schema.pointer("/properties/timeout_ms").is_none());
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
async fn tool_set_builder_registers_git_only_with_runtime_config() {
    let capabilities = crate::config::ToolCapabilityConfig {
        git: true,
        ..Default::default()
    };
    let mut core = PureCore::default_provider().unwrap();

    ToolSetBuilder::from_capabilities(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("git_status").is_none());

    let mut core = PureCore::default_provider().unwrap();
    ToolSetBuilder::from_capabilities(capabilities)
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("git_status").is_some());
    assert!(core.tools.get("git_push").is_some());
}

#[derive(Debug, Clone, Default)]
struct FakeContainerBackend;

impl crate::tool::ContainerBackend for FakeContainerBackend {
    async fn exec(
        &self,
        _request: crate::tool::ContainerExecRequest,
    ) -> crate::Result<crate::tool::ContainerExecOutput> {
        Ok(crate::tool::ContainerExecOutput {
            status: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_bytes: 0,
            stderr_bytes: 0,
            output_artifacts: Vec::new(),
        })
    }

    async fn copy_from(
        &self,
        _request: crate::tool::ContainerCopyFromRequest,
    ) -> crate::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn copy_to(&self, _request: crate::tool::ContainerCopyToRequest) -> crate::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn tool_set_builder_registers_container_only_with_backend() {
    let capabilities = crate::config::ToolCapabilityConfig {
        container: true,
        ..Default::default()
    };
    let mut core = PureCore::default_provider().unwrap();

    ToolSetBuilder::from_capabilities(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("container_exec").is_none());
    assert!(core.tools.get("container_copy").is_none());

    let mut core = PureCore::default_provider().unwrap();
    ToolSetBuilder::from_capabilities(capabilities)
        .with_container_tools(std::sync::Arc::new(FakeContainerBackend))
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("container_exec").is_some());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("list_files").is_some());
    assert!(core.tools.get("search_files").is_some());
    assert!(core.tools.get("apply_patch").is_some());
    assert!(core.tools.get("container_copy").is_some());
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
