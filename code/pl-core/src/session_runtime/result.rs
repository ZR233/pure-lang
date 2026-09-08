use pl_protocol::session_runtime::{ToolTaskOutputFact, ToolTaskRejectedControl};

use crate::tool::{ToolDirective, ToolResult, ToolResultContent};

use super::ToolTaskResult;

pub(crate) fn task_result(output: &ToolResult) -> ToolTaskResult {
    let mut facts = Vec::new();
    let mut skill_activations = Vec::new();
    for event in &output.runtime_events {
        let fact = match event {
            ToolDirective::OutputArtifacts { artifacts } => ToolTaskOutputFact::OutputArtifacts {
                artifacts: artifacts.clone(),
            },
            ToolDirective::AuditMetadata { metadata } => ToolTaskOutputFact::AuditMetadata {
                metadata: metadata.clone(),
            },
            ToolDirective::CacheHit {
                reused_from_call_id,
                result_hash,
                total_bytes,
            } => ToolTaskOutputFact::CacheHit {
                reused_from_call_id: reused_from_call_id.clone(),
                result_hash: result_hash.clone(),
                total_bytes: *total_bytes,
            },
            ToolDirective::OutputMetrics {
                raw_bytes,
                model_visible_bytes,
                artifact_bytes,
                result_hash,
            } => ToolTaskOutputFact::OutputMetrics {
                raw_bytes: *raw_bytes,
                model_visible_bytes: *model_visible_bytes,
                artifact_bytes: *artifact_bytes,
                result_hash: result_hash.clone(),
            },
            ToolDirective::OutputBudget { max_bytes } => ToolTaskOutputFact::OutputBudget {
                max_bytes: *max_bytes,
            },
            ToolDirective::SessionEvents { .. } => ToolTaskOutputFact::RejectedControl {
                directive: ToolTaskRejectedControl::SessionEvents,
            },
            ToolDirective::InteractionRequested { .. } => ToolTaskOutputFact::RejectedControl {
                directive: ToolTaskRejectedControl::InteractionRequested,
            },
            ToolDirective::RevealTools { .. } => ToolTaskOutputFact::RejectedControl {
                directive: ToolTaskRejectedControl::RevealTools,
            },
            ToolDirective::EndTurn { .. } => ToolTaskOutputFact::RejectedControl {
                directive: ToolTaskRejectedControl::EndTurn,
            },
            ToolDirective::SkillActivated { activation } => {
                skill_activations.push(activation.clone());
                continue;
            }
            ToolDirective::ExecutionFailed => continue,
        };
        facts.push(fact);
    }
    let mut result = ToolTaskResult {
        output: output.canonical_output(),
        structured_content: match &output.content {
            ToolResultContent::Text(_) => None,
            ToolResultContent::Json(value) => Some(value.clone()),
        },
        skill_activations,
        timed_out: output.timed_out,
        exit_code: output.exit_code,
        output_file: (!output.output_file.as_os_str().is_empty())
            .then(|| output.output_file.to_string_lossy().into_owned()),
        attachments: output.model_attachments.clone(),
        facts,
    };
    let mut has_metrics = false;
    for fact in &mut result.facts {
        if let ToolTaskOutputFact::OutputMetrics {
            model_visible_bytes,
            ..
        } = fact
        {
            *model_visible_bytes = output.model_output().len() as u64;
            has_metrics = true;
        }
    }
    if !has_metrics {
        let canonical = output.canonical_output();
        let artifacts = task_artifacts(&result);
        result.facts.push(ToolTaskOutputFact::OutputMetrics {
            raw_bytes: canonical.len() as u64,
            model_visible_bytes: output.model_output().len() as u64,
            artifact_bytes: crate::tool::tool_result_artifact_bytes(&artifacts),
            result_hash: crate::canonical_content_hash(canonical.as_bytes()),
        });
    }
    result
}

pub(crate) fn is_rejected_control(fact: &ToolTaskOutputFact) -> bool {
    matches!(fact, ToolTaskOutputFact::RejectedControl { .. })
}

pub(crate) fn task_artifacts(result: &ToolTaskResult) -> Vec<serde_json::Value> {
    let mut artifacts = Vec::new();
    for fact in &result.facts {
        if let ToolTaskOutputFact::OutputArtifacts { artifacts: values } = fact {
            artifacts.extend(values.iter().cloned());
        }
    }
    if let Some(path) = &result.output_file {
        let file = serde_json::json!({"kind":"toolOutput","path":path});
        if !artifacts.contains(&file) {
            artifacts.push(file);
        }
    }
    artifacts
}

pub(crate) fn direct_result_directives(
    result: &ToolTaskResult,
    status: super::ToolTaskStatus,
) -> Vec<ToolDirective> {
    let mut directives: Vec<_> = result
        .skill_activations
        .iter()
        .filter(|_| status == super::ToolTaskStatus::Succeeded)
        .cloned()
        .map(|activation| ToolDirective::SkillActivated { activation })
        .collect();
    for fact in &result.facts {
        let directive = match fact {
            ToolTaskOutputFact::OutputArtifacts { .. } => continue,
            ToolTaskOutputFact::AuditMetadata { metadata } => ToolDirective::AuditMetadata {
                metadata: metadata.clone(),
            },
            ToolTaskOutputFact::CacheHit {
                reused_from_call_id,
                result_hash,
                total_bytes,
            } => ToolDirective::CacheHit {
                reused_from_call_id: reused_from_call_id.clone(),
                result_hash: result_hash.clone(),
                total_bytes: *total_bytes,
            },
            ToolTaskOutputFact::OutputMetrics {
                raw_bytes,
                model_visible_bytes,
                artifact_bytes,
                result_hash,
            } => ToolDirective::OutputMetrics {
                raw_bytes: *raw_bytes,
                model_visible_bytes: *model_visible_bytes,
                artifact_bytes: *artifact_bytes,
                result_hash: result_hash.clone(),
            },
            ToolTaskOutputFact::OutputBudget { max_bytes } => ToolDirective::OutputBudget {
                max_bytes: *max_bytes,
            },
            ToolTaskOutputFact::RejectedControl { .. } => continue,
        };
        directives.push(directive);
    }
    let artifacts = task_artifacts(result);
    if !artifacts.is_empty() {
        directives.push(ToolDirective::OutputArtifacts { artifacts });
    }
    directives
}
