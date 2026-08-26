use std::sync::Arc;

use anyhow::{Context, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::TaskCoordinator;
use crate::tool::{LocalTool, ToolResult, TypedTool};
use crate::turn::ToolEffect;

const DEFAULT_COVERAGE_LIMIT: usize = 50;
const MAX_COVERAGE_LIMIT: usize = 200;
const MAX_COVERAGE_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadReviewFileCoverageInput {
    round_id: String,
    diagnostics_revision: u64,
    category: ReviewCoverageCategory,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ReviewCoverageCategory {
    All,
    Reviewed,
    Missing,
    Unreviewed,
    Duplicate,
    Extra,
    Invalid,
    Violations,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewFileCoveragePage {
    round_id: String,
    diagnostics_revision: u64,
    category: ReviewCoverageCategory,
    expected_count: usize,
    reviewed_count: usize,
    complete: bool,
    items: Vec<serde_json::Value>,
    offset: usize,
    limit: usize,
    total: usize,
    has_more: bool,
}

impl TaskCoordinator {
    pub(crate) fn read_review_file_coverage_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
    ) -> LocalTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        TypedTool::<ReadReviewFileCoverageInput>::new(
            "read_review_file_coverage",
            "按 revision 和分类分页读取 ReviewRound 的冻结文件覆盖及完整拒绝诊断。",
        )
        .handler(move |input: ReadReviewFileCoverageInput, _| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            async move {
                let round = coordinator
                    .task_runtime
                    .aggregate(&thread_id)
                    .await
                    .context("active Task aggregate is not resident")?
                    .facts
                    .reviews
                    .into_iter()
                    .find(|round| round.id == input.round_id)
                    .context("review round not found in active task")?;
                let coverage = round
                    .file_reviews
                    .as_ref()
                    .context("review round has not published file coverage")?;
                ensure!(
                    coverage.diagnostics_revision == input.diagnostics_revision,
                    "review coverage diagnostics revision is stale; read task_status and retry"
                );
                let diagnostics = coverage.last_diagnostics.as_ref();
                let items = match input.category {
                    ReviewCoverageCategory::All => coverage
                        .files
                        .iter()
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    ReviewCoverageCategory::Reviewed => coverage
                        .files
                        .iter()
                        .filter(|file| file.reviewed)
                        .map(serde_json::to_value)
                        .collect::<Result<Vec<_>, _>>()?,
                    ReviewCoverageCategory::Missing => {
                        string_values(diagnostics.map(|value| value.missing_files.as_slice()))
                    }
                    ReviewCoverageCategory::Unreviewed => {
                        string_values(diagnostics.map(|value| value.unreviewed_files.as_slice()))
                    }
                    ReviewCoverageCategory::Duplicate => {
                        string_values(diagnostics.map(|value| value.duplicate_files.as_slice()))
                    }
                    ReviewCoverageCategory::Extra => {
                        string_values(diagnostics.map(|value| value.extra_files.as_slice()))
                    }
                    ReviewCoverageCategory::Invalid => diagnostics
                        .map(|value| {
                            value
                                .invalid_paths
                                .iter()
                                .map(serde_json::to_value)
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()?
                        .unwrap_or_default(),
                    ReviewCoverageCategory::Violations => diagnostics
                        .map(|value| {
                            value
                                .violations
                                .iter()
                                .map(serde_json::to_value)
                                .collect::<Result<Vec<_>, _>>()
                        })
                        .transpose()?
                        .unwrap_or_default(),
                };
                let offset = input.offset.unwrap_or_default();
                let limit = input
                    .limit
                    .unwrap_or(DEFAULT_COVERAGE_LIMIT)
                    .clamp(1, MAX_COVERAGE_LIMIT);
                let total = items.len();
                let items = items
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>();
                let has_more = offset.saturating_add(items.len()) < total;
                let page = ReviewFileCoveragePage {
                    round_id: round.id,
                    diagnostics_revision: coverage.diagnostics_revision,
                    category: input.category,
                    expected_count: coverage.files.len(),
                    reviewed_count: coverage.reviewed_count(),
                    complete: coverage.is_complete(),
                    items,
                    offset,
                    limit,
                    total,
                    has_more,
                };
                ToolResult::json_with_budget(
                    page,
                    /* max_output_tokens */ 16_000,
                    MAX_COVERAGE_OUTPUT_BYTES,
                )
                .map_err(anyhow::Error::from)
            }
        })
        .with_effect(ToolEffect::Read)
    }
}

fn string_values(items: Option<&[String]>) -> Vec<serde_json::Value> {
    items
        .unwrap_or_default()
        .iter()
        .cloned()
        .map(serde_json::Value::String)
        .collect()
}
