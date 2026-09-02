use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use pl_lsp::catalog::{
    LspCatalogError, LspCatalogServer, LspCommandSpec, LspServerCatalog, LspServerDefinition,
    LspUserServerConfig,
};
use pl_lsp::driver::{LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver};
use pl_lsp::host::LspHostBackend;
use pl_lsp::query::{LspQuery, LspQueryOperation};
use pl_lsp::runtime::{
    LspAvailabilityKind, LspMissingComponent, LspRoutingError, LspRuntimeError, LspRuntimeRegistry,
};

#[tokio::test]
async fn public_runtime_contract_projects_catalog_probe_query_and_shutdown() {
    let workspace = workspace("pure.toml");
    let registry = LspRuntimeRegistry::with_catalog(catalog_with(server(
        "pure-lsp",
        "pure",
        Arc::new(ReadyDriver),
    )));

    registry
        .reconcile_workspace_membership(workspace.path())
        .await;
    assert_eq!(
        registry
            .capabilities_for_workspace(workspace.path())
            .await
            .servers[0]
            .availability,
        "checking"
    );

    registry.probe_lsp_server(workspace.path()).await;
    let capabilities = registry.capabilities_for_workspace(workspace.path()).await;
    assert!(capabilities.servers[0].ready);
    let result = registry
        .query_in_workspace(workspace.path(), diagnostics_query("pure"))
        .await
        .expect("diagnostics query");
    assert!(result.success);
    assert_eq!(result.server_id.as_deref(), Some("pure-lsp"));

    registry.shutdown().await;
    registry
        .reconcile_workspace_membership(workspace.path())
        .await;
    assert!(registry.snapshots().await.is_empty());
}

#[tokio::test]
async fn ambiguous_language_is_rejected_with_all_candidates() {
    let workspace = workspace("pure.toml");
    let mut catalog = LspServerCatalog::empty();
    catalog
        .insert(server("alpha", "pure", Arc::new(ReadyDriver)))
        .expect("alpha server");
    catalog
        .insert(server("beta", "pure", Arc::new(ReadyDriver)))
        .expect("beta server");
    let registry = LspRuntimeRegistry::with_catalog(catalog);
    registry
        .reconcile_workspace_membership(workspace.path())
        .await;
    registry.probe_lsp_server(workspace.path()).await;

    let error = registry
        .query_in_workspace(workspace.path(), diagnostics_query("pure"))
        .await
        .expect_err("ambiguous language must not select by registration order");

    assert!(matches!(
        error,
        LspRuntimeError::Routing(LspRoutingError::AmbiguousLanguage {
            language_id,
            servers,
        }) if language_id == "pure" && servers == ["alpha", "beta"]
    ));
}

#[tokio::test]
async fn repair_consumes_missing_component_and_reprobes_the_server() {
    let workspace = workspace("pure.toml");
    let repaired = Arc::new(AtomicBool::new(false));
    let registry = LspRuntimeRegistry::with_catalog(catalog_with(server(
        "repairable",
        "pure",
        Arc::new(RepairableDriver {
            repaired: repaired.clone(),
        }),
    )));
    registry
        .reconcile_workspace_membership(workspace.path())
        .await;
    registry.probe_lsp_server(workspace.path()).await;
    assert!(matches!(
        registry.snapshots().await[0].availability_kind,
        LspAvailabilityKind::MissingServerComponent(_)
    ));

    registry
        .repair_lsp_server(workspace.path(), "repairable")
        .await
        .expect("repair server");

    assert!(repaired.load(Ordering::Acquire));
    assert_eq!(
        registry.snapshots().await[0].availability_kind,
        LspAvailabilityKind::Available
    );
}

