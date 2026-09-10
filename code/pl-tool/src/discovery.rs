//! Model-callable discovery over the Thread's read-only opaque tool directory.
use pl_core::context::{ContextContent, OpaquePayload};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, RegistryError, Tool, ToolError},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Matches stable tool identities without parsing provider declaration formats.
#[derive(Debug)]
pub struct DiscoverTools;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearchInput {
    query: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResult {
    tool_ids: Vec<String>,
}

/// Creates a Thread-owned discovery tool using a declaration encoded by the host/model boundary.
///
/// # Errors
/// Propagates invalid registration identity.
pub fn registration(declaration: OpaquePayload) -> Result<Registration, RegistryError> {
    Ok(
        Registration::new("discover_tools".into(), declaration, DiscoverTools)?
            .with_tool_discovery(),
    )
}

impl Tool for DiscoverTools {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let request: SearchInput = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let query = request.query.trim().to_lowercase();
        let ids = context
            .catalog
            .iter()
            .filter(|tool| {
                tool.tool_id != "discover_tools" && tool.tool_id.to_lowercase().contains(&query)
            })
            .take(20)
            .map(|tool| tool.tool_id.clone())
            .collect::<Vec<_>>();
        let result = serde_json::to_string(&SearchResult {
            tool_ids: ids.clone(),
        })
        .map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            OpaquePayload::text(result.clone()),
            vec![ContextContent::Text {
                text: Arc::from(result),
            }],
        )
        .with_revealed_tools(ids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn discovery_reveals_matching_ids_without_decoding_opaque_declarations() {
        let context = CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Default::default(),
            extension_sequence: 0,
            catalog: vec![
                pl_core::model::ModelToolDeclaration {
                    tool_id: "read_file".into(),
                    declaration: OpaquePayload::new("future.schema", 99, "not JSON").unwrap(),
                },
                pl_core::model::ModelToolDeclaration {
                    tool_id: "write_file".into(),
                    declaration: OpaquePayload::text("another schema"),
                },
            ]
            .into(),
        };
        let output = DiscoverTools
            .execute(
                OpaquePayload::new("application/json", 1, r#"{"query":"read"}"#).unwrap(),
                context,
            )
            .await
            .unwrap();
        assert_eq!(output.revealed_tools(), &["read_file"]);
        assert_eq!(output.control(), pl_core::tool::ToolControl::Continue);
        assert!(output.payload().content().contains("read_file"));
        assert!(!output.payload().content().contains("write_file"));
    }
}
