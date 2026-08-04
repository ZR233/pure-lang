use super::*;
use crate::ToolEffect;
use pretty_assertions::assert_eq;

#[test]
fn shared_tool_schemas_describe_host_independent_workspace_surface() {
    let names = shared_tool_schemas(SharedToolSchemaOptions {
        exec: true,
        workspace_files: true,
        ask_user: true,
        git: true,
        mcp_resources: false,
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
            "search_files",
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
fn session_note_tools_are_available_to_read_only_plan_policy() {
    for name in [
        "read_session_note",
        "search_session_note",
        "write_session_note",
        "apply_session_note_patch",
    ] {
        assert_eq!(ToolEffect::for_builtin_name(name), Some(ToolEffect::Read));
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
fn shared_tool_schemas_can_include_mcp_resource_tools() {
    let names = shared_tool_schemas(SharedToolSchemaOptions {
        mcp_resources: true,
        ..Default::default()
    })
    .into_iter()
    .map(|schema| schema.name().to_string())
    .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "read_session_note",
            "search_session_note",
            "write_session_note",
            "apply_session_note_patch",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
        ]
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
    let mut core = TurnEngine::default_provider().unwrap();

    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    assert!(core.tools.get("exec").is_some());
    assert!(core.tools.get("write_stdin").is_some());
    assert!(core.tools.get("spawn_agent").is_none());
    assert!(core.tools.get("list_agents").is_none());
    assert!(core.tools.get("send_input").is_none());
    assert!(core.tools.get("close_agent").is_none());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("update_todo_list").is_some());
    assert!(core.tools.get("read_session_note").is_some());
    assert!(core.tools.get("search_session_note").is_some());
    assert!(core.tools.get("write_session_note").is_some());
    assert!(core.tools.get("apply_session_note_patch").is_some());
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
async fn default_tool_builder_exposes_only_framework_independent_names() {
    let mut core = TurnEngine::default_provider().unwrap();
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

    let names = core.tools.names();
    for canonical in [
        "git_workspace_info",
        "exec",
        "write_stdin",
        "read_file",
        "list_files",
        "search_files",
        "apply_patch",
        "request_user_input",
        "update_todo_list",
        "read_session_note",
        "search_session_note",
        "write_session_note",
        "apply_session_note_patch",
    ] {
        assert!(
            names.contains(&canonical),
            "missing canonical tool `{canonical}` in {names:?}"
        );
    }
}

#[test]
fn workspace_file_schemas_use_codex_camel_case_fields() {
    let read_schema = crate::tool::WorkspaceFileToolKind::ReadFile.input_schema();
    assert!(read_schema.pointer("/properties/startLine").is_some());
    assert!(read_schema.pointer("/properties/maxLines").is_some());
    assert!(read_schema.pointer("/properties/maxBytes").is_none());
    assert!(read_schema.pointer("/properties/offset").is_none());
    assert!(read_schema.pointer("/properties/line_start").is_none());
    assert!(read_schema.pointer("/properties/line_count").is_none());
    assert!(read_schema.pointer("/properties/max_bytes").is_none());

    let list_schema = crate::tool::WorkspaceFileToolKind::ListFiles.input_schema();
    assert!(list_schema.pointer("/properties/limit").is_some());
    assert!(list_schema.pointer("/properties/cursor").is_some());
    assert!(list_schema.pointer("/properties/includeDirs").is_some());
    assert!(list_schema.pointer("/properties/max_files").is_none());
    assert!(list_schema.pointer("/properties/include_dirs").is_none());

    let search_schema = crate::tool::WorkspaceFileToolKind::SearchFiles.input_schema();
    assert!(search_schema.pointer("/properties/caseSensitive").is_some());
    assert!(search_schema.pointer("/properties/limit").is_some());
    assert!(search_schema.pointer("/properties/cursor").is_some());
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
        Some(crate::tool::WorkspaceFileToolKind::SearchFiles)
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
        crate::tool::WorkspaceFileToolKind::from_name("search.files"),
        None
    );
    assert_eq!(
        crate::tool::WorkspaceFileToolKind::from_name("apply.patch"),
        None
    );
}

#[test]
fn git_schemas_use_codex_camel_case_fields() {
    let branch_schema = crate::tool::GitToolKind::Branch.input_schema();
    assert!(branch_schema.pointer("/properties/startPoint").is_some());
    assert!(branch_schema.pointer("/properties/start_point").is_none());

    let push_schema = crate::tool::GitToolKind::Push.input_schema();
    assert!(push_schema.pointer("/properties/setUpstream").is_some());
    assert!(push_schema.pointer("/properties/set_upstream").is_none());

    let sync_schema = crate::tool::GitToolKind::SyncDefaultBranch.input_schema();
    assert!(sync_schema.pointer("/properties/preserveChanges").is_some());
    assert!(
        sync_schema
            .pointer("/properties/preserve_changes")
            .is_none()
    );
}

#[tokio::test]
async fn tool_set_builder_can_disable_exec() {
    let mut core = TurnEngine::default_provider().unwrap();
    let capabilities = crate::config::ToolCapabilityConfig {
        exec: false,
        ..Default::default()
    };

    core.register_tools_with_capabilities(std::env::temp_dir(), None, capabilities)
        .await;

    assert!(core.tools.get("exec").is_none());
    assert!(core.tools.get("write_stdin").is_none());
    assert!(core.tools.get("spawn_agent").is_none());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("plan_exit").is_some());
}

#[test]
fn register_git_tools_exposes_git_pack_explicitly() {
    let mut core = TurnEngine::default_provider().unwrap();

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
    assert!(core.tools.get("git_sync_default_branch").is_some());
}

#[tokio::test]
async fn tool_set_builder_registers_git_only_with_runtime_config() {
    let capabilities = crate::config::ToolCapabilityConfig {
        git: true,
        ..Default::default()
    };
    let mut core = TurnEngine::default_provider().unwrap();

    ToolSetBuilder::from_capabilities(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("git_status").is_none());

    let mut core = TurnEngine::default_provider().unwrap();
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
    let mut core = TurnEngine::default_provider().unwrap();

    ToolSetBuilder::host_provided(capabilities.clone())
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(!schema_names_without_backend.contains(&"exec".to_string()));
    assert!(!schema_names_without_backend.contains(&"read_file".to_string()));
    assert!(!schema_names_without_backend.contains(&"list_files".to_string()));
    assert!(!schema_names_without_backend.contains(&"search_files".to_string()));
    assert!(!schema_names_without_backend.contains(&"apply_patch".to_string()));
    assert!(core.tools.get("exec").is_none());
    assert!(core.tools.get("write_stdin").is_none());
    assert!(core.tools.get("read_file").is_none());
    assert!(core.tools.get("list_files").is_none());
    assert!(core.tools.get("search_files").is_none());
    assert!(core.tools.get("apply_patch").is_none());

    let mut core = TurnEngine::default_provider().unwrap();
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

    assert!(core.tools.get("exec").is_some());
    assert!(core.tools.get("write_stdin").is_some());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("list_files").is_some());
    assert!(core.tools.get("search_files").is_some());
    assert!(core.tools.get("apply_patch").is_some());
}

#[derive(Debug, Clone, Default)]
struct FakeMcpResourceBackend;

impl crate::tool::McpResourceBackend for FakeMcpResourceBackend {
    type Error = crate::PureError;

    async fn list_resources(
        &self,
        _request: crate::tool::McpListResourcesRequest,
    ) -> std::result::Result<serde_json::Value, Self::Error> {
        Ok(serde_json::json!({ "resources": [] }))
    }

    async fn list_resource_templates(
        &self,
        _request: crate::tool::McpListResourceTemplatesRequest,
    ) -> std::result::Result<serde_json::Value, Self::Error> {
        Ok(serde_json::json!({ "resourceTemplates": [] }))
    }

    async fn read_resource(
        &self,
        request: crate::tool::McpReadResourceRequest,
    ) -> std::result::Result<serde_json::Value, Self::Error> {
        Ok(serde_json::json!({
            "server": request.server,
            "uri": request.uri,
        }))
    }
}

#[derive(Debug)]
struct FakeMcpToolBackend;

impl crate::tool::McpToolBackend for FakeMcpToolBackend {
    type Error = crate::PureError;

    async fn call_tool(
        &self,
        request: crate::tool::McpToolRequest,
    ) -> std::result::Result<serde_json::Value, Self::Error> {
        Ok(serde_json::json!({
            "tool": request.name,
            "arguments": request.arguments,
        }))
    }
}

#[tokio::test]
async fn tool_set_builder_registers_mcp_resource_backend() {
    let capabilities = crate::config::ToolCapabilityConfig {
        mcp: true,
        ..Default::default()
    };
    let mut core = TurnEngine::default_provider().unwrap();

    ToolSetBuilder::from_capabilities(capabilities.clone())
        .with_allowed_tools(["list_mcp_resources", "read_mcp_resource"])
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("list_mcp_resources").is_none());
    assert!(core.tools.get("read_mcp_resource").is_none());

    let mut core = TurnEngine::default_provider().unwrap();
    ToolSetBuilder::from_capabilities(capabilities)
        .with_allowed_tools(["list_mcp_resources", "read_mcp_resource"])
        .with_mcp_resource_tools(std::sync::Arc::new(FakeMcpResourceBackend))
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    assert!(core.tools.get("list_mcp_resources").is_some());
    assert!(core.tools.get("list_mcp_resource_templates").is_none());
    assert!(core.tools.get("read_mcp_resource").is_some());
}

