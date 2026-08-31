//! registry 单元测试：membership、probe/repair 状态、路由与通用性验收。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;

use super::testing::{
    FakeDriver, available_rust_catalog, available_rust_server, catalog_with_rust_analyzer_command,
    language_server, purelang_catalog, purelang_catalog_server,
};
use super::{LspRuntimeRegistry, canonical_workspace_root};
use crate::catalog::{LspServerCatalog, LspUserServerConfig, RUST_ANALYZER_ID};
use crate::types::{
    LanguageToolInfo, LspQuery, LspQueryOperation, LspRoutingError, LspRuntimeError,
};

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-lsp-{name}-{stamp}"))
}

/// 构造带 `Cargo.toml` 的临时 workspace 并返回其路径。
fn rust_workspace(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn answer() -> i32 {\n    42\n}\n",
    )
    .unwrap();
    dir
}

/// 测试直通车：把一个 Available member 注入指定 workspace（多 workspace 路由场景
/// 不经过 reconcile，reconcile 以当前活跃 workspace 为准清理兄弟 workspace）。
async fn register_available_workspace_member(
    registry: &LspRuntimeRegistry,
    workspace_root: &std::path::Path,
    server: &crate::catalog::LspCatalogServer,
) {
    let resolved =
        super::membership::resolve_member(server, &canonical_workspace_root(workspace_root));
    let mut state = registry.state.lock().await;
    state
        .workspaces
        .entry(canonical_workspace_root(workspace_root))
        .or_default()
        .servers
        .insert(
            resolved.id.clone(),
            super::LspRuntimeServerState::new(
                resolved,
                server.driver.clone(),
                crate::types::LspAvailabilityKind::Available,
                Some("multi-workspace test member".to_string()),
                Some(1),
            ),
        );
}

fn diagnostics_query(language_id: &str) -> LspQuery {
    LspQuery {
        operation: LspQueryOperation::Diagnostics,
        file_path: None,
        line: None,
        character: None,
        query: None,
        max_results: None,
        language_id: Some(language_id.to_string()),
    }
}

#[tokio::test]
async fn missing_rust_analyzer_command_records_snapshot() {
    let dir = rust_workspace("missing-command");
    let registry = LspRuntimeRegistry::with_catalog(catalog_with_rust_analyzer_command(
        "definitely-not-rust-analyzer-pure-test",
    ));

    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;
    let snapshots = registry.snapshots().await;

    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].availability_kind,
        crate::types::LspAvailabilityKind::MissingCommand
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn unmatched_detection_reports_disabled_member() {
    let dir = temp_dir("disabled-member");
    fs::create_dir_all(&dir).unwrap();
    let registry = LspRuntimeRegistry::new();

    registry.reconcile_workspace_membership(&dir).await;
    let snapshots = registry.snapshots().await;
    let capabilities = registry.capabilities_for_workspace(&dir).await;

    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        snapshots[0].availability_kind,
        crate::types::LspAvailabilityKind::Disabled
    );
    assert_eq!(capabilities.servers.len(), 1);
    assert_eq!(capabilities.servers[0].availability, "disabled");
    assert!(!capabilities.servers[0].ready);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
