use super::*;
use crate::ToolEffect;
use pretty_assertions::assert_eq;

fn has_tool(core: &TurnEngine, name: &str) -> bool {
    core.tool_names().iter().any(|tool| tool == name)
}

#[test]
fn shared_tool_schemas_describe_host_independent_workspace_surface() {
    let names = shared_tool_schemas(SharedToolSchemaOptions {
        exec: true,
        workspace_files: true,
        ask_user: true,
        git: true,
        todo: true,
        plan_exit: false,
    })
    .into_iter()
    .map(|schema| schema.name().to_string())
    .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "exec",
            "write_stdin",
            "read_file",
            "list_files",
            "apply_patch",
            "request_user_input",
            "update_todo_list",
            "read_session_note",
            "search_session_note",
            "write_session_note",
            "apply_session_note_patch",
            "git_status",
            "git_diff",
            "git_branch",
            "git_fetch",
            "git_commit",
            "git_push",
            "git_workspace_info",
            "git_sync_default_branch",
        ]
    );
}

#[test]
fn session_note_tools_declare_read_effect_for_plan_policy() {
    use crate::tool::{SessionNoteTool, SessionNoteToolKind, Tool};
    for kind in SessionNoteToolKind::all() {
        assert_eq!(
            SessionNoteTool::new(*kind).effect(),
            Some(ToolEffect::Read),
            "{}",
            kind.name()
        );
    }
}

#[test]
fn shared_tool_schema_options_can_disable_plan_exit_fluently() {
    let options = SharedToolSchemaOptions::from_capabilities(
        &crate::config::ToolCapabilityConfig::hosted_workspace(),
    )
    .with_plan_exit(false);
    let names = shared_tool_names(options);

    assert!(names.contains(&"exec".to_string()));
    assert!(names.contains(&"git_status".to_string()));
    assert!(!names.contains(&"plan_exit".to_string()));
}

#[test]
fn tool_visibility_set_combines_shared_product_and_dynamic_tools() {
    let visibility = ToolVisibilitySet::from_tool_names(["read_file", "spawn_agent"])
        .with_tool_names(["github_api_request", "mcp__docs__lookup"]);

    assert!(visibility.contains("read_file"));
    assert!(visibility.contains("spawn_agent"));
    assert!(visibility.contains("github_api_request"));
    assert!(visibility.contains("mcp__docs__lookup"));
    assert!(!visibility.contains("git_status"));
    assert_eq!(visibility.len(), visibility.to_btree_set().len());

    let schemas = visibility.filter_schemas([
        pl_model::ToolSchema::function("github_api_request", "GitHub", serde_json::json!({})),
        pl_model::ToolSchema::function("hidden_product_tool", "Hidden", serde_json::json!({})),
    ]);

    assert_eq!(
        schemas
            .into_iter()
            .map(|schema| schema.name().to_string())
            .collect::<Vec<_>>(),
        vec!["github_api_request".to_string()]
    );
}

#[test]
fn shared_tool_schemas_keep_exec_and_git_opt_in() {
    let names = shared_tool_schemas(SharedToolSchemaOptions {
        workspace_files: true,
        ask_user: true,
        todo: true,
        ..Default::default()
    })
    .into_iter()
    .map(|schema| schema.name().to_string())
    .collect::<Vec<_>>();

    assert!(names.contains(&"read_file".to_string()));
    assert!(names.contains(&"request_user_input".to_string()));
    assert!(names.contains(&"update_todo_list".to_string()));
    assert!(!names.contains(&"git_status".to_string()));
    assert!(!names.contains(&"exec".to_string()));
    assert!(!names.contains(&"spawn_agent".to_string()));
}

#[tokio::test]
async fn default_tools_register_shared_tools_without_product_collaboration() {
    let mut core = test_turn_engine();

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    assert!(has_tool(&core, "exec"));
    assert!(has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "spawn_agent"));
    assert!(!has_tool(&core, "list_agents"));
    assert!(!has_tool(&core, "send_input"));
    assert!(!has_tool(&core, "close_agent"));
    assert!(has_tool(&core, "request_user_input"));
    assert!(has_tool(&core, "update_todo_list"));
    assert!(has_tool(&core, "read_session_note"));
    assert!(has_tool(&core, "search_session_note"));
    assert!(has_tool(&core, "write_session_note"));
    assert!(has_tool(&core, "apply_session_note_patch"));
    assert!(has_tool(&core, "plan_exit"));
    assert!(!has_tool(&core, "send_message"));
    assert!(!has_tool(&core, "followup_task"));
    assert!(!has_tool(&core, "subagent"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "apply_patch"));
    assert!(!has_tool(&core, "lsp_query"));
    assert!(!has_tool(&core, "git_status"));
    assert!(!has_tool(&core, "git_push"));
    assert!(!has_tool(&core, "docker"));
    assert!(!has_tool(&core, "container"));
}

