use std::path::Path;
use std::str::FromStr;

use lsp_types::{DidChangeWatchedFilesParams, FileChangeType, FileEvent, Uri};
use serde_json::Value;

use crate::driver::LspServerDriver;
use crate::resolved::ResolvedLspServer;
use crate::types::{LspResult, LspRuntimeError};

pub(crate) fn initialize_params(
    server: &ResolvedLspServer,
    initialization_options: Value,
) -> Value {
    let workspace_uri = crate::uri::path_to_file_uri(&server.workspace_root);
    let workspace_name = server
        .workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    serde_json::json!({
        "processId": null,
        "rootPath": server.workspace_root,
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
        "initializationOptions": initialization_options,
    })
}

pub(crate) fn workspace_configuration_response(
    params: Option<&Value>,
    driver: &dyn LspServerDriver,
) -> Value {
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
                driver.configuration_response(section)
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn test_server() -> ResolvedLspServer {
        ResolvedLspServer {
            id: "test-lsp".to_string(),
            display_name: "Test LSP".to_string(),
            program: "unused-test-command".to_string(),
            args: Vec::new(),
            extensions: vec![".rs".to_string()],
            language_ids: vec!["rust".to_string()],
            operations: crate::types::LspQueryOperation::all().to_vec(),
            workspace_root: std::env::current_dir().unwrap(),
        }
    }

    #[test]
    fn initialize_params_carries_driver_initialization_options() {
        let params = initialize_params(
            &test_server(),
            serde_json::json!({ "files": { "watcher": "client" } }),
        );

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

    struct StubDriver;

    impl LspServerDriver for StubDriver {
        fn probe<'a>(
            &'a self,
            _command: &'a crate::driver::LspResolvedCommand,
            _host: Option<&'a dyn crate::host::LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, crate::driver::LspProbeOutcome> {
            futures::FutureExt::boxed(std::future::ready(crate::driver::LspProbeOutcome::Failed {
                message: String::new(),
            }))
        }

        fn repair<'a>(
            &'a self,
            _component: &'a crate::types::LspMissingComponent,
            _host: Option<&'a dyn crate::host::LspHostBackend>,
        ) -> futures::future::BoxFuture<'a, Result<(), crate::driver::LspRepairError>> {
            futures::FutureExt::boxed(std::future::ready(Err(
                crate::driver::LspRepairError::NotSupported,
            )))
        }

        fn configuration_response(&self, section: Option<&str>) -> Value {
            match section {
                Some("demo") => serde_json::json!({ "watcher": "client" }),
                _ => Value::Null,
            }
        }
    }

    #[test]
    fn workspace_configuration_maps_each_section_to_the_driver() {
        let params = serde_json::json!({
            "items": [{ "section": "demo" }, { "section": "other" }, {}]
        });

        let result = workspace_configuration_response(Some(&params), &StubDriver);

        assert_eq!(
            result,
            serde_json::json!([{ "watcher": "client" }, null, null])
        );
        assert_eq!(
            workspace_configuration_response(None, &StubDriver),
            serde_json::json!([])
        );
    }
}