#[ignore = "requires rust-analyzer component and starts the language server"]
async fn live_rust_analyzer_queries_unique_cargo_demo() {
    let workspace = tempfile::tempdir().expect("temporary Cargo demo");
    let workspace_root = workspace.path().to_path_buf();
    let source_root = workspace_root.join("src");
    fs::create_dir_all(&source_root).expect("create demo source directory");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname='pure_lsp_live_demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .expect("write demo manifest");
    let library = source_root.join("lib.rs");
    fs::write(&library, "pub fn answer() -> i32 {\n    42\n}\n").expect("write demo library");
    let binary = source_root.join("main.rs");
    fs::write(
        &binary,
        "use pure_lsp_live_demo::answer;\n\nfn main() {\n    println!(\"{}\", answer());\n}\n",
    )
    .expect("write demo binary");
    let registry = LspRuntimeRegistry::new();

    registry
        .reconcile_workspace_membership(&workspace_root)
        .await;
    let snapshots = registry.snapshots().await;
    let outcome = async {
        let document_symbols = registry
            .query(live_query(
                LspQueryOperation::DocumentSymbol,
                &library,
                None,
            ))
            .await?;
        let hover = registry
            .query(live_query(LspQueryOperation::Hover, &library, Some((1, 8))))
            .await?;
        let definition = registry
            .query(live_query(
                LspQueryOperation::GoToDefinition,
                &binary,
                Some((4, 20)),
            ))
            .await?;
        let references = registry
            .query(live_query(
                LspQueryOperation::FindReferences,
                &binary,
                Some((4, 20)),
            ))
            .await?;
        Ok::<_, LspRuntimeError>((document_symbols, hover, definition, references))
    }
    .await;
    let process_id = registry
        .client_for_query_in_workspace(
            &canonical_workspace_root(&workspace_root),
            &live_query(LspQueryOperation::DocumentSymbol, &library, None),
        )
        .await
        .expect("live rust-analyzer client")
        .2
        .child_id_for_test()
        .await
        .expect("live rust-analyzer process id");
    registry.shutdown().await;

    #[cfg(windows)]
    assert!(
        !windows_process_is_running(process_id),
        "rust-analyzer process {process_id} survived registry shutdown"
    );
    #[cfg(not(windows))]
    let _ = process_id;

    assert_eq!(snapshots.len(), 1);
    assert!(
        snapshots[0].availability_kind == crate::types::LspAvailabilityKind::Available,
        "{}",
        snapshots[0].availability_message.as_deref().unwrap_or("")
    );
    let (document_symbols, hover, definition, references) =
        outcome.expect("live rust-analyzer queries");
    eprintln!("document symbols:\n{}", document_symbols.result);
    eprintln!("hover:\n{}", hover.result);
    eprintln!("definition:\n{}", definition.result);
    eprintln!("references:\n{}", references.result);
    for result in [&document_symbols, &hover, &definition, &references] {
        assert!(result.success);
        assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
    }
    assert!(document_symbols.result.contains("answer"));
    assert!(hover.result.contains("answer"), "{}", hover.result);
    assert!(
        definition.result.replace('\\', "/").contains("src/lib.rs"),
        "{}",
        definition.result
    );
    assert!(references.result_count.is_some_and(|count| count >= 2));
    let reference_paths = references.result.replace('\\', "/");
    assert!(reference_paths.contains("src/lib.rs"), "{reference_paths}");
    assert!(reference_paths.contains("src/main.rs"), "{reference_paths}");
}

fn live_query(
    operation: LspQueryOperation,
    file_path: &std::path::Path,
    position: Option<(u32, u32)>,
) -> LspQuery {
    LspQuery {
        operation,
        file_path: Some(file_path.to_path_buf()),
        line: position.map(|(line, _)| line),
        character: position.map(|(_, character)| character),
        query: None,
        max_results: None,
        language_id: Some("rust".to_string()),
    }
}

#[cfg(windows)]
fn windows_process_is_running(process_id: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let Ok(process) =
        (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) })
    else {
        return false;
    };
    let mut exit_code = 0;
    let running = unsafe { GetExitCodeProcess(process, &mut exit_code) }.is_ok()
        && exit_code == STILL_ACTIVE.0 as u32;
    let _ = unsafe { CloseHandle(process) };
    running
}

