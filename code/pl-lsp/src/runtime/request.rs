use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use lsp_types::request::{
    CallHierarchyIncomingCalls, CallHierarchyOutgoingCalls, CallHierarchyPrepare,
    DocumentSymbolRequest, GotoDefinition, GotoImplementation, HoverRequest, References,
    Request as LspRequest, WorkspaceSymbolRequest,
};
use lsp_types::{
    CallHierarchyIncomingCallsParams, CallHierarchyItem, CallHierarchyOutgoingCallsParams,
    CallHierarchyPrepareParams, DocumentSymbolParams, GotoDefinitionParams, HoverParams,
    PartialResultParams, Position, ReferenceContext, ReferenceParams, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams, WorkspaceSymbolParams,
};
use serde::Serialize;
use serde_json::Value;

use crate::client::uri::file_uri;
use crate::client::{LspClient, with_content_modified_retries};
use crate::query::{LspDiagnostic, LspQuery, LspQueryOperation};

use super::{LspResult, LspRuntimeError, ResolvedLspServer};

const SEMANTIC_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SEMANTIC_RETRY_DELAYS: [Duration; 4] = [
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
];

pub(crate) async fn request_for_query(
    client: &Arc<LspClient>,
    query: &LspQuery,
) -> LspResult<Value> {
    let (method, params) = method_and_params(query)?;
    let mut value = request_with_content_modified_retries(client, &method, &params).await?;
    if query.operation.requires_position() && is_empty_semantic_result(&value) {
        client.wait_until_idle(SEMANTIC_STARTUP_TIMEOUT).await;
        for delay in SEMANTIC_RETRY_DELAYS {
            tokio::time::sleep(delay).await;
            value = request_with_content_modified_retries(client, &method, &params).await?;
            if !is_empty_semantic_result(&value) {
                break;
            }
        }
    }
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
        let item = serde_json::from_value::<CallHierarchyItem>(item.clone())?;
        let (method, params) = match query.operation {
            LspQueryOperation::IncomingCalls => {
                serialize_params::<CallHierarchyIncomingCalls>(CallHierarchyIncomingCallsParams {
                    item,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                })?
            }
            LspQueryOperation::OutgoingCalls => {
                serialize_params::<CallHierarchyOutgoingCalls>(CallHierarchyOutgoingCallsParams {
                    item,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                })?
            }
            _ => unreachable!(),
        };
        value = with_content_modified_retries(|| {
            let client = client.clone();
            let method = method.clone();
            let params = params.clone();
            async move { client.request(&method, params).await }
        })
        .await?;
    }
    Ok(value)
}

async fn request_with_content_modified_retries(
    client: &Arc<LspClient>,
    method: &str,
    params: &Value,
) -> LspResult<Value> {
    with_content_modified_retries(|| {
        let client = client.clone();
        let method = method.to_string();
        let params = params.clone();
        async move { client.request(&method, params).await }
    })
    .await
}

fn is_empty_semantic_result(value: &Value) -> bool {
    value.is_null()
        || value.as_array().is_some_and(Vec::is_empty)
        || value.get("contents").is_some_and(|contents| {
            contents.is_null()
                || contents.as_str().is_some_and(str::is_empty)
                || contents.as_array().is_some_and(Vec::is_empty)
                || contents
                    .get("value")
                    .and_then(Value::as_str)
                    .is_some_and(str::is_empty)
        })
}

pub(crate) fn method_and_params(query: &LspQuery) -> LspResult<(String, Value)> {
    match query.operation {
        LspQueryOperation::Hover => serialize_params::<HoverRequest>(HoverParams {
            text_document_position_params: text_document_position(query)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
        }),
        LspQueryOperation::GoToDefinition => {
            serialize_params::<GotoDefinition>(definition_params(query)?)
        }
        LspQueryOperation::FindReferences => serialize_params::<References>(ReferenceParams {
            text_document_position: text_document_position(query)?,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        }),
        LspQueryOperation::DocumentSymbol => {
            serialize_params::<DocumentSymbolRequest>(DocumentSymbolParams {
                text_document: text_document(query)?,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
        }
        LspQueryOperation::WorkspaceSymbol => {
            serialize_params::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
                query: query.query.clone().unwrap_or_default(),
                ..WorkspaceSymbolParams::default()
            })
        }
        LspQueryOperation::GoToImplementation => {
            serialize_params::<GotoImplementation>(definition_params(query)?)
        }
        LspQueryOperation::PrepareCallHierarchy
        | LspQueryOperation::IncomingCalls
        | LspQueryOperation::OutgoingCalls => {
            serialize_params::<CallHierarchyPrepare>(CallHierarchyPrepareParams {
                text_document_position_params: text_document_position(query)?,
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
        }
        LspQueryOperation::Diagnostics => unreachable!("diagnostics does not call LSP server"),
    }
}

fn definition_params(query: &LspQuery) -> LspResult<GotoDefinitionParams> {
    Ok(GotoDefinitionParams {
        text_document_position_params: text_document_position(query)?,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    })
}

fn text_document_position(query: &LspQuery) -> LspResult<TextDocumentPositionParams> {
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
    Ok(TextDocumentPositionParams {
        text_document: text_document(query)?,
        position: Position::new(line - 1, character - 1),
    })
}

fn text_document(query: &LspQuery) -> LspResult<TextDocumentIdentifier> {
    let path = query
        .file_path
        .as_deref()
        .ok_or_else(|| LspRuntimeError::InvalidQuery("filePath is required".to_string()))?;
    Ok(TextDocumentIdentifier {
        uri: file_uri(path)?,
    })
}

fn serialize_params<R>(params: R::Params) -> LspResult<(String, Value)>
where
    R: LspRequest,
    R::Params: Serialize,
{
    Ok((R::METHOD.to_string(), serde_json::to_value(params)?))
}

pub(crate) fn extensions_for_language(
    server: &ResolvedLspServer,
    language_id: &str,
) -> Vec<String> {
    if server.language_ids.len() == server.extensions.len() {
        server
            .language_ids
            .iter()
            .zip(server.extensions.iter())
            .filter(|(candidate, _)| candidate.as_str() == language_id)
            .map(|(_, extension)| extension.clone())
            .collect()
    } else if server
        .language_ids
        .iter()
        .any(|candidate| candidate == language_id)
    {
        server.extensions.clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::LspQueryOperation;

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
