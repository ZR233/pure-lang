use std::sync::Arc;

use anyhow::{Context, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{
    ReviewDesignReference, ReviewFinding, ReviewScope, ReviewVerdict, TaskCoordinator,
};
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::turn::ToolEffect;

const DEFAULT_FINDING_OFFSET: usize = 0;
const DEFAULT_FINDING_LIMIT: usize = 10;
const MAX_FINDING_LIMIT: usize = 50;
const MAX_REVIEW_ROUND_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadReviewRoundInput {
    /// Review round id from task_status.
    round_id: String,
    /// Zero-based finding offset.
    #[serde(default)]
    offset: Option<usize>,
    /// Maximum findings to return.
    #[serde(default)]
    #[schemars(range(min = 1, max = 50))]
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRoundDetail {
    id: String,
    round: u32,
    scope: ReviewScope,
    verdict: ReviewVerdict,
    summary: Option<String>,
    design_references: Vec<ReviewDesignReference>,
    findings: Vec<ReviewFinding>,
    offset: usize,
    limit: usize,
    total: usize,
    has_more: bool,
}

impl TaskCoordinator {
    pub(crate) fn read_review_round_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<ReadReviewRoundInput>::new(
            "read_review_round",
            "Read the full findings (including recommendations) of one review round, paginated and not truncated.",
        )
        .registered(move |input: ReadReviewRoundInput, _| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let aggregate = coordinator
                        .task_runtime
                        .aggregate(&thread_id)
                        .await
                        .context("active Task aggregate is not resident")?;
                    let offset = input.offset.unwrap_or(DEFAULT_FINDING_OFFSET);
                    let limit = input
                        .limit
                        .unwrap_or(DEFAULT_FINDING_LIMIT)
                        .clamp(1, MAX_FINDING_LIMIT);
                    let round = aggregate
                        .facts
                        .reviews
                        .into_iter()
                        .find(|candidate| candidate.id == input.round_id)
                        .with_context(|| {
                            format!("review round {} not found in active task", input.round_id)
                        })?;
                    if round.findings.is_empty() {
                        bail!("review round {} has no findings to read", input.round_id);
                    }
                    let total = round.findings.len();
                    let verdict = round.verdict();
                    let summary = round.summary().map(str::to_string);
                    let findings = round
                        .findings
                        .into_iter()
                        .skip(offset)
                        .take(limit)
                        .collect::<Vec<_>>();
                    let has_more = offset + findings.len() < total;
                    let detail = ReviewRoundDetail {
                        id: round.id,
                        round: round.round,
                        scope: round.scope,
                        verdict,
                        summary,
                        design_references: round.design_references,
                        findings,
                        offset,
                        limit,
                        total,
                        has_more,
                    };
                    ToolExecutionResult::<serde_json::Value>::json_with_budget(
                        detail,
                        /* max_output_tokens */ 16_000,
                        MAX_REVIEW_ROUND_OUTPUT_BYTES,
                    )
                    .map_err(anyhow::Error::from)
                }
            })
        .with_effect(ToolEffect::Read)
    }
}