/// 零 pl-core 改动的通用性验收：注册假想语言 server 后，
/// workspace 检测、capabilities 报告与 languageId 路由全程走公共 API。
#[tokio::test]
async fn fake_language_server_flows_through_public_registry_api() {
    let dir = temp_dir("purelang-universality");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("pure.toml"), "schema = 1\n").unwrap();
    fs::write(dir.join("src/hello.purelang"), "fn hello() {}\n").unwrap();
    let registry = LspRuntimeRegistry::with_catalog(LspServerCatalog::empty());
    registry
        .register_server(purelang_catalog_server())
        .await
        .unwrap();
    registry.reconcile_workspace_membership(&dir).await;

    let capabilities = registry.capabilities_for_workspace(&dir).await;
    assert_eq!(capabilities.servers.len(), 1);
    let server = &capabilities.servers[0];
    assert_eq!(server.id, "purelang-server");
    assert_eq!(server.language_ids, vec!["purelang".to_string()]);
    assert_eq!(
        server.operations,
        vec![
            "hover".to_string(),
            "documentSymbol".to_string(),
            "diagnostics".to_string()
        ]
    );
    assert_eq!(server.availability, "checking");
    assert!(!server.ready);

    registry.probe_lsp_server(&dir).await;
    let capabilities = registry.capabilities_for_workspace(&dir).await;
    assert_eq!(capabilities.servers[0].availability, "available");
    assert!(capabilities.servers[0].ready);

    let languages = registry.available_languages_for_workspace(&dir).await;
    assert_eq!(
        languages,
        vec![LanguageToolInfo {
            language_id: "purelang".to_string(),
            server_id: "purelang-server".to_string(),
            display_name: "PureLang".to_string(),
            extensions: vec![".purelang".to_string()],
        }]
    );

    let result = registry
        .query_in_workspace(&dir, diagnostics_query("purelang"))
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.server_id.as_deref(), Some("purelang-server"));
    fs::remove_dir_all(dir).unwrap();
}

/// 操作子集校验：未声明的能力被路由层拒绝。
#[tokio::test]
async fn operation_outside_declared_capabilities_is_rejected() {
    let dir = temp_dir("purelang-operation-subset");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("pure.toml"), "schema = 1\n").unwrap();
    let registry = LspRuntimeRegistry::with_catalog(purelang_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let error = registry
        .query_in_workspace(
            &dir,
            LspQuery {
                operation: LspQueryOperation::FindReferences,
                file_path: None,
                line: None,
                character: None,
                query: None,
                max_results: None,
                language_id: Some("purelang".to_string()),
            },
        )
        .await
        .unwrap_err();

    match error {
        LspRuntimeError::InvalidQuery(message) => {
            assert!(message.contains("findReferences"), "{message}");
            assert!(message.contains("purelang-server"), "{message}");
        }
        other => panic!("unexpected error: {other}"),
    }
    fs::remove_dir_all(dir).unwrap();
}

