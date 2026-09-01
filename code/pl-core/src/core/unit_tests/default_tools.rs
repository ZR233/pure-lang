use super::*;
use crate::ToolEffect;
use pretty_assertions::assert_eq;

fn has_tool(core: &TurnEngine, name: &str) -> bool {
    core.tool_names().iter().any(|tool| tool == name)
}

#[test]
fn session_note_tools_declare_read_effect_for_plan_policy() {
    use crate::tool::{SessionNoteTool, SessionNoteToolKind, Tool};
    for kind in SessionNoteToolKind::all() {
        assert_eq!(
            SessionNoteTool::new(*kind, crate::TurnWorkingSetHandle::default()).effect(),
            Some(ToolEffect::Read),
            "{}",
            kind.name()
        );
    }
}

#[tokio::test]
async fn default_tools_register_shared_tools_without_product_collaboration() {
    let mut core = test_turn_engine();

    core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await
        .expect("install default tools");

    assert!(has_tool(&core, "exec"));
    assert!(has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "spawn_agent"));
    assert!(!has_tool(&core, "list_agents"));
    assert!(!has_tool(&core, "send_input"));
    assert!(!has_tool(&core, "close_agent"));
    assert!(has_tool(&core, "request_user_input"));
    assert!(has_tool(&core, "submit_plan"));
    assert!(has_tool(&core, "update_todo_list"));
    assert!(has_tool(&core, "read_session_note"));
    assert!(has_tool(&core, "search_session_note"));
    assert!(has_tool(&core, "write_session_note"));
    assert!(has_tool(&core, "apply_session_note_patch"));
    assert!(!has_tool(&core, "plan_exit"));
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

    BuiltinToolInstaller::host_provided(capabilities)
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
        .install(&mut core, workspace_root, None)
        .await
        .expect("install host tools");

    let names = core.tool_names();
    for canonical in [
        "git_workspace_info",
        "exec",
        "write_stdin",
        "read_file",
        "list_files",
        "apply_patch",
        "request_user_input",
        "submit_plan",
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
async fn builtin_tool_installer_can_disable_exec() {
    let mut core = test_turn_engine();
    let capabilities = crate::config::ToolCapabilityConfig {
        exec: false,
        ..Default::default()
    };

    core.install_tools_with_capabilities(std::env::temp_dir(), None, capabilities)
        .await
        .expect("install selected tools");

    assert!(!has_tool(&core, "exec"));
    assert!(!has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "spawn_agent"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "request_user_input"));
    assert!(has_tool(&core, "submit_plan"));
    assert!(!has_tool(&core, "plan_exit"));
}

#[test]
fn register_git_tools_exposes_git_pack_explicitly() {
    let mut core = test_turn_engine();

    core.install_git_tools(
        crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
        std::sync::Arc::new(crate::tool::LocalExecutionBackend),
        std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
    )
    .expect("install git tools");

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
async fn builtin_tool_installer_registers_git_only_with_runtime_config() {
    let capabilities = crate::config::ToolCapabilityConfig {
        git: true,
        ..Default::default()
    };
    let mut core = test_turn_engine();

    BuiltinToolInstaller::from_capabilities(capabilities.clone())
        .install(&mut core, std::env::temp_dir(), None)
        .await
        .expect("install without git runtime");

    assert!(!has_tool(&core, "git_status"));

    let mut core = test_turn_engine();
    BuiltinToolInstaller::from_capabilities(capabilities)
        .with_git_tools(
            crate::tool::GitWorkspaceConfig::local(std::env::temp_dir()),
            std::sync::Arc::new(crate::tool::LocalExecutionBackend),
            std::sync::Arc::new(crate::tool::NoGitCredentialProvider),
        )
        .install(&mut core, std::env::temp_dir(), None)
        .await
        .expect("install with git runtime");

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
    let mut core = test_turn_engine();

    BuiltinToolInstaller::host_provided(capabilities.clone())
        .install(&mut core, std::env::temp_dir(), None)
        .await
        .expect("install host tools without backends");

    assert!(!has_tool(&core, "exec"));
    assert!(!has_tool(&core, "write_stdin"));
    assert!(!has_tool(&core, "read_file"));
    assert!(!has_tool(&core, "list_files"));
    assert!(!has_tool(&core, "apply_patch"));

    let mut core = test_turn_engine();
    BuiltinToolInstaller::host_provided(capabilities)
        .with_command_backend(std::sync::Arc::new(crate::tool::LocalCommandBackend::new(
            std::env::temp_dir(),
        )))
        .with_workspace_file_backend(std::sync::Arc::new(
            crate::tool::ContainerWorkspaceFileBackend::new(std::sync::Arc::new(
                FakeContainerBackend,
            )),
        ))
        .install(&mut core, std::env::temp_dir(), None)
        .await
        .expect("install host tools with backends");

    assert!(has_tool(&core, "exec"));
    assert!(has_tool(&core, "write_stdin"));
    assert!(has_tool(&core, "read_file"));
    assert!(has_tool(&core, "list_files"));
    assert!(!has_tool(&core, "search_files"));
    assert!(has_tool(&core, "apply_patch"));
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

    core.install_profile_tools()
        .await
        .expect("install profile tools");

    let lease = core.acquire_tool_plan();
    let read_tool = lease.binding("read_file").expect("read_file tool");
    let patch_tool = lease.binding("apply_patch").expect("apply_patch tool");
    assert!(format!("{:?}", read_tool.tool()).contains("LocalWorkspaceFileTool"));
    assert!(format!("{:?}", patch_tool.tool()).contains("LocalWorkspaceFileTool"));
}

#[tokio::test]
async fn profiled_host_tools_do_not_register_local_workspace_tools() {
    let runtime = CoreRuntimeProfile::minimal()
        .with_agent_workspace(crate::tool::AgentWorkspace::local(std::env::temp_dir()))
        .with_workspace_instructions("rules");
    let mut core = test_turn_engine_builder(
        pl_model::ProviderEndpoint::deepseek(None),
        pl_model::ModelInfo::fallback("deepseek-v4-flash"),
    )
    .with_runtime_profile(runtime)
    .build();

    core.install_profile_tools()
        .await
        .expect("install profile tools");

    assert!(core.tool_names().is_empty());
}

#[tokio::test]
async fn default_tools_register_lsp_query_when_runtime_is_shared() {
    let registry = pl_lsp::LspRuntimeRegistry::new();
    let mut core = test_turn_engine().with_lsp_runtime(registry.clone());

    core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await
        .expect("install default tools");

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
    core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
        .await
        .expect("install default tools");

    let events = record_enabled_tools_for_core(&core, "session-1", "turn-1");
    let event = enabled_tools_event(&events);

    assert_eq!(event.turn_id, "turn-1");
    assert!(event.tools.contains(&"exec".to_string()));
    assert!(event.tools.contains(&"read_file".to_string()));
    assert!(!event.tools.contains(&"plan_exit".to_string()));
    assert!(event.tools.contains(&"write_file".to_string()));
    assert!(event.tools.contains(&"apply_patch".to_string()));
}