#[tokio::test]
async fn default_tool_builder_exposes_only_framework_independent_names() {
    let mut core = test_turn_engine();
    let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
    let workspace_root = std::env::temp_dir();

    ToolSetBuilder::host_provided(capabilities)
        .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
            workspace_root.clone(),
        )))
        .with_workspace_file_backend(std::sync::Arc::new(
            crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                FakeContainerBackend,
            )),
        ))
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .register(&mut core, workspace_root, None)
        .await;

    let names = core.tool_names();
    for canonical in [
        "git_workspace_info",
        "exec",
        "write_stdin",
        "read_file",
        "list_files",
        "apply_patch",
        "request_user_input",
        "update_todo_list",
        "read_session_note",
        "search_session_note",
        "write_session_note",
        "apply_session_note_patch",
    ] {
        assert!(
            names.contains(&canonical.to_string()),
            "missing canonical tool `{canonical}` in {names:?}"
        );
    }
}

#[test]
fn workspace_file_tool_kind_rejects_dot_aliases() {
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("read_file"),
        Some(crate::tool::WorkspaceFileToolKind::ReadFile)
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("list_files"),
        Some(crate::tool::WorkspaceFileToolKind::ListFiles)
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("search_files"),
        None
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("apply_patch"),
        Some(crate::tool::WorkspaceFileToolKind::ApplyPatch)
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("read.file"),
        None
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("list.files"),
        None
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("apply.patch"),
        None
    );
}

#[tokio::test]
async fn tool_set_builder_can_disable_exec() {
    let mut core = test_turn_engine();
    let capabilities = crate::config::ToolCapabilityConfig {
        exec: false,
        ..Default::default()
    };

    core.register_tools_with_capabilities(std::env::temp_dir(), None, capabilities)
        .await;

    assert!(!has_tool(&core, "exec"));
    assert!(!has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "spawn_agent"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "request_user_input"));
    assert!(has_tool(&core, "plan_exit"));
}

#[test]
fn register_git_tools_exposes_git_pack_explicitly() {
    let mut core = test_turn_engine();

    core.register_git_tools(
        crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
        std::sync::Arc::new(crate::tool::LocalExecutionBackend),
        std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
    );

    assert!(has_tool(&core, "git_status"));
    assert!(has_tool(&core, "git_diff"));
    assert!(has_tool(&core, "git_branch"));
    assert!(has_tool(&core, "git_fetch"));
    assert!(has_tool(&core, "git_commit"));
    assert!(has_tool(&core, "git_push"));
    assert!(has_tool(&core, "git_workspace_info"));
    assert!(has_tool(&core, "git_sync_default_branch"));
}

#[tokio::test]
async fn tool_set_builder_registers_git_only_with_runtime_config() {
    let capabilities = crate::config::ToolCapabilityConfig {
        git: true,
        ..Default::default()
    };
    let mut core = test_turn_engine();

    ToolSetBuilder::from_capabilities(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(!has_tool(&core, "git_status"));

    let mut core = test_turn_engine();
    ToolSetBuilder::from_capabilities(capabilities)
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(has_tool(&core, "git_status"));
    assert!(has_tool(&core, "git_push"));
}

#[derive(Debug, Clone, Default)]
struct FakeContainerBackend;

impl crate::tool::ContainerBackend for FakeContainerBackend {
    type Error = String;

    async fn exec(
        &self,
        _request: crate::tool::ContainerExecRequest,
    ) -> std::result::Result<crate::tool::ContainerExecOutput, Self::Error> {
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
    ) -> std::result::Result<Vec<u8>, Self::Error> {
        Ok(Vec::new())
    }

    async fn copy_to(
        &self,
        _request: crate::tool::ContainerCopyToRequest,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn host_provided_tool_set_requires_explicit_workspace_backends() {
    let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
    let schema_names_without_backend =
        ToolSetBuilder::host_provided(capabilities.clone()).shared_tool_names();
    let mut core = test_turn_engine();

    ToolSetBuilder::host_provided(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(!schema_names_without_backend.contains(&"exec".to_string()));
    assert!(!schema_names_without_backend.contains(&"read_file".to_string()));
    assert!(!schema_names_without_backend.contains(&"list_files".to_string()));
    assert!(!schema_names_without_backend.contains(&"apply_patch".to_string()));
    assert!(!has_tool(&core, "exec"));
    assert!(!has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "read_file"));
    assert!(!has_tool(&core, "list_files"));
    assert!(!has_tool(&core, "apply_patch"));

    let mut core = test_turn_engine();
    ToolSetBuilder::host_provided(capabilities)
        .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
            std::env::temp_dir(),
        )))
        .with_workspace_file_backend(std::sync::Arc::new(
            crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                FakeContainerBackend,
            )),
        ))
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(has_tool(&core, "exec"));
    assert!(has_tool(&core, "write_stdin"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "list_files"));
    assert!(!has_tool(&core, "search_files"));
    assert!(has_tool(&core, "apply_patch"));
}

