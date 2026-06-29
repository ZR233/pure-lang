use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde_json::Value;

use crate::client::{LspClient, with_content_modified_retries};
use crate::server_definition::LspServerDefinition;
use crate::types::{LspDiagnostic, LspQuery, LspQueryOperation, LspResult, LspRuntimeError};
use crate::uri::path_to_file_uri;

pub(crate) async fn request_for_query(
    client: &Arc<LspClient>,
    query: &LspQuery,
) -> LspResult<Value> {
    let (method, params) = method_and_params(query)?;
    let mut value = with_content_modified_retries(|| {
        let client = client.clone();
        let method = method.clone();
        let params = params.clone();
        async move { client.request(&method, params).await }
    })
    .await?;
    if matches!(
        query.operation,
        LspQueryOperation::IncomingCalls | LspQueryOperation::OutgoingCalls
    ) {
        let Some(items) = value.as_array() else {
            return Ok(Value::Array(Vec::new()));
        };
        let Some(item) = items.first() else {
            return Ok(Value::Array(Vec::new()));
        };
        let method = match query.operation {
            LspQueryOperation::IncomingCalls => "callHierarchy/incomingCalls",
            LspQueryOperation::OutgoingCalls => "callHierarchy/outgoingCalls",
            _ => unreachable!(),
        };
        value = with_content_modified_retries(|| {
            let client = client.clone();
            let item = item.clone();
            async move {
                client
                    .request(method, serde_json::json!({ "item": item }))
                    .await
            }
        })
        .await?;
    }
    Ok(value)
}

pub(crate) fn method_and_params(query: &LspQuery) -> LspResult<(String, Value)> {
    let uri = query
        .file_path
        .as_deref()
        .map(path_to_file_uri)
        .unwrap_or_default();
    let position = if query.operation.requires_position() {
        let line = query
            .line
            .ok_or_else(|| LspRuntimeError::InvalidQuery("line is required".to_string()))?;
        let character = query
            .character
            .ok_or_else(|| LspRuntimeError::InvalidQuery("character is required".to_string()))?;
        if line == 0 || character == 0 {
            return Err(LspRuntimeError::InvalidQuery(
                "line and character are 1-based and must be positive".to_string(),
            ));
        }
        Some(serde_json::json!({
            "line": line - 1,
            "character": character - 1,
        }))
    } else {
        None
    };
    let text_document = serde_json::json!({ "uri": uri });
    let output = match query.operation {
        LspQueryOperation::Hover => (
            "textDocument/hover",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::GoToDefinition => (
            "textDocument/definition",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::FindReferences => (
            "textDocument/references",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::DocumentSymbol => (
            "textDocument/documentSymbol",
            serde_json::json!({ "textDocument": text_document }),
        ),
        LspQueryOperation::WorkspaceSymbol => (
            "workspace/symbol",
            serde_json::json!({ "query": query.query.clone().unwrap_or_default() }),
        ),
        LspQueryOperation::GoToImplementation => (
            "textDocument/implementation",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::PrepareCallHierarchy
        | LspQueryOperation::IncomingCalls
        | LspQueryOperation::OutgoingCalls => (
            "textDocument/prepareCallHierarchy",
            serde_json::json!({ "textDocument": text_document, "position": position }),
        ),
        LspQueryOperation::Diagnostics => unreachable!("diagnostics does not call LSP server"),
    };
    Ok((output.0.to_string(), output.1))
}

pub(crate) fn extensions_for_language(
    definition: &LspServerDefinition,
    language_id: &str,
) -> Vec<String> {
    if definition.language_ids.len() == definition.extensions.len() {
        definition
            .language_ids
            .iter()
            .zip(definition.extensions.iter())
            .filter(|(candidate, _)| candidate.as_str() == language_id)
            .map(|(_, extension)| extension.clone())
            .collect()
    } else if definition
        .language_ids
        .iter()
        .any(|candidate| candidate == language_id)
    {
        definition.extensions.clone()
    } else {
        Vec::new()
    }
}

pub(crate) fn diagnostic_counts(
    diagnostics: &HashMap<String, Vec<LspDiagnostic>>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for values in diagnostics.values() {
        for diagnostic in values {
            *counts.entry(diagnostic.server_id.clone()).or_insert(0) += 1;
        }
    }
    counts
}

pub(crate) fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LspQueryOperation;

    #[test]
    fn position_query_uses_one_based_input() {
        let query = LspQuery {
            operation: LspQueryOperation::Hover,
            file_path: Some(std::env::temp_dir().join("pure-lsp-position/src/lib.rs")),
            line: Some(7),
            character: Some(3),
            query: None,
            max_results: None,
            language_id: None,
        };

        let (_, params) = method_and_params(&query).unwrap();

        assert_eq!(params["position"]["line"], serde_json::json!(6));
        assert_eq!(params["position"]["character"], serde_json::json!(2));
    }
}