/// 歧义拒绝：两个 server 声明同一 language_id 且都匹配 workspace。
#[tokio::test]
async fn ambiguous_language_lists_candidate_servers() {
    let dir = temp_dir("ambiguous-language");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("duallang.toml"), "schema = 1\n").unwrap();
    let registry = LspRuntimeRegistry::with_catalog(LspServerCatalog::empty());
    registry
        .register_server(language_server(
            "alpha-server",
            "Alpha",
            "duallang",
            "duallang.toml",
            FakeDriver::ready("alpha 1.0"),
        ))
        .await
        .unwrap();
    registry
        .register_server(language_server(
            "beta-server",
            "Beta",
            "duallang",
            "duallang.toml",
            FakeDriver::ready("beta 1.0"),
        ))
        .await
        .unwrap();
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let error = registry
        .query_in_workspace(&dir, diagnostics_query("duallang"))
        .await
        .unwrap_err();

    match error {
        LspRuntimeError::Routing(LspRoutingError::AmbiguousLanguage {
            language_id,
            servers,
        }) => {
            assert_eq!(language_id, "duallang");
            assert_eq!(
                servers,
                vec!["alpha-server".to_string(), "beta-server".to_string()]
            );
        }
        other => panic!("unexpected error: {other}"),
    }
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn ambiguous_file_extension_does_not_pick_a_server_by_registration_order() {
    let dir = temp_dir("ambiguous-extension");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("project.toml"), "schema = 1\n").unwrap();
    let source = dir.join("main.shared");
    fs::write(&source, "main\n").unwrap();
    let registry = LspRuntimeRegistry::with_catalog(LspServerCatalog::empty());
    let mut alpha = language_server(
        "alpha-server",
        "Alpha",
        "alpha",
        "project.toml",
        FakeDriver::ready("alpha 1.0"),
    );
    alpha.definition.extensions = vec![".shared".to_string()];
    let mut beta = language_server(
        "beta-server",
        "Beta",
        "beta",
        "project.toml",
        FakeDriver::ready("beta 1.0"),
    );
    beta.definition.extensions = vec![".shared".to_string()];
    registry.register_server(alpha).await.unwrap();
    registry.register_server(beta).await.unwrap();
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let error = registry
        .server_id_for_query(&LspQuery {
            operation: LspQueryOperation::DocumentSymbol,
            file_path: Some(source),
            line: None,
            character: None,
            query: None,
            max_results: None,
            language_id: None,
        })
        .await
        .expect_err("an ambiguous extension must be rejected");

    assert!(matches!(error, LspRuntimeError::Routing(_)));
    fs::remove_dir_all(dir).unwrap();
}

