use std::path::Path;
use std::sync::atomic::Ordering;

use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    FileChangeType, TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Uri,
    VersionedTextDocumentIdentifier,
};

use super::configuration::watched_file_event_params;
use super::connection::LspClient;
use super::uri::parse_uri;
use crate::runtime::{LspResult, LspRuntimeError};

const MAX_FILE_SIZE_BYTES: u64 = 10_000_000;

#[derive(Debug, Clone)]
pub(super) struct OpenDocument {
    uri: Uri,
    version: i32,
    content: String,
}

impl LspClient {
    pub(crate) async fn open_document(&self, path: &Path, uri: &str) -> LspResult<()> {
        let (content, file_size) = self.read_document(path).await?;
        if file_size > MAX_FILE_SIZE_BYTES {
            return Ok(());
        }
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
        if let Some(document) = document {
            if document.content == content {
                return Ok(());
            }
            let next_version = document.version + 1;
            self.notify(
                "textDocument/didChange",
                full_document_change(&document, next_version, &content),
            )
            .await?;
            if let Some(document) = self.opened_files.lock().await.get_mut(path) {
                document.version = next_version;
                document.content = content;
            }
        } else {
            let language_id = self.server.language_for_path(path).unwrap_or("text");
            let uri = parse_uri(uri)?;
            self.notify(
                "textDocument/didOpen",
                DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri: uri.clone(),
                        language_id: language_id.to_string(),
                        version: 1,
                        text: content.clone(),
                    },
                },
            )
            .await?;
            self.opened_files.lock().await.insert(
                path.to_path_buf(),
                OpenDocument {
                    uri,
                    version: 1,
                    content,
                },
            );
        }
        Ok(())
    }

    pub(crate) async fn close_document(&self, path: &Path) -> LspResult<()> {
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
        if let Some(document) = document {
            self.notify(
                "textDocument/didClose",
                DidCloseTextDocumentParams {
                    text_document: TextDocumentIdentifier { uri: document.uri },
                },
            )
            .await?;
            self.opened_files.lock().await.remove(path);
        }
        Ok(())
    }

    pub(crate) async fn change_document(&self, path: &Path) -> LspResult<()> {
        let (content, _) = self.read_document(path).await?;
        let _sync = self.document_sync.lock().await;
        let document = self.opened_files.lock().await.get(path).cloned();
        if let Some(document) = document {
            if document.content == content {
                return Ok(());
            }
            let next_version = document.version + 1;
            self.notify(
                "textDocument/didChange",
                full_document_change(&document, next_version, &content),
            )
            .await?;
            if let Some(document) = self.opened_files.lock().await.get_mut(path) {
                document.version = next_version;
                document.content = content;
            }
        }
        Ok(())
    }

    pub(crate) async fn refresh_document(&self, path: &Path) -> LspResult<()> {
        let is_file = if let Some(host) = &self.host {
            host.stat(path)
                .await
                .map_err(|error| LspRuntimeError::Unavailable(error.to_string()))?
                .is_some_and(|stat| stat.is_file)
        } else {
            path.is_file()
        };
        if is_file {
            self.change_document(path).await
        } else {
            self.close_document(path).await
        }
    }

    pub(crate) async fn file_changed(&self, path: &Path) -> LspResult<()> {
        self.notify_watched_file_event(path, FileChangeType::CHANGED)
            .await
    }

    pub(crate) async fn file_deleted(&self, path: &Path) -> LspResult<()> {
        self.notify_watched_file_event(path, FileChangeType::DELETED)
            .await
    }

    async fn notify_watched_file_event(&self, path: &Path, typ: FileChangeType) -> LspResult<()> {
        if !self.initialized.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.notify(
            "workspace/didChangeWatchedFiles",
            watched_file_event_params(path, typ)?,
        )
        .await
    }

    async fn read_document(&self, path: &Path) -> LspResult<(String, u64)> {
        if let Some(host) = &self.host {
            let stat = host
                .stat(path)
                .await
                .map_err(|error| LspRuntimeError::Unavailable(error.to_string()))?
                .ok_or_else(|| {
                    LspRuntimeError::Unavailable(format!(
                        "LSP document does not exist: {}",
                        path.display()
                    ))
                })?;
            if !stat.is_file {
                return Err(LspRuntimeError::Unavailable(format!(
                    "LSP document is not a file: {}",
                    path.display()
                )));
            }
            if stat.byte_size > MAX_FILE_SIZE_BYTES {
                return Ok((String::new(), stat.byte_size));
            }
            let bytes = host
                .read_file(path, MAX_FILE_SIZE_BYTES)
                .await
                .map_err(|error| LspRuntimeError::Unavailable(error.to_string()))?;
            let content = String::from_utf8(bytes).map_err(|error| {
                LspRuntimeError::Unavailable(format!(
                    "LSP document is not UTF-8 ({}): {error}",
                    path.display()
                ))
            })?;
            Ok((content, stat.byte_size))
        } else {
            let content = tokio::fs::read_to_string(path).await?;
            let file_size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            Ok((content, file_size))
        }
    }
}

fn full_document_change(
    document: &OpenDocument,
    version: i32,
    content: &str,
) -> DidChangeTextDocumentParams {
    DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: document.uri.clone(),
            version,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: content.to_string(),
        }],
    }
}