#[tokio::test]
async fn tool_set_builder_registers_host_mcp_tools() {
    let capabilities = crate::config::ToolCapabilityConfig {
        mcp: true,
        ..Default::default()
    };
    let mut core = TurnEngine::default_provider().unwrap();
    let schema = pl_model::ToolSchema::function(
        "mcp__docs__lookup",
        "Lookup docs.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        }),
    );

    ToolSetBuilder::from_capabilities(capabilities)
        .with_allowed_tools(["mcp__docs__lookup"])
        .with_mcp_tools(
            vec![schema.clone()],
            std::sync::Arc::new(FakeMcpToolBackend),
        )
        .register(&mut core, std::env::temp_dir(), None)
        .await;

    let schemas = core.tools.schemas();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name(), schema.name());
    assert_eq!(schemas[0].description(), schema.description());
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let output = core
        .tools
        .get("mcp__docs__lookup")
        .expect("mcp tool")
        .execute(
            crate::tool::ToolInput {
                arguments: serde_json::json!({ "query": "turn engine" }),
                session_id: "session_mcp".to_string(),
                tool_id: "call_mcp".to_string(),
                revision_base: 0,
            },
            test_tool_context(event_tx),
        )
        .await
        .expect("execute mcp tool");

    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output.description).expect("json output"),
        serde_json::json!({
            "tool": "mcp__docs__lookup",
            "arguments": { "query": "turn engine" },
        })
    );

    let workspace_root = std::env::temp_dir();
    let mut core = TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
        .unwrap()
        .with_runtime_profile(CoreRuntimeProfile::host_provided(workspace_root.clone()))
        .build();
    ToolSetBuilder::from_capabilities(crate::config::ToolCapabilityConfig {
        mcp: true,
        ..Default::default()
    })
    .with_allowed_tools(["mcp__docs__lookup"])
    .with_mcp_tools(vec![schema], std::sync::Arc::new(FakeMcpToolBackend))
    .register(&mut core, workspace_root, None)
    .await;

    assert!(core.has_tool("mcp__docs__lookup"));
}

