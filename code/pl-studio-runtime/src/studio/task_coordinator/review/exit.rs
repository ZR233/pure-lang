use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::super::{
    AgentReview, ReviewDesignReference, ReviewFinding, ReviewVerdict, TaskCoordinator,
};
use super::trace::validate_review_trace;
use super::validate_review_repository;
use crate::AgentRuntimeHandle;
use crate::tool::{
    RegisteredTool, ToolExecutionResult, ToolInputSchemaField, strict_tool_input_schema,
};
use crate::turn::ToolEffect;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewExitInput {
    verdict: ReviewVerdict,
    summary: String,
    design_references: Vec<ReviewDesignReference>,
    findings: Vec<ReviewFinding>,
}

impl TaskCoordinator {
    pub(crate) fn review_exit_tool(
        self: &Arc<Self>,
        thread_id: impl Into<String>,
        runtime: Option<AgentRuntimeHandle>,
    ) -> RegisteredTool {
        let coordinator = self.clone();
        let thread_id = thread_id.into();
        RegisteredTool::from_typed_fallible_execution_result(
            "review_exit",
            "Submit trace-validated read-only review findings and end the reviewer turn.",
            review_exit_schema(),
            move |input: ReviewExitInput, context| {
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
                    if let Some(runtime) = runtime
                        && let Err(error) =
                            resume_planner_after_review(&runtime, &thread_id, &round).await
                    {
                        let message = format!(
                            "planner continuation after review {} failed: {error}",
                            round.id
                        );
                        coordinator
                            .block_continuation_failure(&round.task_run_id, message)
                            .await?;
                        return Err(error);
                    }
                    let mut output = ToolExecutionResult::<serde_json::Value>::json(round)
                        .map_err(anyhow::Error::from)?;
                    output.ends_turn = true;
                    Ok::<_, anyhow::Error>(output)
                }
            },
        )
        .with_effect(ToolEffect::Read)
    }
}

async fn resume_planner_after_review(
    runtime: &AgentRuntimeHandle,
    thread_id: &str,
    round: &super::super::ReviewRoundRecord,
) -> Result<()> {
    let root_agent_id = crate::studio::agent_host::root_agent_id(thread_id);
    runtime
        .wait_until_idle(root_agent_id.clone())
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let thread = pl_core::ThreadId::new(thread_id.to_string())?;
    let request = pl_core::AgentSubmitRequest::start(
        thread,
        format!(
            "Review round {} completed. Read task_status and list_agents, then continue from the canonical Task phase.",
            round.id
        ),
    )
    .with_presentation(pl_core::MailboxPresentation::Hidden)
    .with_metadata(serde_json::json!({
        "kind": "taskReviewContinuation",
        "taskRunId": round.task_run_id,
        "reviewRoundId": round.id,
        "scope": round.scope,
    }))
    .with_mail_id(format!("task-review-continuation:{}", round.id))
    .with_turn_policy(pl_core::AgentTurnSubmitPolicy::StartOnly);
    match runtime.submit(root_agent_id, request).await {
        Ok(_) => Ok(()),
        Err(pl_core::AgentRuntimeError::InvalidInput(reason))
            if reason == "startTurn requires an idle Thread" =>
        {
            // Another input won the idle-to-start race after the review transaction committed.
            // That turn is already prepared from the new canonical Task phase.
            Ok(())
        }
        Err(error) => Err(anyhow::anyhow!(error.to_string())),
    }
}

fn validate_review_exit(
    input: ReviewExitInput,
    read_design: &BTreeMap<String, String>,
) -> Result<AgentReview> {
    let summary = input.summary.trim().to_string();
    if summary.is_empty() {
        bail!("review summary must not be empty");
    }
    if matches!(
        input.verdict,
        ReviewVerdict::Pending | ReviewVerdict::Failed
    ) {
        bail!("reviewer may only select pass, changesRequired, or blocked");
    }
    match input.verdict {
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
        verdict: input.verdict,
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

fn review_exit_schema() -> serde_json::Value {
    let reference = serde_json::json!({
        "type":"object",
        "properties": {
            "path":{"type":"string"},
            "section":{"type":"string"}
        },
        "required":["path","section"],
        "additionalProperties":false
    });
    strict_tool_input_schema([
        ToolInputSchemaField::required(
            "verdict",
            serde_json::json!({"type":"string","enum":["pass","changesRequired","blocked"]}),
        ),
        ToolInputSchemaField::required("summary", serde_json::json!({"type":"string"})),
        ToolInputSchemaField::required(
            "designReferences",
            serde_json::json!({"type":"array","items":reference.clone()}),
        ),
        ToolInputSchemaField::required(
            "findings",
            serde_json::json!({
                "type":"array",
                "items":{
                    "type":"object",
                    "properties":{
                        "severity":{"type":"string"},
                        "title":{"type":"string"},
                        "body":{"type":"string"},
                        "path":{"type":["string","null"]},
                        "line":{"type":["integer","null"]},
                        "designReferences":{"type":"array","items":reference}
                    },
                    "required":["severity","title","body","path","line","designReferences"],
                    "additionalProperties":false
                }
            }),
        ),
    ])
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
            path: Some("code/example.rs".to_string()),
            line: Some(12),
            design_references: Vec::new(),
        };
        let pass_error = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewVerdict::Pass,
                summary: "reviewed".to_string(),
                design_references: vec![reference("Review design")],
                findings: vec![finding],
            },
            &read_design(),
        )
        .unwrap_err();
        let changes_error = validate_review_exit(
            ReviewExitInput {
                verdict: ReviewVerdict::ChangesRequired,
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
}