#[tokio::test]
async fn user_server_missing_command_is_published_as_typed_availability() {
    let workspace = workspace("pure.toml");
    let user_servers = BTreeMap::from([(
        "pure".to_string(),
        LspUserServerConfig {
            command: "definitely-not-a-language-server-pure-test".to_string(),
            language_ids: vec!["pure".to_string()],
            detection: vec!["pure.toml".to_string()],
            extensions: vec![".pure".to_string()],
            ..LspUserServerConfig::default()
        },
    )]);
    let registry = LspRuntimeRegistry::with_catalog(
        LspServerCatalog::with_user_servers(&user_servers).expect("user catalog"),
    );
    registry
        .reconcile_workspace_membership(workspace.path())
        .await;
    registry.probe_lsp_server(workspace.path()).await;

    let snapshot = registry
        .snapshots()
        .await
        .into_iter()
        .find(|snapshot| snapshot.id == "pure")
        .expect("custom server snapshot");
    assert_eq!(
        snapshot.availability_kind,
        LspAvailabilityKind::MissingCommand
    );
}

#[test]
fn user_catalog_rejects_language_conflicts() {
    let user_servers = BTreeMap::from([(
        "custom-rust".to_string(),
        LspUserServerConfig {
            command: "other-rust-server".to_string(),
            language_ids: vec!["rust".to_string()],
            ..LspUserServerConfig::default()
        },
    )]);

    let error = LspServerCatalog::with_user_servers(&user_servers)
        .expect_err("builtin language ownership must be unique");

    assert!(matches!(
        error,
        LspCatalogError::ConflictingLanguage { language_id, .. } if language_id == "rust"
    ));
}

fn workspace(marker: &str) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    std::fs::write(workspace.path().join(marker), "schema = 1\n").expect("write workspace marker");
    workspace
}

fn catalog_with(server: LspCatalogServer) -> LspServerCatalog {
    let mut catalog = LspServerCatalog::empty();
    catalog.insert(server).expect("unique server");
    catalog
}

fn server(id: &str, language_id: &str, driver: Arc<dyn LspServerDriver>) -> LspCatalogServer {
    LspCatalogServer {
        definition: LspServerDefinition {
            id: id.to_string(),
            display_name: id.to_string(),
            language_ids: vec![language_id.to_string()],
            extensions: vec![format!(".{language_id}")],
            detection: vec!["pure.toml".to_string()],
            command: LspCommandSpec {
                program: "unused-test-server".to_string(),
                args: Vec::new(),
            },
            operations: LspQueryOperation::all().to_vec(),
        },
        driver,
    }
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

#[derive(Debug)]
struct ReadyDriver;

impl LspServerDriver for ReadyDriver {
    fn probe<'a>(
        &'a self,
        _command: &'a LspResolvedCommand,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, LspProbeOutcome> {
        std::future::ready(LspProbeOutcome::Ready {
            version: "test 1.0".to_string(),
        })
        .boxed()
    }

    fn repair<'a>(
        &'a self,
        _component: &'a LspMissingComponent,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, Result<(), LspRepairError>> {
        std::future::ready(Err(LspRepairError::NotSupported)).boxed()
    }
}

#[derive(Debug)]
struct RepairableDriver {
    repaired: Arc<AtomicBool>,
}

impl LspServerDriver for RepairableDriver {
    fn probe<'a>(
        &'a self,
        _command: &'a LspResolvedCommand,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, LspProbeOutcome> {
        let outcome = if self.repaired.load(Ordering::Acquire) {
            LspProbeOutcome::Ready {
                version: "repaired".to_string(),
            }
        } else {
            LspProbeOutcome::MissingComponent(LspMissingComponent {
                component: "pure-component".to_string(),
                repair_hint: "install pure-component".to_string(),
            })
        };
        std::future::ready(outcome).boxed()
    }

    fn repair<'a>(
        &'a self,
        component: &'a LspMissingComponent,
        _host: Option<&'a dyn LspHostBackend>,
    ) -> futures::future::BoxFuture<'a, Result<(), LspRepairError>> {
        async move {
            if component.component != "pure-component" {
                return Err(LspRepairError::NotSupported);
            }
            self.repaired.store(true, Ordering::Release);
            Ok(())
        }
        .boxed()
    }
}
