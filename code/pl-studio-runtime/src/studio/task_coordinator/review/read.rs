use std::sync::Arc;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use super::super::{
    ReviewDesignReference, ReviewFinding, ReviewScope, ReviewVerdict, TaskCoordinator,
};
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::turn::ToolEffect;

const DEFAULT_FINDING_OFFSET: usize = 0;
const DEFAULT_FINDING_LIMIT: usize = 10;
const MAX_FINDING_LIMIT: usize = 50;
const MAX_REVIEW_ROUND_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadReviewRoundInput {
    round_id: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
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
        RegisteredTool::from_typed_fallible_execution_result(
            "read_review_round",
            "Read the full findings (including recommendations) of one review round, paginated and not truncated.",
            strict_tool_input_schema([
                ToolInputSchemaField::required(
                    "roundId",
                    serde_json::json!({"type":"string"}),
                ),
                ToolInputSchemaField::optional(
                    "offset",
                    serde_json::json!({"type":"integer","minimum":0,"default":0}),
                ),
                ToolInputSchemaField::optional(
                    "limit",
                    serde_json::json!({"type":"integer","minimum":1,"maximum":50,"default":10}),
                ),
            ]),
            move |input: ReadReviewRoundInput, _| {
                let coordinator = coordinator.clone();
                let thread_id = thread_id.clone();
                async move {
                    let run = coordinator
                        .store
                        .read_active_task_run_for_root_thread(&thread_id)
                        .await?;
                    let offset = input.offset.unwrap_or(DEFAULT_FINDING_OFFSET);
                    let limit = input
                        .limit
                        .unwrap_or(DEFAULT_FINDING_LIMIT)
                        .clamp(1, MAX_FINDING_LIMIT);
                    let round = coordinator
                        .store
                        .list_review_rounds(&run.id)
                        .await?
                        .into_iter()
                        .find(|candidate| candidate.id == input.round_id)
                        .with_context(|| {
                            format!("review round {} not found in active task", input.round_id)
                        })?;
                    if round.findings.is_empty() {
                        bail!("review round {} has no findings to read", input.round_id);
                    }
                    let total = round.findings.len();
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
                        verdict: round.verdict,
                        summary: round.summary,
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
            },
        )
        .with_effect(ToolEffect::Read)
    }
}
