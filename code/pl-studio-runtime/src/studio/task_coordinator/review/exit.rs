use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{
    AgentReview, ReviewDesignReference, ReviewExitDiagnostics, ReviewExitViolation,
    ReviewFileCoverage, ReviewFileReview, ReviewFinding, ReviewInvalidPath, ReviewRoundRecord,
    ReviewVerdict, TaskCoordinator, TaskPlannerWakeRequest, TaskPlannerWakeSource,
};
use super::trace::inspect_review_trace;
use crate::AgentRuntimeHandle;
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::turn::ToolEffect;

const DIAGNOSTIC_PREVIEW_LIMIT: usize = 20;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewExitInput {
    verdict: ReviewExitVerdict,
    /// 简洁的整体审查结论。
    summary: String,
    /// 审查期间实际读取的 design 章节。
    design_references: Vec<ReviewDesignReference>,
    /// 所有确定、离散、可执行的未解决 finding；pass 时必须为空。
    findings: Vec<ReviewFinding>,
    /// 冻结 changed-files 清单中每个文件的审查声明。
    file_reviews: Vec<ReviewFileReviewInput>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewFileReviewInput {
    /// 规范化的仓库相对路径，必须与 ReviewRound 冻结清单精确匹配。
    path: String,
    /// 仅在结合完整 diff 审查该文件后设为 true。
    reviewed: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ReviewExitVerdict {
    Pass,
    ChangesRequired,
    Blocked,
}

#[derive(Debug, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
enum ReviewExitOutcome {
    Accepted {
        round: Box<ReviewRoundRecord>,
        coverage: ReviewCoverageSummary,
    },
    Rejected {
        code: &'static str,
        recoverable: bool,
        message: &'static str,
        diagnostics_revision: u64,
        coverage: Box<ReviewCoverageDiagnosticsOutput>,
        violations: DiagnosticPage<ReviewExitViolation>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCoverageSummary {
    expected_count: usize,
    reviewed_count: usize,
    complete: bool,
    diagnostics_revision: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCoverageDiagnosticsOutput {
    expected_count: usize,
    submitted_count: usize,
    reviewed_count: usize,
    missing_files: DiagnosticPage<String>,
    unreviewed_files: DiagnosticPage<String>,
    duplicate_files: DiagnosticPage<String>,
    extra_files: DiagnosticPage<String>,
    invalid_paths: DiagnosticPage<ReviewInvalidPath>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticPage<T> {
    items: Vec<T>,
    total: usize,
    has_more: bool,
}

struct ReviewExitValidation {
    review: AgentReview,
    file_reviews: ReviewFileCoverage,
}

impl TaskCoordinator {
    pub(crate) fn review_exit_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: Option<AgentRuntimeHandle>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        FunctionToolDefinition::<ReviewExitInput>::new(
            "review_exit",
            "提交只读审查结论和每个 changed file 的覆盖声明；门禁失败会返回完整分类诊断并允许同一 Turn 重试。",
        )
        .registered(move |input: ReviewExitInput, context| {
            let coordinator = coordinator.clone();
            let thread_id = thread_id.clone();
            let runtime = runtime.clone();
            async move {
                let root_agent_id = crate::studio::agent_host::root_agent_id(&thread_id);
                let reviewer = context
                    .active_subagent
                    .as_ref()
                    .filter(|agent| {
                        agent.role == "reviewer"
                            && agent.depth == 1
                            && agent.parent_id.as_deref() == Some(root_agent_id.as_str())
                    })
                    .context("review_exit requires the harness-owned depth-1 reviewer")?;
                let aggregate = coordinator
                    .task_runtime
                    .aggregate(&thread_id)
                    .await
                    .context("active Task aggregate is not resident")?;
                let run = aggregate.facts.run;
                let round = aggregate
                    .facts
                    .reviews
                    .into_iter()
                    .find(|round| round.reviewer_thread_id() == Some(reviewer.id.as_str()))
                    .context("reviewer has no canonical ReviewRound")?;
                ensure!(
                    round.task_run_id == run.id && round.verdict() == ReviewVerdict::Pending,
                    "reviewer ReviewRound is not pending in the active Task"
                );
                let frozen = round
                    .file_reviews
                    .as_ref()
                    .context("pending review has no frozen file coverage snapshot")?;
                let trace =
                    inspect_review_trace(&context.parent_session, context.workspace.root()).await?;
                let validation = validate_review_exit(
                    input,
                    &trace.read_design,
                    &frozen.expected_paths(),
                    trace.violations,
                );
                if validation
                    .file_reviews
                    .last_diagnostics
                    .as_ref()
                    .is_some_and(|diagnostics| !diagnostics.is_empty())
                {
                    let round = coordinator
                        .task_runtime
                        .record_review_rejection(
                            &thread_id,
                            &reviewer.id,
                            validation.file_reviews,
                        )
                        .await?;
                    let outcome = rejected_outcome(&round)?;
                    let output = serde_json::to_string(&outcome)?;
                    return Ok::<_, anyhow::Error>(ToolExecutionResult::<serde_json::Value>::failure(
                        output,
                    ));
                }

                let round = coordinator
                    .task_runtime
                    .complete_task_review(
                        &thread_id,
                        &reviewer.id,
                        validation.review,
                        validation.file_reviews,
                    )
                    .await?;
                if let Some(runtime) = runtime {
                    let wake = TaskPlannerWakeRequest {
                        task_run_id: round.task_run_id.clone(),
                        root_thread_id: thread_id.clone(),
                        source: TaskPlannerWakeSource::Review {
                            review_round_id: round.id.clone(),
                            scope: round.scope,
                        },
                    };
                    if let Err(error) = crate::studio::agent_host::materialize_task_planner_wake(
                        &runtime,
                        &coordinator.task_runtime(),
                        &wake,
                    )
                    .await
                    {
                        tracing::warn!(
                            review_round_id = %round.id,
                            error_bytes = error.to_string().len(),
                            "Task review committed; Planner wake remains pending"
                        );
                    }
                }
                let coverage = coverage_summary(&round)?;
                let outcome = ReviewExitOutcome::Accepted {
                    round: Box::new(round),
                    coverage,
                };
                let output = serde_json::to_string(&outcome)?;
                Ok::<_, anyhow::Error>(
                    ToolExecutionResult::<serde_json::Value>::success(output).ending_turn(),
                )
            }
        })
        .with_effect(ToolEffect::Read)
    }
}

fn validate_review_exit(
    input: ReviewExitInput,
    read_design: &BTreeMap<String, String>,
    expected_files: &[String],
    mut violations: Vec<ReviewExitViolation>,
) -> ReviewExitValidation {
    let verdict = match input.verdict {
        ReviewExitVerdict::Pass => ReviewVerdict::Pass,
        ReviewExitVerdict::ChangesRequired => ReviewVerdict::ChangesRequired,
        ReviewExitVerdict::Blocked => ReviewVerdict::Blocked,
    };
    let summary = input.summary.trim().to_string();
    if summary.is_empty() {
        push_violation(
            &mut violations,
            "summaryMissing",
            "review summary 不能为空",
            Some("summary"),
        );
    }
    match verdict {
        ReviewVerdict::Pass if !input.findings.is_empty() => push_violation(
            &mut violations,
            "passHasFindings",
            "pass 不能包含未解决 finding",
            Some("findings"),
        ),
        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked if input.findings.is_empty() => {
            push_violation(
                &mut violations,
                "findingMissing",
                "changesRequired 和 blocked 必须包含具体 finding",
                Some("findings"),
            );
        }
        ReviewVerdict::Pass
        | ReviewVerdict::ChangesRequired
        | ReviewVerdict::Blocked
        | ReviewVerdict::Pending
        | ReviewVerdict::Failed => {}
    }
    if input.design_references.is_empty() {
        push_violation(
            &mut violations,
            "designReferenceMissing",
            "review_exit 至少需要一个实际读取的 design reference",
            Some("designReferences"),
        );
    }
    let top_paths = input
        .design_references
        .iter()
        .map(|reference| reference.path.clone())
        .collect::<BTreeSet<_>>();
    for (index, reference) in input.design_references.iter().enumerate() {
        validate_reference(
            reference,
            read_design,
            &format!("designReferences[{index}]"),
            &mut violations,
        );
    }
    for (index, finding) in input.findings.iter().enumerate() {
        let location = format!("findings[{index}]");
        if finding.title.trim().is_empty() || finding.body.trim().is_empty() {
            push_violation(
                &mut violations,
                "findingContentMissing",
                "finding 的 title 和 body 必须非空",
                Some(&location),
            );
        }
        if finding.recommendation.trim().is_empty() {
            push_violation(
                &mut violations,
                "recommendationMissing",
                "finding 必须给出说明改什么以及为什么的具体 recommendation",
                Some(&location),
            );
        }
        let design_claim = finding.title.to_ascii_lowercase().contains("design")
            || finding.body.to_ascii_lowercase().contains("design")
            || finding.title.contains("设计")
            || finding.body.contains("设计");
        if design_claim && finding.design_references.is_empty() {
            push_violation(
                &mut violations,
                "findingDesignReferenceMissing",
                "设计一致性 finding 必须引用实际读取的 design 章节",
                Some(&location),
            );
        }
        for (reference_index, reference) in finding.design_references.iter().enumerate() {
            let reference_location = format!("{location}.designReferences[{reference_index}]");
            validate_reference(reference, read_design, &reference_location, &mut violations);
            if !top_paths.contains(&reference.path) {
                push_violation(
                    &mut violations,
                    "findingReferenceNotTopLevel",
                    "finding 的 design reference 必须同时出现在顶层 designReferences",
                    Some(&reference_location),
                );
            }
        }
    }

    let review = AgentReview {
        verdict,
        summary,
        design_references: input.design_references,
        findings: input.findings,
    };
    let file_reviews = validate_file_reviews(expected_files, input.file_reviews, violations);
    ReviewExitValidation {
        review,
        file_reviews,
    }
}

fn validate_file_reviews(
    expected_files: &[String],
    submitted: Vec<ReviewFileReviewInput>,
    violations: Vec<ReviewExitViolation>,
) -> ReviewFileCoverage {
    let expected = expected_files.iter().cloned().collect::<BTreeSet<_>>();
    let submitted_count = submitted.len();
    let mut raw_counts = BTreeMap::<String, usize>::new();
    let mut valid = BTreeMap::<String, Vec<bool>>::new();
    let mut invalid_paths = BTreeSet::<(String, String)>::new();
    for file in submitted {
        *raw_counts.entry(file.path.clone()).or_default() += 1;
        match normalize_review_path(&file.path) {
            Ok(path) => valid.entry(path).or_default().push(file.reviewed),
            Err(reason) => {
                invalid_paths.insert((file.path, reason));
            }
        }
    }

    let missing_files = expected
        .iter()
        .filter(|path| !valid.contains_key(*path))
        .cloned()
        .collect::<Vec<_>>();
    let unreviewed_files = expected
        .iter()
        .filter(|path| {
            valid
                .get(*path)
                .is_some_and(|reviews| reviews.iter().any(|reviewed| !reviewed))
        })
        .cloned()
        .collect::<Vec<_>>();
    let duplicate_files = raw_counts
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect::<Vec<_>>();
    let extra_files = valid
        .keys()
        .filter(|path| !expected.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    let invalid_paths = invalid_paths
        .into_iter()
        .map(|(path, reason)| ReviewInvalidPath { path, reason })
        .collect::<Vec<_>>();
    let files = expected
        .into_iter()
        .map(|path| {
            let reviewed = valid
                .get(&path)
                .is_some_and(|reviews| reviews.as_slice() == [true]);
            ReviewFileReview { path, reviewed }
        })
        .collect();
    ReviewFileCoverage {
        version: super::super::REVIEW_FILE_COVERAGE_VERSION,
        diagnostics_revision: 0,
        files,
        last_diagnostics: Some(ReviewExitDiagnostics {
            submitted_count,
            missing_files,
            unreviewed_files,
            duplicate_files,
            extra_files,
            invalid_paths,
            violations,
        }),
    }
}

fn normalize_review_path(raw: &str) -> std::result::Result<String, String> {
    if raw.is_empty() {
        return Err("路径不能为空".to_string());
    }
    if raw.trim() != raw {
        return Err("路径不能包含首尾空白".to_string());
    }
    if raw.contains('\\') {
        return Err("路径必须使用 `/` 分隔符".to_string());
    }
    let path = Path::new(raw);
    let components = path.components().collect::<Vec<_>>();
    if path.is_absolute()
        || components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("路径必须是仓库内的规范相对路径，且不能包含 `.` 或 `..`".to_string());
    }
    let normalized = components
        .iter()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized != raw {
        return Err("路径不是规范形式".to_string());
    }
    Ok(normalized)
}

fn validate_reference(
    reference: &ReviewDesignReference,
    read_design: &BTreeMap<String, String>,
    location: &str,
    violations: &mut Vec<ReviewExitViolation>,
) {
    let path = Path::new(&reference.path);
    let valid_path = !reference.path.contains('\\')
        && !path.is_absolute()
        && path.components().count() >= 2
        && matches!(path.components().next(), Some(Component::Normal(part)) if part == "design")
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && read_design.contains_key(&reference.path);
    if !valid_path {
        push_violation(
            violations,
            "designReferenceInvalid",
            "design reference 必须指向实际读取的规范 design/** 文件",
            Some(location),
        );
    }
    if reference.section.trim().is_empty() {
        push_violation(
            violations,
            "designSectionMissing",
            "design reference section 不能为空",
            Some(location),
        );
    } else if valid_path
        && !read_design
            .get(&reference.path)
            .is_some_and(|content| content.contains(reference.section.trim()))
    {
        push_violation(
            violations,
            "designSectionNotRead",
            "design reference section 不在实际 read_file 内容中",
            Some(location),
        );
    }
}

fn push_violation(
    violations: &mut Vec<ReviewExitViolation>,
    code: &str,
    message: &str,
    location: Option<&str>,
) {
    violations.push(ReviewExitViolation {
        code: code.to_string(),
        message: message.to_string(),
        location: location.map(str::to_string),
    });
}

fn rejected_outcome(round: &ReviewRoundRecord) -> Result<ReviewExitOutcome> {
    let coverage = round
        .file_reviews
        .as_ref()
        .context("rejected review has no persisted file coverage")?;
    let diagnostics = coverage
        .last_diagnostics
        .as_ref()
        .context("rejected review has no persisted diagnostics")?;
    Ok(ReviewExitOutcome::Rejected {
        code: "reviewExitRejected",
        recoverable: true,
        message: "review_exit 门禁未通过；请根据全部诊断修正后在同一 Turn 重试",
        diagnostics_revision: coverage.diagnostics_revision,
        coverage: Box::new(ReviewCoverageDiagnosticsOutput {
            expected_count: coverage.files.len(),
            submitted_count: diagnostics.submitted_count,
            reviewed_count: coverage.reviewed_count(),
            missing_files: diagnostic_page(&diagnostics.missing_files),
            unreviewed_files: diagnostic_page(&diagnostics.unreviewed_files),
            duplicate_files: diagnostic_page(&diagnostics.duplicate_files),
            extra_files: diagnostic_page(&diagnostics.extra_files),
            invalid_paths: diagnostic_page(&diagnostics.invalid_paths),
        }),
        violations: diagnostic_page(&diagnostics.violations),
    })
}

fn coverage_summary(round: &ReviewRoundRecord) -> Result<ReviewCoverageSummary> {
    let coverage = round
        .file_reviews
        .as_ref()
        .context("accepted review has no persisted file coverage")?;
    Ok(ReviewCoverageSummary {
        expected_count: coverage.files.len(),
        reviewed_count: coverage.reviewed_count(),
        complete: coverage.is_complete(),
        diagnostics_revision: coverage.diagnostics_revision,
    })
}

fn diagnostic_page<T: Clone>(items: &[T]) -> DiagnosticPage<T> {
    DiagnosticPage {
        items: items
            .iter()
            .take(DIAGNOSTIC_PREVIEW_LIMIT)
            .cloned()
            .collect(),
        total: items.len(),
        has_more: items.len() > DIAGNOSTIC_PREVIEW_LIMIT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_design() -> BTreeMap<String, String> {
        BTreeMap::from([(
            "design/guide.md".to_string(),
            "# Review design\n\n## Completion gate\n".to_string(),
        )])
    }

    fn reference(section: &str) -> ReviewDesignReference {
        ReviewDesignReference {
            path: "design/guide.md".to_string(),
            section: section.to_string(),
        }
    }

    fn pass_input(file_reviews: Vec<ReviewFileReviewInput>) -> ReviewExitInput {
        ReviewExitInput {
            verdict: ReviewExitVerdict::Pass,
            summary: "已完整审查".to_string(),
            design_references: vec![reference("Review design")],
            findings: Vec::new(),
            file_reviews,
        }
    }

    #[test]
    fn file_coverage_reports_every_category_in_one_rejection() {
        let expected = vec![
            "src/a.rs".to_string(),
            "src/b.rs".to_string(),
            "src/c.rs".to_string(),
            "src/d.rs".to_string(),
        ];
        let validation = validate_review_exit(
            pass_input(vec![
                ReviewFileReviewInput {
                    path: "src/a.rs".to_string(),
                    reviewed: true,
                },
                ReviewFileReviewInput {
                    path: "src/a.rs".to_string(),
                    reviewed: true,
                },
                ReviewFileReviewInput {
                    path: "src/b.rs".to_string(),
                    reviewed: false,
                },
                ReviewFileReviewInput {
                    path: "src/d.rs".to_string(),
                    reviewed: true,
                },
                ReviewFileReviewInput {
                    path: "src/extra.rs".to_string(),
                    reviewed: true,
                },
                ReviewFileReviewInput {
                    path: "../escape.rs".to_string(),
                    reviewed: true,
                },
            ]),
            &read_design(),
            &expected,
            Vec::new(),
        );
        let diagnostics = validation.file_reviews.last_diagnostics.as_ref().unwrap();

        assert_eq!(diagnostics.missing_files, vec!["src/c.rs"]);
        assert_eq!(diagnostics.unreviewed_files, vec!["src/b.rs"]);
        assert_eq!(diagnostics.duplicate_files, vec!["src/a.rs"]);
        assert_eq!(diagnostics.extra_files, vec!["src/extra.rs"]);
        assert_eq!(diagnostics.invalid_paths[0].path, "../escape.rs");
        assert_eq!(validation.file_reviews.reviewed_count(), 1);
    }

    #[test]
    fn empty_frozen_file_list_accepts_only_empty_submission() {
        let accepted =
            validate_review_exit(pass_input(Vec::new()), &read_design(), &[], Vec::new());
        assert!(accepted.file_reviews.is_complete());
        assert!(
            accepted
                .file_reviews
                .last_diagnostics
                .as_ref()
                .unwrap()
                .is_empty()
        );

        let rejected = validate_review_exit(
            pass_input(vec![ReviewFileReviewInput {
                path: "src/extra.rs".to_string(),
                reviewed: true,
            }]),
            &read_design(),
            &[],
            Vec::new(),
        );
        assert_eq!(
            rejected.file_reviews.last_diagnostics.unwrap().extra_files,
            vec!["src/extra.rs"]
        );
    }

    #[test]
    fn validation_aggregates_review_contract_violations() {
        let finding = ReviewFinding {
            severity: "high".to_string(),
            title: "设计缺陷".to_string(),
            body: String::new(),
            recommendation: String::new(),
            path: Some("src/lib.rs".to_string()),
            line: Some(12),
            design_references: Vec::new(),
        };
        let validation = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewExitVerdict::Pass,
                summary: String::new(),
                design_references: Vec::new(),
                findings: vec![finding],
                file_reviews: vec![ReviewFileReviewInput {
                    path: "src/lib.rs".to_string(),
                    reviewed: true,
                }],
            },
            &read_design(),
            &["src/lib.rs".to_string()],
            vec![ReviewExitViolation {
                code: "locatorMissing".to_string(),
                message: "missing".to_string(),
                location: None,
            }],
        );
        let violations = &validation
            .file_reviews
            .last_diagnostics
            .as_ref()
            .unwrap()
            .violations;
        let codes = violations
            .iter()
            .map(|violation| violation.code.as_str())
            .collect::<BTreeSet<_>>();

        assert!(codes.contains("locatorMissing"));
        assert!(codes.contains("summaryMissing"));
        assert!(codes.contains("passHasFindings"));
        assert!(codes.contains("designReferenceMissing"));
        assert!(codes.contains("findingContentMissing"));
        assert!(codes.contains("recommendationMissing"));
        assert!(codes.contains("findingDesignReferenceMissing"));
    }
}
