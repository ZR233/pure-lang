use std::path::Path;

use lsp_types::{
    CallHierarchyClientCapabilities, ClientCapabilities, CompletionClientCapabilities,
    CompletionItemCapability, ConfigurationParams, DidChangeWatchedFilesClientCapabilities,
    DidChangeWatchedFilesParams, DocumentSymbolClientCapabilities, FileChangeType, FileEvent,
    GeneralClientCapabilities, GotoCapability, HoverClientCapabilities, InitializeParams,
    MarkupKind, PositionEncodingKind, PublishDiagnosticsClientCapabilities,
    ReferenceClientCapabilities, TextDocumentClientCapabilities, WindowClientCapabilities,
    WorkDoneProgressParams, WorkspaceClientCapabilities, WorkspaceFolder,
};
use serde_json::Value;

use super::uri::file_uri;
use crate::driver::LspServerDriver;
use crate::runtime::{LspResult, ResolvedLspServer};

pub(crate) fn initialize_params(
    server: &ResolvedLspServer,
    initialization_options: Value,
) -> LspResult<Value> {
    let workspace_uri = file_uri(&server.workspace_root)?;
    let workspace_name = server
        .workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();
    #[allow(deprecated)]
    let params = InitializeParams {
        process_id: None,
        root_path: Some(server.workspace_root.display().to_string()),
        root_uri: Some(workspace_uri.clone()),
        initialization_options: Some(initialization_options),
        capabilities: client_capabilities(),
        workspace_folders: Some(vec![WorkspaceFolder {
            uri: workspace_uri,
            name: workspace_name,
        }]),
        work_done_progress_params: WorkDoneProgressParams::default(),
        ..InitializeParams::default()
    };
    Ok(serde_json::to_value(params)?)
}

pub(crate) fn workspace_configuration_response(
    params: Option<&Value>,
    driver: &dyn LspServerDriver,
) -> Value {
    let Some(params) = params
        .cloned()
        .and_then(|params| serde_json::from_value::<ConfigurationParams>(params).ok())
    else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        params
            .items
            .iter()
            .map(|item| driver.configuration_response(item.section.as_deref()))
            .collect(),
    )
}

pub(crate) fn watched_file_event_params(
    path: &Path,
    typ: FileChangeType,
) -> LspResult<DidChangeWatchedFilesParams> {
    Ok(DidChangeWatchedFilesParams {
        changes: vec![FileEvent::new(file_uri(path)?, typ)],
    })
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..WindowClientCapabilities::default()
        }),
        workspace: Some(WorkspaceClientCapabilities {
            configuration: Some(true),
            did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities {
                dynamic_registration: Some(true),
                ..DidChangeWatchedFilesClientCapabilities::default()
            }),
            ..WorkspaceClientCapabilities::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                related_information: Some(true),
                code_description_support: Some(true),
                data_support: Some(true),
                ..PublishDiagnosticsClientCapabilities::default()
            }),
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown]),
                ..HoverClientCapabilities::default()
            }),
            completion: Some(CompletionClientCapabilities {
                completion_item: Some(CompletionItemCapability {
                    documentation_format: Some(vec![MarkupKind::Markdown]),
                    ..CompletionItemCapability::default()
                }),
                ..CompletionClientCapabilities::default()
            }),
            definition: Some(GotoCapability::default()),
            references: Some(ReferenceClientCapabilities::default()),
            document_symbol: Some(DocumentSymbolClientCapabilities::default()),
            implementation: Some(GotoCapability::default()),
            call_hierarchy: Some(CallHierarchyClientCapabilities {
                dynamic_registration: Some(false),
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![PositionEncodingKind::UTF16]),
            ..GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    }
}
