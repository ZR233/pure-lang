use std::sync::Arc;

use super::{LspCatalogServer, LspCommandSpec, LspServerDefinition};
use crate::driver::rust_analyzer::RustAnalyzerDriver;
use crate::query::LspQueryOperation;

/// 内置 rust-analyzer server id。
pub const RUST_ANALYZER_ID: &str = "rust-analyzer";

pub(super) fn builtin_rust_analyzer() -> LspCatalogServer {
    LspCatalogServer {
        definition: LspServerDefinition {
            id: RUST_ANALYZER_ID.to_string(),
            display_name: "rust-analyzer".to_string(),
            language_ids: vec!["rust".to_string()],
            extensions: vec![".rs".to_string()],
            detection: vec!["Cargo.toml".to_string()],
            command: LspCommandSpec {
                program: RUST_ANALYZER_ID.to_string(),
                args: Vec::new(),
            },
            operations: LspQueryOperation::all().to_vec(),
        },
        driver: Arc::new(RustAnalyzerDriver::new()),
    }
}
