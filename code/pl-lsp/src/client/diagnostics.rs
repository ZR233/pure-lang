use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use lsp_types::{DiagnosticSeverity, NumberOrString, PublishDiagnosticsParams};
use tokio::sync::Mutex;

use crate::clock::unix_seconds;
use crate::query::{LspDiagnostic, LspPosition, LspRange};

use super::uri::{file_uri_to_path, normalize_separators};

/// 诊断收集器，负责接收 LSP 服务器发布的诊断信息并存储。
#[derive(Clone)]
pub(crate) struct DiagnosticSink {
    server_id: String,
    workspace_root: PathBuf,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
    pub updates: tokio::sync::broadcast::Sender<()>,
}

impl DiagnosticSink {
    pub fn new(
        server_id: String,
        workspace_root: PathBuf,
        diagnostics: Arc<Mutex<HashMap<String, Vec<LspDiagnostic>>>>,
        updates: tokio::sync::broadcast::Sender<()>,
    ) -> Self {
        Self {
            server_id,
            workspace_root,
            diagnostics,
            updates,
        }
    }

    pub async fn publish(&self, params: PublishDiagnosticsParams) {
        let received_at = unix_seconds();
        let path = file_uri_to_path(params.uri.as_str());
        let display_path = path
            .strip_prefix(&self.workspace_root)
            .map(normalize_separators)
            .unwrap_or_else(|_| normalize_separators(&path));
        let diagnostics = params
            .diagnostics
            .iter()
            .map(|diagnostic| LspDiagnostic {
                server_id: self.server_id.clone(),
                uri: params.uri.as_str().to_string(),
                path: display_path.clone(),
                range: LspRange {
                    start: LspPosition {
                        line: diagnostic.range.start.line,
                        character: diagnostic.range.start.character,
                    },
                    end: LspPosition {
                        line: diagnostic.range.end.line,
                        character: diagnostic.range.end.character,
                    },
                },
                severity: diagnostic.severity.map(diagnostic_severity),
                message: diagnostic.message.clone(),
                source: diagnostic.source.clone(),
                code: diagnostic.code.clone().map(number_or_string),
                received_at,
            })
            .collect::<Vec<_>>();
        let server_id = &self.server_id;
        let uri_str = params.uri.as_str();
        self.diagnostics
            .lock()
            .await
            .insert(format!("{server_id}:{uri_str}"), diagnostics);
        let _ = self.updates.send(());
    }
}

/// 将 LSP 诊断 severity 映射为数值。
fn diagnostic_severity(severity: DiagnosticSeverity) -> u32 {
    match severity {
        DiagnosticSeverity::ERROR => 1,
        DiagnosticSeverity::WARNING => 2,
        DiagnosticSeverity::INFORMATION => 3,
        DiagnosticSeverity::HINT => 4,
        _ => 0,
    }
}

/// 将 `NumberOrString` 转换为字符串。
fn number_or_string(nos: NumberOrString) -> String {
    match nos {
        NumberOrString::Number(n) => n.to_string(),
        NumberOrString::String(s) => s,
    }
}
