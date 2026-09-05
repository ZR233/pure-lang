//! Pure-Lang 核心逻辑层。
//!
//! 目录页：`engine` 持有 [`TurnEngine`] 单轮编排引擎，`model_turn`/`profile`/
//! `tool_set` 提供请求与装配模型，`turn_loop` 与 `tool_dispatch` 承担模型循环与
//! 工具执行，`permission`/`progress`/`turn_result` 为其支撑边界。

mod engine;
mod model_turn;
mod permission;
mod profile;
pub(crate) mod progress;
mod tool_dispatch;
mod tool_set;
mod turn_loop;
mod turn_result;

pub use engine::TurnEngine;
pub(crate) use engine::generate_turn_id;
pub use model_turn::*;
pub use profile::*;
pub use tool_set::*;

// 测试经由 `super::*` 消费这些入口。
#[cfg(test)]
use crate::tool::{LocalWorkspaceFileTool, WorkspaceFileToolKind, WriteFileTool};
#[cfg(test)]
use crate::turn::BudgetTracker;
#[cfg(test)]
use permission::approval_request;
#[cfg(test)]
use tool_dispatch::{ToolExecutionContext, execute_tool_calls};

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use pretty_assertions::assert_eq;

    use crate::tool::SubagentContext;
    use crate::trace::TraceRecorder;
    use crate::turn::TurnOptions;

    use pl_model::completion::ToolCall;
    use pl_protocol::{
        InteractionContent, InteractionResolution, ToolApprovalResolution,
        ToolApprovalResolutionPayload,
    };
    use pl_trace::TraceEventKind;

    use super::test_support::*;
    use super::tool_dispatch::ToolExecutionOutcome;
    use super::*;
    use crate::ToolEffect;
    use crate::turn::PermissionMode;

    #[tokio::test]
    async fn enabled_tools_snapshot_remains_internal_trace_event() {
        let mut core = test_turn_engine();
        core.install_default_tools(std::env::temp_dir(), Some("rules".to_string()))
            .await
            .expect("install default tools");
        let tool_plan = core.acquire_tool_plan();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = crate::trace::TraceRecorder::new("session-1".to_string(), event_tx, 0);

        super::turn_loop::enabled_tools::record_enabled_tools(
            &mut recorder,
            "turn-1",
            0,
            &tool_plan,
        );
        let events = recorder.drain();
        let event = events
            .iter()
            .find_map(|event| match &event.kind {
                pl_trace::TraceEventKind::EnabledToolsRecorded { event } => Some(event),
                _ => None,
            })
            .expect("enabled tools event");

        assert_eq!(event.turn_id, "turn-1");
        assert!(event.tools.contains(&"read_file".to_string()));
    }

    fn test_turn_engine() -> TurnEngine {
        TurnEngineBuilder::from_route(&crate::ResolvedModelRoute {
            pricing_mode: pl_protocol::PricingMode::Catalog,
            role: crate::AgentRoleId::new("test").unwrap(),
            provider_id: crate::ProviderId::new("test").unwrap(),
            endpoint: pl_model::provider::ProviderEndpoint::deepseek(None),
            model: pl_model::model::ModelInfo::compatible("deepseek-v4-flash"),
            effort: None,
        })
        .unwrap()
        .build()
    }

    #[test]
    fn default_turn_options_request_approval_for_workspace_escape() {
        let options = TurnOptions::default();

        assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
        assert!(options.interaction_callback.is_none());
    }

    #[tokio::test]
    async fn request_approval_allows_external_path_after_user_approval() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_root =
            std::env::temp_dir().join(format!("pure-permission-workspace-{unique}"));
        let outside_root = std::env::temp_dir().join(format!("pure-permission-outside-{unique}"));
        tokio::fs::create_dir_all(&workspace_root).await.unwrap();
        tokio::fs::create_dir_all(&outside_root).await.unwrap();
        let outside_file = outside_root.join("note.txt");
        tokio::fs::write(&outside_file, "external ok")
            .await
            .unwrap();
        let mut core = test_turn_engine();
        core.register_test_tool(LocalWorkspaceFileTool::new(
            WorkspaceFileToolKind::ReadFile,
            crate::tool::ToolWorkspace::new(crate::tool::AgentWorkspace::local(
                workspace_root.clone(),
            )),
        ));
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({"path": outside_file.to_string_lossy()}),
            "call-1",
        );
        let seen_interaction = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen_interaction_for_callback = seen_interaction.clone();
        let options = TurnOptions::default().with_interaction_callback(std::sync::Arc::new(
            move |interaction| {
                let seen_interaction = seen_interaction_for_callback.clone();
                async move {
                    match &interaction.content {
                        InteractionContent::ToolApproval(approval) => {
                            assert_eq!(approval.request().name, "read_file")
                        }
                        other => panic!("unexpected payload: {other:?}"),
                    }
                    *seen_interaction.lock().unwrap() = Some(interaction);
                    InteractionResolution::ToolApproval(ToolApprovalResolutionPayload {
                        decision: ToolApprovalResolution::Approved,
                        reason: None,
                    })
                }
                .boxed()
            },
        ));
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace_root.clone()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert!(records[0].result.contains("external ok"));
        assert!(seen_interaction.lock().unwrap().is_some());
        assert!(runtime_progress_texts(&mut event_rx).is_empty());
        let events = recorder.drain();
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-call-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::AwaitingApproval,
                TestToolPhase::Approved,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
        let _ = tokio::fs::remove_dir_all(outside_root).await;
    }

    #[tokio::test]
    async fn workspace_tool_without_approval_skips_approved_trace_phase() {
        let workspace = tempfile::tempdir().unwrap();
        let mut core = test_turn_engine();
        core.register_test_tool(WriteFileTool::new(crate::tool::ToolWorkspace::new(
            crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
        )));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "write_file",
            serde_json::json!({
                "path": "note.txt",
                "content": "direct",
                "mode": "create",
            }),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(workspace.path().to_path_buf()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Succeeded);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Succeeded,
            ]
        );
    }

    #[tokio::test]
    async fn unknown_tool_records_one_terminal_event_and_tool_result() {
        let core = test_turn_engine();
        let tool_call = ToolCall::function(
            "provider-item-1",
            "missing_tool",
            serde_json::json!({"value": 1}),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &TurnOptions::default(),
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            ToolExecutionOutcome::Failed(pl_trace::TraceToolFailureKind::Execution),
        );
        assert_eq!(records[0].id, "provider-item-1");
        assert_eq!(records[0].call_id, "call-1");
        assert!(records[0].result.contains("Unknown tool: missing_tool"));
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![TestToolPhase::Started, TestToolPhase::Failed,]
        );
    }

    #[tokio::test]
    async fn execution_policy_denied_tool_records_one_terminal_event_and_tool_result() {
        let mut core = test_turn_engine();
        let tool_workspace = core.tool_workspace();
        core.register_test_tool(WriteFileTool::new(tool_workspace));
        let tool_call = ToolCall::function(
            "provider-item-1",
            "write_file",
            serde_json::json!({"path": "note.txt", "content": "nope"}),
            "call-1",
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
        let options =
            TurnOptions::default().with_execution_policy(crate::AgentExecutionPolicy::default());

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Denied);
        assert!(
            records[0]
                .result
                .contains("Tool disabled by execution policy: write_file")
        );
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![TestToolPhase::Started, TestToolPhase::Denied,]
        );
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => Some(item),
                TraceEventKind::TracePartFailed { item } => Some(item),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("terminal tool item");
        assert_eq!(
            terminal.tool().and_then(|tool| match tool.state() {
                pl_trace::TraceToolState::Denied(state) => Some(state.reason()),
                pl_trace::TraceToolState::Started(_)
                | pl_trace::TraceToolState::Streaming(_)
                | pl_trace::TraceToolState::AwaitingApproval(_)
                | pl_trace::TraceToolState::Approved(_)
                | pl_trace::TraceToolState::Running(_)
                | pl_trace::TraceToolState::Succeeded(_)
                | pl_trace::TraceToolState::Failed(_)
                | pl_trace::TraceToolState::Cancelled(_) => None,
            }),
            Some("Tool disabled by execution policy: write_file")
        );
    }

    #[tokio::test]
    async fn cancelling_running_tool_records_interrupted_terminal_event() {
        let mut core = test_turn_engine();
        core.register_test_tool(SleepingTool);
        let tool_call = ToolCall::function(
            "provider-item-1",
            "sleeping_tool",
            serde_json::json!({}),
            "call-1",
        );
        let token = tokio_util::sync::CancellationToken::new();
        let options = TurnOptions::default().with_cancellation(token.clone());
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(
            std::time::Duration::from_millis(60_000),
        ));
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            token.cancel();
        });

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                tool_plan: core.acquire_tool_plan(),
                options: &options,
                session_id: "turn-1",
                turn_id: "turn-1",
                step: 0,
                workspace: crate::tool::AgentWorkspace::local(std::env::temp_dir()),
                active_subagent: None,
                tool_cache: crate::tool::cache::TurnToolCacheHandle::default(),
            },
        )
        .await
        .unwrap();
        cancel_task.await.unwrap();
        let events = recorder.drain();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].outcome, ToolExecutionOutcome::Cancelled);
        assert_eq!(records[0].result, "Tool execution interrupted");
        assert_eq!(terminal_tool_event_count(&events), 1);
        assert_eq!(
            tool_statuses(&events, "turn-1-provider-item-1"),
            vec![
                TestToolPhase::Started,
                TestToolPhase::Running,
                TestToolPhase::Cancelled,
            ]
        );
        let terminal = events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::TracePartFailed { item } => Some(item),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .expect("interrupted tool item");
        assert!(matches!(
            terminal.tool().map(pl_trace::TraceToolPart::state),
            Some(pl_trace::TraceToolState::Cancelled(_)),
        ));
    }

    #[test]
    fn approval_request_extracts_working_directory() {
        let call = ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({
                "command": "pwd",
                "cwd": "C:/work"
            }),
            "call-1",
        );

        let request = approval_request(&call, None);

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn approval_request_marks_parent_agent() {
        let call = ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({"command": "pwd"}),
            "call-1",
        );
        let active_subagent = SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            agent_path: None,
            role: "executor".to_string(),
            task: "inspect".to_string(),
            depth: 1,
        };

        let request = approval_request(&call, Some(&active_subagent));

        assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
    }

    fn has_tool(core: &TurnEngine, name: &str) -> bool {
        core.tool_names().iter().any(|tool| tool == name)
    }

    #[test]
    fn session_note_tools_declare_read_effect_for_plan_policy() {
        use crate::tool::{SessionNoteTool, SessionNoteToolKind, StaticTool};
        for kind in SessionNoteToolKind::all() {
            assert_eq!(
                SessionNoteTool::new(*kind, crate::TurnWorkingSetHandle::default())
                    .policy()
                    .effect(),
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
        for name in [
            "plan_current",
            "plan_next",
            "plan_history",
            "plan_submit",
            "plan_restart",
        ] {
            assert!(has_tool(&core, name), "missing Plan tool {name}");
        }
        assert!(!has_tool(&core, "submit_plan"));
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
            "plan_current",
            "plan_next",
            "plan_history",
            "plan_submit",
            "plan_restart",
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
        assert!(has_tool(&core, "plan_current"));
        assert!(has_tool(&core, "plan_submit"));
        assert!(has_tool(&core, "plan_restart"));
        assert!(!has_tool(&core, "submit_plan"));
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
    async fn profiled_local_workspace_installs_workspace_tools_in_the_unified_plan() {
        let runtime = CoreRuntimeProfile::local_workspace(std::env::temp_dir())
            .with_workspace_instructions("rules");
        let mut core = test_turn_engine_builder(
            pl_model::provider::ProviderEndpoint::deepseek(None),
            pl_model::model::ModelInfo::compatible("deepseek-v4-flash"),
        )
        .with_runtime_profile(runtime)
        .build();

        core.install_profile_tools()
            .await
            .expect("install profile tools");

        let lease = core.acquire_tool_plan();
        let read_tool = lease.binding("read_file").expect("read_file tool");
        let patch_tool = lease.binding("apply_patch").expect("apply_patch tool");
        assert_eq!(
            read_tool.tool().definition().spec(),
            &crate::tool::WorkspaceFileToolKind::ReadFile.to_spec()
        );
        assert_eq!(
            patch_tool.tool().definition().spec(),
            &crate::tool::WorkspaceFileToolKind::ApplyPatch.to_spec()
        );
        assert_eq!(
            read_tool.tool().execution(),
            crate::tool::ToolExecution::Local
        );
        assert_eq!(
            patch_tool.tool().execution(),
            crate::tool::ToolExecution::Local
        );
    }

    #[tokio::test]
    async fn profiled_host_tools_do_not_register_local_workspace_tools() {
        let runtime = CoreRuntimeProfile::minimal()
            .with_agent_workspace(crate::tool::AgentWorkspace::local(std::env::temp_dir()))
            .with_workspace_instructions("rules");
        let mut core = test_turn_engine_builder(
            pl_model::provider::ProviderEndpoint::deepseek(None),
            pl_model::model::ModelInfo::compatible("deepseek-v4-flash"),
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
        let registry = pl_lsp::runtime::LspRuntimeRegistry::new();
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
        assert!(event.tools.contains(&"plan_current".to_string()));
        assert!(event.tools.contains(&"plan_submit".to_string()));
        assert!(!event.tools.contains(&"submit_plan".to_string()));
        assert!(!event.tools.contains(&"plan_exit".to_string()));
        assert!(event.tools.contains(&"write_file".to_string()));
        assert!(event.tools.contains(&"apply_patch".to_string()));
    }
}

/// core 测试共享基建：引擎构造器与 trace 断言 helper。
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::TraceRecorder;
    use crate::tool::{StaticTool, ToolCallContext, ToolPolicy, ToolResult};
    use pl_model::model::ModelInfo;
    use pl_model::provider::ProviderEndpoint;
    use pl_trace::{
        AgentEvent, TraceEvent, TraceEventKind, TracePartKind, TracePartSource, TraceTextChannel,
    };

    pub(crate) fn test_static_tool_definition(
        name: &'static str,
        description: &'static str,
    ) -> crate::tool::StaticToolDefinition {
        crate::tool::StaticToolDefinition::new(crate::tool::ToolName::builtin(name), description)
    }

    pub(crate) fn test_route(
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> crate::ResolvedModelRoute {
        crate::ResolvedModelRoute {
            pricing_mode: pl_protocol::PricingMode::Catalog,
            role: crate::AgentRoleId::new("test").unwrap(),
            provider_id: crate::ProviderId::new("test").unwrap(),
            endpoint,
            model,
            effort: None,
        }
    }

    pub(crate) fn test_turn_engine_builder(
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> TurnEngineBuilder {
        TurnEngineBuilder::from_route(&test_route(endpoint, model)).unwrap()
    }

    pub(crate) fn test_turn_engine() -> TurnEngine {
        test_turn_engine_builder(
            ProviderEndpoint::deepseek(None),
            ModelInfo::compatible("deepseek-v4-flash"),
        )
        .build()
    }

    pub(crate) fn terminal_tool_event_count(events: &[TraceEvent]) -> usize {
        events
            .iter()
            .filter(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item } => {
                    item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
                }
                TraceEventKind::TracePartFailed { item } => {
                    item.kind() == pl_trace::TracePartKind::Tool && item.is_terminal()
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => false,
            })
            .count()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum TestToolPhase {
        Started,
        Streaming,
        AwaitingApproval,
        Approved,
        Running,
        Succeeded,
        Failed,
        Denied,
        Cancelled,
    }

    impl From<&pl_trace::TraceToolState> for TestToolPhase {
        fn from(state: &pl_trace::TraceToolState) -> Self {
            match state {
                pl_trace::TraceToolState::Started(_) => Self::Started,
                pl_trace::TraceToolState::Streaming(_) => Self::Streaming,
                pl_trace::TraceToolState::AwaitingApproval(_) => Self::AwaitingApproval,
                pl_trace::TraceToolState::Approved(_) => Self::Approved,
                pl_trace::TraceToolState::Running(_) => Self::Running,
                pl_trace::TraceToolState::Succeeded(_) => Self::Succeeded,
                pl_trace::TraceToolState::Failed(_) => Self::Failed,
                pl_trace::TraceToolState::Denied(_) => Self::Denied,
                pl_trace::TraceToolState::Cancelled(_) => Self::Cancelled,
            }
        }
    }

    pub(crate) fn tool_statuses(events: &[TraceEvent], item_id: &str) -> Vec<TestToolPhase> {
        events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartStarted { item }
                | TraceEventKind::TracePartCompleted { item }
                | TraceEventKind::TracePartFailed { item }
                    if item.kind() == TracePartKind::Tool && item.item_id() == item_id =>
                {
                    item.tool().map(|tool| TestToolPhase::from(tool.state()))
                }
                TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. }
                | TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. } => None,
            })
            .collect()
    }

    pub(crate) fn live_tool_result_deltas(events: &[AgentEvent], item_id: &str) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                AgentEvent::TracePartDelta { event }
                    if event.kind() == TracePartKind::Tool && event.item_id == item_id =>
                {
                    match &event.delta {
                        pl_trace::TraceDelta::ToolResult { delta } => Some(delta.clone()),
                        pl_trace::TraceDelta::Text { .. }
                        | pl_trace::TraceDelta::Thinking { .. }
                        | pl_trace::TraceDelta::ReasoningContent { .. }
                        | pl_trace::TraceDelta::ToolArguments { .. } => None,
                    }
                }
                AgentEvent::TracePartStarted { .. }
                | AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. }
                | AgentEvent::Done => None,
            })
            .collect()
    }

    pub(crate) fn runtime_progress_texts(
        event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
    ) -> Vec<String> {
        let mut progress_texts = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            match event {
                AgentEvent::TracePartCompleted { item }
                    if item.source() == TracePartSource::Runtime
                        && item
                            .text()
                            .is_some_and(|text| text.channel() == TraceTextChannel::Commentary) =>
                {
                    progress_texts.push(
                        item.text()
                            .expect("runtime commentary text")
                            .content()
                            .to_string(),
                    )
                }
                AgentEvent::TracePartStarted { .. }
                | AgentEvent::TracePartDelta { .. }
                | AgentEvent::TracePartCompleted { .. }
                | AgentEvent::TracePartFailed { .. }
                | AgentEvent::InteractionChanged { .. }
                | AgentEvent::AgentRuntimeUpdated { .. }
                | AgentEvent::SkillActivated { .. }
                | AgentEvent::TodoListUpdated { .. }
                | AgentEvent::TurnInterrupted { .. }
                | AgentEvent::TurnBudgetLimited { .. }
                | AgentEvent::Error { .. }
                | AgentEvent::Done => {}
            }
        }
        progress_texts
    }

    pub(crate) fn record_enabled_tools_for_core(
        core: &TurnEngine,
        session_id: &str,
        turn_id: &str,
    ) -> Vec<TraceEvent> {
        let tool_plan = core.acquire_tool_plan();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new(session_id.to_string(), event_tx, 0);

        super::turn_loop::enabled_tools::record_enabled_tools(
            &mut recorder,
            turn_id,
            0,
            &tool_plan,
        );

        recorder.drain()
    }

    pub(crate) fn enabled_tools_event(events: &[TraceEvent]) -> &pl_trace::EnabledToolsEvent {
        events
            .iter()
            .find_map(|event| match &event.kind {
                TraceEventKind::EnabledToolsRecorded { event } => Some(event),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. } => None,
            })
            .expect("enabled tools event")
    }

    #[derive(Debug)]
    pub(crate) struct SleepingTool;

    impl StaticTool for SleepingTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("sleeping_tool", "Sleeps until the turn is cancelled")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default().with_parallel_tool_calls()
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok(ToolResult::success("done"))
            }
        }
    }

    #[derive(Debug)]
    pub(crate) struct DeltaEchoTool;

    impl StaticTool for DeltaEchoTool {
        type Input = serde_json::Value;

        fn definition(&self) -> crate::tool::StaticToolDefinition {
            test_static_tool_definition("delta_echo", "Echoes a trace delta before completing")
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::default()
        }

        fn execute(
            &self,
            _input: Self::Input,
            context: ToolCallContext,
        ) -> impl std::future::Future<Output = crate::Result<ToolResult>> + Send {
            async move {
                let now = crate::time::unix_seconds();
                let event = pl_trace::TracePartDeltaEvent {
                    turn_id: context.identity().turn_id.clone(),
                    item_id: context.identity().item_id.clone(),
                    started_sequence: 0,
                    revision: context.identity().revision_base.saturating_add(1),
                    created_at: now,
                    updated_at: now,
                    delta: pl_trace::TraceDelta::ToolResult {
                        delta: "runtime delta".to_string(),
                    },
                };
                let _ = context.events().send(AgentEvent::TracePartDelta { event });
                Ok(ToolResult::success("delta complete"))
            }
        }
    }
}
