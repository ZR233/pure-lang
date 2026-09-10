//! Automatic review policy, model invocation and response decoding owned by Studio.
use crate::approval::{ToolApprovalDecision, ToolReviewCallback, ToolReviewRequest};
use pl_model::completion::{CompletionRequest, ReasoningConfig, ReasoningSummary};
use pl_model::config::ResolvedModelRoute;
use pl_model::runtime::{ModelInvocationContext, ModelRuntime};
use pl_protocol::{Message, MessageContent, MessageRole};
use serde::Deserialize;
use std::sync::Arc;

pub(crate) type ReviewUsageSink =
    Arc<dyn Fn(pl_protocol::InferenceBillingRecord) -> crate::Result<()> + Send + Sync>;

#[derive(Clone)]
struct ReviewBinding {
    runtime: ModelRuntime,
    route: Arc<ResolvedModelRoute>,
    reasoning: Option<ReasoningConfig>,
    usage: ReviewUsageSink,
}

pub(crate) fn reviewer(
    route: &ResolvedModelRoute,
    usage: ReviewUsageSink,
) -> crate::Result<ToolReviewCallback> {
    let runtime = ModelRuntime::new_with_provider_id(
        route.provider_id.as_str(),
        route.endpoint.clone(),
        route.model.clone(),
    )?
    .with_pricing_mode(route.pricing_mode);
    let reasoning = route.effort.as_ref().map(|effort| ReasoningConfig {
        effort: Some(effort.as_str().into()),
        summary: Some(ReasoningSummary::Enabled),
    });
    let binding = ReviewBinding {
        runtime,
        reasoning,
        route: Arc::new(route.clone()),
        usage,
    };
    Ok(Arc::new(move |request| {
        let binding = binding.clone();
        Box::pin(async move { review(binding, request).await })
    }))
}

async fn review(binding: ReviewBinding, request: ToolReviewRequest) -> ToolApprovalDecision {
    let payload = serde_json::json!({
        "toolName": request.tool.name, "arguments": request.tool.arguments,
        "workingDirectory": request.tool.working_directory, "parentAgentId": request.tool.parent_agent_id,
        "permissionMode": request.permission_mode.label(), "workspaceAccess": format!("{:?}", request.workspace_access),
        "workspaceRoot": request.workspace_root.display().to_string(),
        "riskSummary": "Assess the actual requested operations and granted resource access",
    });
    let message = Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(payload.to_string()),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    };
    let completion = CompletionRequest::builder()
        .instructions(include_str!("prompts/permission_review.md"))
        .messages(vec![message])
        .tool_choice("none")
        .temperature(Some(0.0))
        .max_tokens(512)
        .reasoning(binding.reasoning.clone())
        .build();
    let (events, _) = tokio::sync::broadcast::channel(1);
    let invocation = ModelInvocationContext::new(Default::default())
        .with_events(events)
        .with_cancellation(request.cancellation_token);
    let result = binding.runtime.complete(completion, invocation).await;
    let route = &binding.route;
    let accounting = match &result {
        Ok(response) => response.accounting.clone(),
        Err(failure) => (*failure.accounting).clone(),
    };
    let billing = pl_protocol::InferenceBillingRecord {
        purpose: Some("review".into()),
        inference_id: crate::studio::new_id("review"),
        provider_instance_id: route.provider_id.as_str().into(),
        provider: route.endpoint.name.clone(),
        model: result
            .as_ref()
            .ok()
            .map(|response| response.model.clone())
            .filter(|model| !model.is_empty())
            .unwrap_or_else(|| route.model.slug.clone()),
        reasoning_effort: route.effort.as_ref().map(|effort| effort.as_str().into()),
        context_window: route.model.resolved_context_window(),
        accounting,
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        orchestration: result
            .as_ref()
            .ok()
            .map(|response| response.orchestration.clone())
            .unwrap_or_default(),
        timing: result.as_ref().ok().and_then(|response| response.timing),
        recorded_at: crate::studio::unix_seconds(),
    };
    if let Err(error) = (binding.usage)(billing) {
        return ToolApprovalDecision::Denied {
            reason: format!("review accounting could not be admitted: {error}"),
        };
    }
    match result {
        Ok(response) => {
            match parse_reviewer_decision(response.content.as_deref().unwrap_or_default()) {
                Ok(decision) => decision,
                Err(reason) => ToolApprovalDecision::Denied {
                    reason: reason.to_string(),
                },
            }
        }
        Err(error) => ToolApprovalDecision::Denied {
            reason: format!("AI reviewer failed: {error}"),
        },
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewerDecision {
    decision: ReviewVerdict,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReviewVerdict {
    Approved,
    Denied,
}

#[derive(Debug, thiserror::Error)]
#[error("reviewer returned invalid decision JSON: {0}")]
struct ReviewDecodeError(#[from] serde_json::Error);

fn parse_reviewer_decision(content: &str) -> Result<ToolApprovalDecision, ReviewDecodeError> {
    let parsed: ReviewerDecision = serde_json::from_str(content.trim())?;
    Ok(match parsed.decision {
        ReviewVerdict::Approved => ToolApprovalDecision::Approved,
        ReviewVerdict::Denied => ToolApprovalDecision::Denied {
            reason: parsed
                .reason
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| "AI reviewer denied the tool call".into()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    #[test]
    fn reviewer_decision_requires_strict_json() {
        assert_eq!(
            parse_reviewer_decision(r#"{"decision":"approved","reason":"ok"}"#).unwrap(),
            ToolApprovalDecision::Approved
        );
        assert_eq!(
            parse_reviewer_decision(r#"{"decision":"denied","reason":"too broad"}"#).unwrap(),
            ToolApprovalDecision::Denied {
                reason: "too broad".to_string()
            }
        );
        assert!(parse_reviewer_decision("approved").is_err());
        assert!(parse_reviewer_decision(r#"{"decision":"maybe"}"#).is_err());
    }
}