#[tokio::test]
async fn tool_set_builder_respects_allowed_tools() {
    let capabilities = crate::config::ToolCapabilityConfig::hosted_workspace();
    let mut core = TurnEngine::default_provider().unwrap();

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
    assert!(core.tools.get("exec").is_some());
    assert!(core.tools.get("read_file").is_some());
    assert!(core.tools.get("git_status").is_some());
    assert!(core.tools.get("request_user_input").is_some());
    assert!(core.tools.get("update_todo_list").is_some());

    assert!(core.tools.get("list_files").is_none());
    assert!(core.tools.get("git_push").is_none());
    assert!(core.tools.get("plan_exit").is_none());
}

#[tokio::test]
async fn profiled_local_workspace_uses_unified_workspace_file_tools() {
    let runtime = CoreRuntimeProfile::local_workspace(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
        .unwrap()
        .with_runtime_profile(runtime)
        .build();

    core.register_profile_tools().await;

    let read_tool = core.tools.get("read_file").expect("read_file tool");
    let patch_tool = core.tools.get("apply_patch").expect("apply_patch tool");
    assert!(format!("{read_tool:?}").contains("LocalWorkspaceFileTool"));
    assert!(format!("{patch_tool:?}").contains("LocalWorkspaceFileTool"));
}

#[tokio::test]
async fn profiled_host_tools_do_not_register_local_workspace_tools() {
    let runtime = CoreRuntimeProfile::host_provided(std::env::temp_dir())
        .with_workspace_instructions("rules");
    let mut core = TurnEngineBuilder::from_provider_info(pl_model::ProviderInfo::deepseek(None))
        .unwrap()
        .with_runtime_profile(runtime)
        .build();

    core.register_profile_tools().await;

    assert!(core.tools.is_empty());
}

#[tokio::test]
async fn default_tools_register_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = TurnEngine::default_provider()
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
async fn enabled_tools_snapshot_records_registered_tools() {
    let mut core = TurnEngine::default_provider().unwrap();
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
    let mut core = TurnEngine::default_provider()
        .unwrap()
        .with_lsp_runtime(registry);
    core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await;

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
    let event = enabled_tools_event(&events);

    // 空注册表没有可用语言，不应出现任何 LSP 工具。
    assert!(event.tools.iter().all(|t| !t.starts_with("lsp_query_")));
    assert!(event.tools.contains(&"plan_exit".to_string()));
}
