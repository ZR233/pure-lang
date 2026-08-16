//! registry 测试设施：fake driver 与 catalog 构造器。不进生产 catalog。

use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;

use crate::catalog::{
    LspCatalogServer, LspCommandSpec, LspServerCatalog, LspServerDefinition, RUST_ANALYZER_ID,
};
use crate::driver::{LspProbeOutcome, LspRepairError, LspResolvedCommand, LspServerDriver};
use crate::types::{LspMissingComponent, LspQueryOperation};

/// 探测结果可配置的假想语言 driver；不启动任何进程。
pub(super) struct FakeDriver {
    outcome: LspProbeOutcome,
}

impl FakeDriver {
    pub(super) fn ready(version: &str) -> Arc<Self> {
        Arc::new(Self {
            outcome: LspProbeOutcome::Ready {
                version: version.to_string(),
            },
        })
    }
}

impl LspServerDriver for FakeDriver {
    fn probe<'a>(&'a self, _command: &'a LspResolvedCommand) -> BoxFuture<'a, LspProbeOutcome> {
        FutureExt::boxed(std::future::ready(self.outcome.clone()))
    }

    fn repair<'a>(
        &'a self,
        _component: &'a LspMissingComponent,
    ) -> BoxFuture<'a, Result<(), LspRepairError>> {
        FutureExt::boxed(std::future::ready(Err(LspRepairError::NotSupported)))
    }
}

fn catalog_with_single(server: LspCatalogServer) -> LspServerCatalog {
    let mut catalog = LspServerCatalog::empty();
    catalog.insert(server).expect("unique test server id");
    catalog
}

fn builtin_rust_analyzer_definition() -> LspServerDefinition {
    LspServerCatalog::builtin()
        .get(RUST_ANALYZER_ID)
        .expect("builtin rust-analyzer entry")
        .definition
        .clone()
}

/// 内置 rust-analyzer 定义 + 指定 command（沿用 RustAnalyzerDriver，可观测 MissingCommand）。
pub(super) fn catalog_with_rust_analyzer_command(program: &str) -> LspServerCatalog {
    let builtin = LspServerCatalog::builtin();
    let rust = builtin.get(RUST_ANALYZER_ID).expect("builtin entry");
    let mut definition = builtin_rust_analyzer_definition();
    definition.command = LspCommandSpec {
        program: program.to_string(),
        args: Vec::new(),
    };
    catalog_with_single(LspCatalogServer {
        definition,
        driver: rust.driver.clone(),
    })
}

/// 内置 rust-analyzer 定义 + fake driver（探测恒为 Available，无进程依赖）。
pub(super) fn available_rust_catalog() -> LspServerCatalog {
    catalog_with_single(available_rust_server())
}

/// [`available_rust_catalog`] 的单条 server 形态，供多 workspace 路由测试注入。
pub(super) fn available_rust_server() -> LspCatalogServer {
    LspCatalogServer {
        definition: builtin_rust_analyzer_definition(),
        driver: FakeDriver::ready("rust-analyzer test"),
    }
}

/// 假想语言 "purelang" 的 server：检测 `pure.toml`，只声明部分操作。
pub(super) fn purelang_catalog() -> LspServerCatalog {
    catalog_with_single(purelang_catalog_server())
}

/// [`purelang_catalog`] 的单条 server 形态，供 `register_server` 使用。
pub(super) fn purelang_catalog_server() -> LspCatalogServer {
    language_server(
        "purelang-server",
        "PureLang",
        "purelang",
        "pure.toml",
        FakeDriver::ready("purelang-lsp 0.1 (test)"),
    )
}

/// 声明单个语言的通用测试 server。
pub(super) fn language_server(
    id: &str,
    display_name: &str,
    language_id: &str,
    detection: &str,
    driver: Arc<FakeDriver>,
) -> LspCatalogServer {
    LspCatalogServer {
        definition: LspServerDefinition {
            id: id.to_string(),
            display_name: display_name.to_string(),
            language_ids: vec![language_id.to_string()],
            extensions: vec![format!(".{language_id}")],
            detection: vec![detection.to_string()],
            command: LspCommandSpec {
                program: format!("{id}-lsp"),
                args: Vec::new(),
            },
            operations: vec![
                LspQueryOperation::Hover,
                LspQueryOperation::DocumentSymbol,
                LspQueryOperation::Diagnostics,
            ],
        },
        driver,
    }
}