#[tokio::test]
async fn tool_set_builder_respects_allowed_tools() {
    let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
    let mut core = test_turn_engine();

    let builder = ToolSetBuilder::host_provided(capabilities)
        .with_allowed_tools([
            "exec",
            "read_file",
            "git_status",
            "request_user_input",
            "update_todo_list",
        ])
        .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
            std::env::temp_dir(),
        )))
        .with_workspace_file_backend(std::sync::Arc::new(
            crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                FakeContainerBackend,
            )),
        ))
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        );
    let schema_names = builder
        .shared_tool_schemas()
        .into_iter()
        .map(|schema| schema.name().to_string())
        .collect::<Vec<_>>();

    builder
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert_eq!(
        schema_names,
        vec![
            "exec",
            "read_file",
            "request_user_input",
            "update_todo_list",
            "git_status",
        ]
    );
    assert!(has_tool(&core, "exec"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "git_status"));
    assert!(has_tool(&core, "request_user_input"));
    assert!(has_tool(&core, "update_todo_list"));

    assert!(!has_tool(&core, "list_files"));
    assert!(!has_tool(&core, "git_push"));
    assert!(!has_tool(&core, "plan_exit"));
}

#[tokio::test]
async fn profiled_local_workspace_uses_unified_workspace_file_tools() {
    let runtime = CoreRuntimeProfile::local_workspace(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = test_turn_engine_builder(
        pl_model::ProviderEndpoint::deepseek(None),
        pl_model::ModelInfo::fallback("deepseek-v4-flash"),
    )
    .with_runtime_profile(runtime)
    .build();

    core.register_profile_tools().await;

    let lease = core.acquire_tool_lease().unwrap();
    let read_tool = lease.entry("read_file").expect("read_file tool");
    let patch_tool = lease.entry("apply_patch").expect("apply_patch tool");
    assert!(format!("{:?}", read_tool.tool()).contains("LocalWorkspaceFileTool"));
    assert!(format!("{:?}", patch_tool.tool()).contains("LocalWorkspaceFileTool"));
}

#[tokio::test]
async fn profiled_host_tools_do_not_register_local_workspace_tools() {
    let runtime = CoreRuntimeProfile::host_provided(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = test_turn_engine_builder(
        pl_model::ProviderEndpoint::deepseek(None),
        pl_model::ModelInfo::fallback("deepseek-v4-flash"),
    )
    .with_runtime_profile(runtime)
    .build();

    core.register_profile_tools().await;

    assert!(core.tool_names().is_empty());
}

#[tokio::test]
async fn default_tools_register_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = test_turn_engine().with_lsp_runtime(registry.clone());

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    // 空注册表没有可用语言，不应注册任何按语言命名的 LSP 工具。
    assert!(
        core.tool_names()
            .iter()
            .all(|name| !name.starts_with("lsp_query_"))
    );
}

#[tokio::test]
async fn enabled_tools_snapshot_records_registered_tools() {
    let mut core = test_turn_engine();
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"exec".to_string()));
    assert!(event.tools.contains(&"read_file".to_string()));
    assert!(event.tools.contains(&"plan_exit".to_string()));
    assert!(event.tools.contains(&"write_file".to_string()));
    assert!(event.tools.contains(&"apply_patch".to_string()));
}

#[tokio::test]
async fn enabled_tools_snapshot_includes_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = test_turn_engine().with_lsp_runtime(registry);
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
    let event = enabled_tools_event(&events);

    // 空注册表没有可用语言，不应出现任何 LSP 工具。
    assert!(event.tools.iter().all(|t| !t.starts_with("lsp_query_")));
    assert!(event.tools.contains(&"plan_exit".to_string()));
}