/// 未知语言错误列出当前可用语言。
#[tokio::test]
async fn unknown_language_lists_available_languages() {
    let dir = rust_workspace("unknown-language");
    let registry = LspRuntimeRegistry::with_catalog(available_rust_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let error = registry
        .query_in_workspace(&dir, diagnostics_query("kotlin"))
        .await
        .unwrap_err();

    match error {
        LspRuntimeError::Unavailable(message) => {
            assert!(message.contains("kotlin"), "{message}");
            assert!(message.contains("rust"), "{message}");
        }
        other => panic!("unexpected error: {other}"),
    }
    fs::remove_dir_all(dir).unwrap();
}

/// 用户配置声明的自定义 server 通过 catalog 合并进入 capabilities。
#[tokio::test]
async fn apply_user_servers_publishes_custom_language_members() {
    let dir = temp_dir("user-server-apply");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("pure.toml"), "schema = 1\n").unwrap();
    let registry = LspRuntimeRegistry::new();
    let mut user_servers = BTreeMap::new();
    user_servers.insert(
        "purelang".to_string(),
        LspUserServerConfig {
            command: "definitely-not-a-language-server-pure-test".to_string(),
            args: Vec::new(),
            language_ids: vec!["purelang".to_string()],
            detection: vec!["pure.toml".to_string()],
            extensions: vec![".purelang".to_string()],
            display_name: Some("PureLang (user)".to_string()),
            operations: Vec::new(),
        },
    );
    registry.apply_user_servers(&user_servers).await.unwrap();
    registry.reconcile_workspace_membership(&dir).await;

    let capabilities = registry.capabilities_for_workspace(&dir).await;
    assert_eq!(capabilities.servers.len(), 2);
    let purelang = capabilities
        .servers
        .iter()
        .find(|server| server.id == "purelang")
        .expect("user server member");
    assert_eq!(purelang.language_ids, vec!["purelang".to_string()]);
    assert_eq!(purelang.availability, "checking");
    let rust = capabilities
        .servers
        .iter()
        .find(|server| server.id == RUST_ANALYZER_ID)
        .expect("builtin member");
    assert_eq!(rust.availability, "disabled");

    let conflict = registry
        .apply_user_servers(&BTreeMap::from([(
            "custom-rust".to_string(),
            LspUserServerConfig {
                command: "other".to_string(),
                args: Vec::new(),
                language_ids: vec!["rust".to_string()],
                detection: Vec::new(),
                extensions: Vec::new(),
                display_name: None,
                operations: Vec::new(),
            },
        )]))
        .await
        .unwrap_err();
    assert!(matches!(
        conflict,
        crate::catalog::LspCatalogError::ConflictingLanguage { .. }
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn available_languages_returns_empty_when_no_servers() {
    let registry = LspRuntimeRegistry::new();
    let languages = registry.available_languages().await;
    assert!(languages.is_empty());
}

#[tokio::test]
async fn shutdown_is_terminal_and_reconcile_cannot_publish_a_workspace() {
    let workspace = rust_workspace("closed_registry");
    let registry = LspRuntimeRegistry::new();

    registry.shutdown().await;
    registry.reconcile_workspace_membership(&workspace).await;

    let state = registry.state.lock().await;
    assert!(state.closed);
    assert!(state.workspaces.is_empty());
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn shutdown_waits_for_in_flight_reconcile_section() {
    let registry = LspRuntimeRegistry::new();
    let reconcile_guard = registry.lifecycle.read().await;
    let shutting_down = registry.clone();
    let shutdown = tokio::spawn(async move { shutting_down.shutdown().await });
    loop {
        if registry.state.lock().await.closed {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert!(!shutdown.is_finished());
    drop(reconcile_guard);
    tokio::time::timeout(Duration::from_secs(5), shutdown)
        .await
        .expect("shutdown must finish after reconcile releases its lease")
        .expect("shutdown task must not panic");
}

#[tokio::test]
async fn available_languages_returns_empty_when_server_not_available() {
    let dir = rust_workspace("available-languages");
    let registry = LspRuntimeRegistry::with_catalog(catalog_with_rust_analyzer_command(
        "definitely-not-rust-analyzer-pure-test",
    ));
    registry.reconcile_workspace_membership(&dir).await;

    let languages = registry.available_languages().await;
    assert!(languages.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn available_languages_returns_available_server_languages() {
    let dir = rust_workspace("available-languages-rust");
    let registry = LspRuntimeRegistry::with_catalog(available_rust_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let languages = registry.available_languages().await;

    assert_eq!(
        languages,
        vec![LanguageToolInfo {
            language_id: "rust".to_string(),
            server_id: RUST_ANALYZER_ID.to_string(),
            display_name: "rust-analyzer".to_string(),
            extensions: vec![".rs".to_string()],
        }]
    );
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn multiple_workspaces_keep_independent_language_servers_and_route_by_file() {
    let first = rust_workspace("workspace-pool-first");
    let second = rust_workspace("workspace-pool-second");
    let first_file = first.join("src/lib.rs");
    let second_file = second.join("src/lib.rs");
    let registry = LspRuntimeRegistry::new();
    let rust = available_rust_server();
    register_available_workspace_member(&registry, &first, &rust).await;
    register_available_workspace_member(&registry, &second, &rust).await;

    assert_eq!(registry.state.lock().await.workspaces.len(), 2);
    assert_eq!(
        registry
            .available_languages_for_workspace(&first)
            .await
            .len(),
        1
    );
    assert_eq!(
        registry
            .available_languages_for_workspace(&second)
            .await
            .len(),
        1
    );
    let first_root = registry
        .workspace_root_for_query(&LspQuery {
            operation: LspQueryOperation::DocumentSymbol,
            file_path: Some(first_file),
            line: None,
            character: None,
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        })
        .await
        .unwrap();
    let second_root = registry
        .workspace_root_for_query(&LspQuery {
            operation: LspQueryOperation::DocumentSymbol,
            file_path: Some(second_file),
            line: None,
            character: None,
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        })
        .await
        .unwrap();

    assert_eq!(first_root, canonical_workspace_root(&first));
    assert_eq!(second_root, canonical_workspace_root(&second));
    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
}

#[tokio::test]
async fn server_id_for_query_prefers_language_id() {
    let dir = rust_workspace("language-route");
    let registry = LspRuntimeRegistry::with_catalog(available_rust_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;
    let query = LspQuery {
        operation: LspQueryOperation::WorkspaceSymbol,
        file_path: None,
        line: None,
        character: None,
        query: Some("LspRuntimeRegistry".to_string()),
        max_results: None,
        language_id: Some("rust".to_string()),
    };

    let server_id = registry.server_id_for_query(&query).await.unwrap();

    assert_eq!(server_id, RUST_ANALYZER_ID);
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn diagnostics_query_with_language_id_reports_target_server() {
    let dir = rust_workspace("diagnostics-language-route");
    let registry = LspRuntimeRegistry::with_catalog(available_rust_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let result = registry.query(diagnostics_query("rust")).await.unwrap();

    assert!(result.success);
    assert_eq!(result.server_id.as_deref(), Some(RUST_ANALYZER_ID));
    fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn multiple_workspaces_require_a_file_path_and_do_not_switch_roots() {
    let first = rust_workspace("multi-root-required-first");
    let second = rust_workspace("multi-root-required-second");
    let registry = LspRuntimeRegistry::new();
    let rust = available_rust_server();
    register_available_workspace_member(&registry, &first, &rust).await;
    register_available_workspace_member(&registry, &second, &rust).await;

    let error = registry
        .workspace_root_for_query(&LspQuery {
            operation: LspQueryOperation::WorkspaceSymbol,
            file_path: None,
            line: None,
            character: None,
            query: Some("symbol".to_string()),
            max_results: None,
            language_id: Some("rust".to_string()),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, LspRuntimeError::InvalidQuery(_)));
    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
}

#[tokio::test]
async fn nested_workspaces_route_to_the_longest_canonical_root() {
    let outer = temp_dir("nested-root-outer");
    let inner = outer.join("nested");
    fs::create_dir_all(inner.join("src")).unwrap();
    fs::write(
        inner.join("Cargo.toml"),
        "[package]\nname='inner'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        outer.join("Cargo.toml"),
        "[workspace]\nmembers=['nested']\n",
    )
    .unwrap();
    let file = inner.join("src/lib.rs");
    fs::write(&file, "fn nested() {}\n").unwrap();
    let registry = LspRuntimeRegistry::new();
    let rust = available_rust_server();
    register_available_workspace_member(&registry, &outer, &rust).await;
    register_available_workspace_member(&registry, &inner, &rust).await;

    let root = registry
        .workspace_root_for_query(&LspQuery {
            operation: LspQueryOperation::DocumentSymbol,
            file_path: Some(file),
            line: None,
            character: None,
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        })
        .await
        .unwrap();

    assert_eq!(root, canonical_workspace_root(&inner));
    fs::remove_dir_all(outer).unwrap();
}

#[tokio::test]
async fn file_outside_registered_workspaces_is_unavailable() {
    let workspace = rust_workspace("outside-route-workspace");
    let outside = temp_dir("outside-route-file").join("lib.rs");
    fs::create_dir_all(outside.parent().unwrap()).unwrap();
    fs::write(&outside, "fn outside() {}\n").unwrap();
    let registry = LspRuntimeRegistry::new();
    register_available_workspace_member(&registry, &workspace, &available_rust_server()).await;

    let error = registry
        .workspace_root_for_query(&LspQuery {
            operation: LspQueryOperation::DocumentSymbol,
            file_path: Some(outside.clone()),
            line: None,
            character: None,
            query: None,
            max_results: None,
            language_id: Some("rust".to_string()),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, LspRuntimeError::Unavailable(_)));
    fs::remove_dir_all(workspace).unwrap();
    fs::remove_dir_all(outside.parent().unwrap()).unwrap();
}

/// membership 指纹随 catalog 与检测结果变化。
#[tokio::test]
async fn membership_fingerprint_tracks_catalog_and_detection() {
    let rust_dir = rust_workspace("fingerprint-rust");
    let plain_dir = temp_dir("fingerprint-plain");
    fs::create_dir_all(&plain_dir).unwrap();
    let registry = LspRuntimeRegistry::new();

    let rust_fingerprint = registry.membership_fingerprint(&rust_dir).await;
    let plain_fingerprint = registry.membership_fingerprint(&plain_dir).await;

    assert_ne!(rust_fingerprint, plain_fingerprint);
    fs::remove_dir_all(rust_dir).unwrap();
    fs::remove_dir_all(plain_dir).unwrap();
}

#[tokio::test]
async fn repair_requires_missing_server_component_state() {
    let dir = rust_workspace("repair-state");
    let registry = LspRuntimeRegistry::with_catalog(available_rust_catalog());
    registry.reconcile_workspace_membership(&dir).await;
    registry.probe_lsp_server(&dir).await;

    let error = registry
        .repair_lsp_server(&dir, RUST_ANALYZER_ID)
        .await
        .unwrap_err();

    match error {
        LspRuntimeError::Unavailable(message) => {
            assert!(message.contains("missingServerComponent"), "{message}");
        }
        other => panic!("unexpected error: {other}"),
    }
    fs::remove_dir_all(dir).unwrap();
}

/// 直接构造 MissingServerComponent 状态验证 repair 只接受该状态并调用 driver。
#[tokio::test]
async fn repair_runs_driver_for_missing_server_component() {
    use crate::types::LspAvailabilityKind;

    struct RepairableDriver {
        repaired: std::sync::Mutex<Vec<String>>,
    }

    impl crate::driver::LspServerDriver for RepairableDriver {
        fn probe<'a>(
            &'a self,
            _command: &'a crate::driver::LspResolvedCommand,
            _host: Option<&'a dyn crate::host::LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, crate::driver::LspProbeOutcome> {
            futures::FutureExt::boxed(std::future::ready(crate::driver::LspProbeOutcome::Ready {
                version: "repaired".to_string(),
            }))
        }

        fn repair<'a>(
            &'a self,
            component: &'a crate::types::LspMissingComponent,
            _host: Option<&'a dyn crate::host::LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, Result<(), crate::driver::LspRepairError>> {
            let component = component.clone();
            let repaired = &self.repaired;
            futures::FutureExt::boxed(async move {
                repaired.lock().unwrap().push(component.component.clone());
                Ok(())
            })
        }
    }

    let dir = rust_workspace("repair-driver");
    let driver = Arc::new(RepairableDriver {
        repaired: std::sync::Mutex::new(Vec::new()),
    });
    let server = language_server(
        "repairable",
        "Repairable",
        "repairlang",
        "Cargo.toml",
        FakeDriver::ready("unused"),
    );
    let mut catalog = LspServerCatalog::empty();
    catalog
        .insert(crate::catalog::LspCatalogServer {
            definition: server.definition,
            driver: driver.clone(),
        })
        .unwrap();
    let registry = LspRuntimeRegistry::with_catalog(catalog);
    registry.reconcile_workspace_membership(&dir).await;
    {
        let mut state = registry.state.lock().await;
        let member = state
            .workspaces
            .get_mut(&canonical_workspace_root(&dir))
            .and_then(|workspace| workspace.servers.get_mut("repairable"))
            .expect("repairable member");
        member.availability_kind =
            LspAvailabilityKind::MissingServerComponent(crate::types::LspMissingComponent {
                component: "repairlang-extra".to_string(),
                repair_hint: "install repairlang-extra".to_string(),
            });
    }

    registry
        .repair_lsp_server(&dir, "repairable")
        .await
        .unwrap();

    assert_eq!(
        driver.repaired.lock().unwrap().as_slice(),
        ["repairlang-extra"]
    );
    let snapshots = registry.snapshots().await;
    assert!(
        snapshots[0].availability_kind == LspAvailabilityKind::Available,
        "{:?}",
        snapshots[0].availability_kind
    );
    fs::remove_dir_all(dir).unwrap();
}
