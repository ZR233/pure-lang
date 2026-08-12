use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Deserialize;

use super::super::{
    AgentReview, ReviewDesignReference, ReviewFinding, ReviewVerdict, TaskCoordinator,
    TaskPlannerWakeRequest, TaskPlannerWakeSource,
};
use super::trace::validate_review_trace;
use super::validate_review_repository;
use crate::AgentRuntimeHandle;
use crate::tool::{FunctionToolDefinition, RegisteredTool, ToolExecutionResult};
use crate::turn::ToolEffect;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewExitInput {
    verdict: ReviewExitVerdict,
    /// Concise overall review summary.
    summary: String,
    /// Actual design sections read during the review.
    design_references: Vec<ReviewDesignReference>,
    /// Actionable unresolved findings; empty only for pass.
    findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ReviewExitVerdict {
    Pass,
    ChangesRequired,
    Blocked,
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
            "Submit trace-validated read-only review findings and end the reviewer turn.",
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
                let trace =
                    validate_review_trace(&context.parent_session, context.workspace.root())
                        .await?;
                let review = validate_review_exit(input, &trace.read_design)?;
                let run = coordinator
                    .store
                    .read_active_task_run_for_root_thread(&thread_id)
                    .await?;
                validate_review_repository(&run).await?;
                let round = coordinator
                    .store
                    .complete_task_review(&thread_id, &reviewer.id, review)
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
                        &coordinator.store,
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
                let mut output = ToolExecutionResult::<serde_json::Value>::json(round)
                    .map_err(anyhow::Error::from)?;
                output.ends_turn = true;
                Ok::<_, anyhow::Error>(output)
            }
        })
        .with_effect(ToolEffect::Read)
    }
}

fn validate_review_exit(
    input: ReviewExitInput,
    read_design: &BTreeMap<String, String>,
) -> Result<AgentReview> {
    let verdict = match input.verdict {
        ReviewExitVerdict::Pass => ReviewVerdict::Pass,
        ReviewExitVerdict::ChangesRequired => ReviewVerdict::ChangesRequired,
        ReviewExitVerdict::Blocked => ReviewVerdict::Blocked,
    };
    let summary = input.summary.trim().to_string();
    if summary.is_empty() {
        bail!("review summary must not be empty");
    }
    match verdict {
        ReviewVerdict::Pass if !input.findings.is_empty() => {
            bail!("pass requires no unresolved findings")
        }
        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked if input.findings.is_empty() => {
            bail!("changesRequired and blocked require a concrete finding")
        }
        ReviewVerdict::Pass
        | ReviewVerdict::ChangesRequired
        | ReviewVerdict::Blocked
        | ReviewVerdict::Pending
        | ReviewVerdict::Failed => {}
    }
    if input.design_references.is_empty() {
        bail!("review_exit requires at least one actual design reference");
    }
    let mut top_paths = BTreeSet::new();
    for reference in &input.design_references {
        validate_reference(reference, read_design)?;
        top_paths.insert(reference.path.clone());
    }
    for finding in &input.findings {
        if finding.title.trim().is_empty() || finding.body.trim().is_empty() {
            bail!("review findings require non-empty title and body");
        }
        if finding.recommendation.trim().is_empty() {
            bail!("review findings require a concrete recommendation explaining how to fix it");
        }
        let design_claim = finding.title.to_ascii_lowercase().contains("design")
            || finding.body.to_ascii_lowercase().contains("design")
            || finding.title.contains("设计")
            || finding.body.contains("设计");
        if design_claim && finding.design_references.is_empty() {
            bail!("design consistency findings require actual design references");
        }
        for reference in &finding.design_references {
            validate_reference(reference, read_design)?;
            if !top_paths.contains(&reference.path) {
                bail!("finding design reference is absent from top-level designReferences");
            }
        }
    }
    Ok(AgentReview {
        verdict,
        summary,
        design_references: input.design_references,
        findings: input.findings,
    })
}

fn validate_reference(
    reference: &ReviewDesignReference,
    read_design: &BTreeMap<String, String>,
) -> Result<()> {
    let path = Path::new(&reference.path);
    if reference.path.contains('\\')
        || path.is_absolute()
        || path.components().count() < 2
        || !matches!(path.components().next(), Some(Component::Normal(part)) if part == "design")
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !read_design.contains_key(&reference.path)
    {
        bail!("design reference must name an actually read normalized design/** file");
    }
    if reference.section.trim().is_empty() {
        bail!("design reference section must not be empty");
    }
    if !read_design
        .get(&reference.path)
        .is_some_and(|content| content.contains(reference.section.trim()))
    {
        bail!("design reference section was not present in the actual read_file content");
    }
    Ok(())
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

    #[test]
    fn pass_with_finding_and_changes_without_finding_are_rejected() {
        let finding = ReviewFinding {
            severity: "high".to_string(),
            title: "Bug".to_string(),
            body: "The implementation can fail.".to_string(),
            recommendation:
                "Return the error instead of unwrapping; map it into ConfigError::Read.".to_string(),
            path: Some("code/example.rs".to_string()),
            line: Some(12),
            design_references: Vec::new(),
        };
        let pass_error = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewExitVerdict::Pass,
                summary: "reviewed".to_string(),
                design_references: vec![reference("Review design")],
                findings: vec![finding.clone()],
            },
            &read_design(),
        )
        .unwrap_err();
        let changes_error = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewExitVerdict::ChangesRequired,
                summary: "reviewed".to_string(),
                design_references: vec![reference("Review design")],
                findings: Vec::new(),
            },
            &read_design(),
        )
        .unwrap_err();

        assert!(pass_error.to_string().contains("pass requires"));
        assert!(changes_error.to_string().contains("concrete finding"));
    }

    #[test]
    fn changes_required_finding_without_recommendation_is_rejected() {
        let finding = ReviewFinding {
            severity: "high".to_string(),
            title: "Bug".to_string(),
            body: "The implementation can fail.".to_string(),
            recommendation: "   ".to_string(),
            path: Some("code/example.rs".to_string()),
            line: Some(12),
            design_references: Vec::new(),
        };
        let error = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewExitVerdict::ChangesRequired,
                summary: "reviewed".to_string(),
                design_references: vec![reference("Review design")],
                findings: vec![finding],
            },
            &read_design(),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("recommendation"),
            "expected recommendation requirement, got: {error}"
        );
    }
}
