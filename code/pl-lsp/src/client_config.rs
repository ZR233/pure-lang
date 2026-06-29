use std::path::Path;
use std::str::FromStr;

use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri};
use serde_json::Value;

use crate::server_definition::{LspServerDefinition, RUST_ANALYZER_ID};
use crate::types::{LspResult, LspRuntimeError};

pub(crate) fn initialize_params(definition: &LspServerDefinition) -> Value {
    let workspace_uri = crate::uri::path_to_file_uri(&definition.workspace_root);
    let workspace_name = definition
        .workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    serde_json::json!({
        "processId": null,
        "rootPath": definition.workspace_root,
        "rootUri": workspace_uri,
        "workspaceFolders": [{
            "uri": workspace_uri,
            "name": workspace_name,
        }],
        "capabilities": {
            "window": {
                "workDoneProgress": true,
            },
            "workspace": {
                "configuration": true,
                "didChangeWatchedFiles": {
                    "dynamicRegistration": true,
                },
            },
            "textDocument": {
                "publishDiagnostics": {
                    "relatedInformation": true,
                    "codeDescription": true,
                    "dataSupport": true,
                },
                "hover": {
                    "contentFormat": ["markdown"],
                },
                "completion": {
                    "completionItem": {
                        "documentationFormat": ["markdown"],
                    },
                },
                "definition": {},
                "references": {},
                "documentSymbol": {},
                "implementation": {},
                "callHierarchy": { "dynamicRegistration": false },
            },
            "general": {
                "positionEncodings": ["utf-16"],
            },
        },
        "initializationOptions": initialization_options(definition),
    })
}

pub(crate) fn workspace_configuration_response(params: Option<&Value>, server_id: &str) -> Value {
    let Some(items) = params
        .and_then(|params| params.get("items"))
        .and_then(Value::as_array)
    else {
        return serde_json::json!([]);
    };
    Value::Array(
        items
            .iter()
            .map(|item| {
                let section = item.get("section").and_then(Value::as_str);
                configuration_value_for_section(server_id, section)
            })
            .collect(),
    )
}

pub(crate) fn watched_file_event_params(path: &Path, typ: FileChangeType) -> LspResult<Value> {
    let uri = Uri::from_str(&crate::uri::path_to_file_uri(path))
        .map_err(|error| LspRuntimeError::InvalidQuery(format!("invalid file URI: {error}")))?;
    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent::new(uri, typ)],
    };
    Ok(serde_json::to_value(params)?)
}

fn initialization_options(definition: &LspServerDefinition) -> Value {
    if definition.id == RUST_ANALYZER_ID {
        rust_analyzer_settings()
    } else {
        Value::Null
    }
}

fn configuration_value_for_section(server_id: &str, section: Option<&str>) -> Value {
    if server_id != RUST_ANALYZER_ID {
        return Value::Null;
    }
    match section {
        Some("rust-analyzer") | None => rust_analyzer_settings(),
        Some("rust-analyzer.files") => serde_json::json!({ "watcher": "client" }),
        Some("rust-analyzer.files.watcher") => serde_json::json!("client"),
        Some(_) => Value::Null,
    }
}

fn rust_analyzer_settings() -> Value {
    serde_json::json!({
        "files": {
            "watcher": "client",
        },
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::server_definition::RUST_ANALYZER_ID;

    #[test]
    fn initialize_params_configures_rust_analyzer_client_watcher() {
        let params = initialize_params(&test_definition(RUST_ANALYZER_ID));

        assert_eq!(
            params["capabilities"]["window"]["workDoneProgress"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["capabilities"]["workspace"]["configuration"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["capabilities"]["workspace"]["didChangeWatchedFiles"]["dynamicRegistration"],
            serde_json::json!(true)
        );
        assert_eq!(
            params["initializationOptions"],
            serde_json::json!({ "files": { "watcher": "client" } })
        );
    }

    #[test]
    fn workspace_configuration_returns_rust_analyzer_watcher_settings() {
        let params = serde_json::json!({
            "items": [
                { "section": "rust-analyzer" },
                { "section": "rust-analyzer.files" },
                { "section": "rust-analyzer.files.watcher" },
                { "section": "rust-analyzer.cargo" }
            ]
        });

        let result = workspace_configuration_response(Some(&params), RUST_ANALYZER_ID);

        assert_eq!(
            result,
            serde_json::json!([
                { "files": { "watcher": "client" } },
                { "watcher": "client" },
                "client",
                null
            ])
        );
    }

    fn test_definition(id: &str) -> LspServerDefinition {
        LspServerDefinition {
            id: id.to_string(),
            display_name: id.to_string(),
            command: id.to_string(),
            args: Vec::new(),
            extensions: vec![".rs".to_string()],
            language_ids: vec!["rust".to_string()],
            workspace_root: std::env::current_dir().unwrap(),
        }
    }
}
