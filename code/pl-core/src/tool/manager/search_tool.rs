//! The deferred-catalog search tool injected into plans with hidden tools.

use std::future::Future;

use pl_protocol::{PureError, Result};
use schemars::JsonSchema;
use serde::Serialize;

use super::plan::ToolBinding;
use super::scope::{ToolExposure, ToolGroupId};
use crate::tool::{
    DynTool, StaticTool, StaticToolDefinition, ToolCallContext, ToolDirective, ToolName,
    ToolPolicy, ToolResult,
};

#[derive(Debug, Clone, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ToolSearchInput {
    /// Search terms matched against deferred tool names and descriptions.
    #[schemars(length(min = 1, max = 512))]
    query: String,
    /// Maximum number of matching tools to reveal for the next model step.
    #[schemars(range(min = 1, max = 20))]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolSearchMatch {
    name: String,
    description: String,
}

#[derive(Debug)]
struct ToolSearchTool {
    catalog_fingerprint: String,
    entries: Vec<ToolSearchMatch>,
}

impl StaticTool for ToolSearchTool {
    type Input = ToolSearchInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::builtin("tool_search"),
            "Search deferred tool metadata and reveal matching tools for the next model step.",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only().with_parallel_tool_calls()
    }

    fn execute(
        &self,
        input: Self::Input,
        _context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            let query = input.query.trim().to_ascii_lowercase();
            if query.is_empty() {
                return Err(PureError::ToolExecutionFailed {
                    tool: "tool_search".to_string(),
                    error: "query cannot be empty".to_string(),
                });
            }
            let terms = query.split_whitespace().collect::<Vec<_>>();
            let mut matches = self
                .entries
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.to_ascii_lowercase();
                    let description = entry.description.to_ascii_lowercase();
                    let score = terms.iter().fold(0_u64, |score, term| {
                        score
                            + u64::from(name.contains(term)) * 4
                            + u64::from(description.contains(term))
                    });
                    (score > 0).then_some((score, entry))
                })
                .collect::<Vec<_>>();
            matches.sort_by(|(left_score, left), (right_score, right)| {
                right_score
                    .cmp(left_score)
                    .then_with(|| left.name.cmp(&right.name))
            });
            let matches = matches
                .into_iter()
                .take(input.limit.unwrap_or(8).min(20))
                .map(|(_, entry)| entry.clone())
                .collect::<Vec<_>>();
            let mut result = ToolResult::json(serde_json::json!({
                "matches": matches,
                "revealedForNextModelStep": true,
            }))?;
            result.runtime_events.push(ToolDirective::RevealTools {
                catalog_fingerprint: self.catalog_fingerprint.clone(),
                tool_names: matches.into_iter().map(|entry| entry.name).collect(),
            });
            Ok(result)
        }
    }
}

pub(super) fn tool_search_binding(
    deferred: &[ToolBinding],
    catalog_fingerprint: &str,
) -> ToolBinding {
    let entries = deferred
        .iter()
        .map(|binding| ToolSearchMatch {
            name: binding.name().to_string(),
            description: tool_spec_description(&binding.spec).to_string(),
        })
        .collect();
    let tool: DynTool = ToolSearchTool {
        catalog_fingerprint: catalog_fingerprint.to_string(),
        entries,
    }
    .into();
    ToolBinding {
        spec: tool.definition().spec().clone(),
        execution: tool.execution(),
        programmatic_eligible: false,
        generation: deferred
            .iter()
            .map(|binding| binding.generation())
            .max()
            .unwrap_or_default(),
        group: ToolGroupId::new("tool-search"),
        exposure: ToolExposure::Direct,
        developer_instructions: None,
        tool,
    }
}

fn tool_spec_description(spec: &pl_protocol::ToolSpec) -> &str {
    match spec {
        pl_protocol::ToolSpec::Function { description, .. }
        | pl_protocol::ToolSpec::Custom { description, .. } => description,
        pl_protocol::ToolSpec::ProgrammaticToolCalling
        | pl_protocol::ToolSpec::WebSearch { .. } => "Provider-hosted tool",
    }
}
