//! Product progress reports are immutable extensions, never implicit completion or review approval.
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::extensions::ExtensionMutation,
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pl_protocol::{AgentProgressReport, AgentProgressStage, AgentSubmissionRecord};
use schemars::JsonSchema;
use serde::Deserialize;

pub(crate) const EXTENSION: &str = "studio.agent-progress";
pub(crate) const FORMAT: &str = "pl.studio.agent-progress";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProgressInput {
    stage: ProgressStage,
    #[schemars(length(min = 1, max = 1200))]
    summary: String,
    #[schemars(length(max = 500))]
    next_step: String,
    #[serde(default)]
    #[schemars(length(max = 20000))]
    detail: Option<String>,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum ProgressStage {
    Exploring,
    Implementing,
    Verifying,
    Blocked,
    ReadyForCompletion,
}
impl From<ProgressStage> for AgentProgressStage {
    fn from(value: ProgressStage) -> Self {
        match value {
            ProgressStage::Exploring => Self::Exploring,
            ProgressStage::Implementing => Self::Implementing,
            ProgressStage::Verifying => Self::Verifying,
            ProgressStage::Blocked => Self::Blocked,
            ProgressStage::ReadyForCompletion => Self::ReadyForCompletion,
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ProgressError {
    #[error("invalid progress report length")]
    Length,
    #[error("progress revision exhausted")]
    RevisionExhausted,
    #[error("unsupported saved progress report {format} version {version}")]
    Unsupported { format: String, version: u32 },
}

#[derive(Debug)]
struct ReportProgress;
impl Tool for ReportProgress {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        let input: ProgressInput = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        if input.summary.trim().is_empty()
            || input.summary.chars().count() > 1200
            || input.next_step.chars().count() > 500
            || input
                .detail
                .as_ref()
                .is_some_and(|value| value.chars().count() > 20000)
        {
            return Err(ToolError::new(ProgressError::Length));
        }
        let previous = context.extensions.get(EXTENSION);
        let revision = match previous {
            Some(previous) => decode(&previous.payload)?
                .report
                .revision
                .checked_add(1)
                .ok_or_else(|| ToolError::new(ProgressError::RevisionExhausted))?,
            None => 1,
        };
        let report = AgentSubmissionRecord {
            report: AgentProgressReport {
                stage: input.stage.into(),
                summary: input.summary,
                next_step: input.next_step,
                revision,
            },
            detail: input.detail,
            created_at: crate::studio::unix_seconds(),
        };
        let payload = OpaquePayload::new(
            FORMAT,
            1,
            serde_json::to_string(&report).map_err(ToolError::new)?,
        )
        .map_err(ToolError::new)?;
        let visible = serde_json::to_string(&report.report).map_err(ToolError::new)?;
        Ok(ToolOutput::new(
            payload.clone(),
            vec![ContextContent::Text {
                text: visible.into(),
            }],
        )
        .with_extension_mutations(vec![ExtensionMutation::Put {
            id: EXTENSION.into(),
            expected_revision: previous.map(|value| value.revision),
            payload,
        }]))
    }
}

pub(crate) fn decode(payload: &OpaquePayload) -> Result<AgentSubmissionRecord, ToolError> {
    if payload.format() != FORMAT || payload.version() != 1 {
        return Err(ToolError::new(ProgressError::Unsupported {
            format: payload.format().into(),
            version: payload.version(),
        }));
    }
    serde_json::from_str(payload.content()).map_err(ToolError::new)
}

pub(super) fn registration() -> Result<Registration, super::ThreadAssemblyError> {
    let spec = pl_protocol::ToolSpec::function(
        "report_progress",
        "Report current work and a substantive progress detail. This does not complete a Turn or authorize review.",
        serde_json::to_value(schemars::schema_for!(ProgressInput)).map_err(|error| {
            super::ThreadAssemblyError::Resource {
                operation: "encode progress schema",
                source: Box::new(error),
            }
        })?,
    );
    Ok(Registration::new(
        "report_progress".into(),
        pl_model::runtime::thread_tool_declaration(&spec)?,
        ReportProgress,
    )?
    .with_extension_updates())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::thread::extensions::ExtensionRecord;
    use pretty_assertions::assert_eq;
    use std::{collections::BTreeMap, sync::Arc};

    fn context(extensions: BTreeMap<String, ExtensionRecord>) -> CallContext {
        CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "child".into(),
            turn_id: "turn".into(),
            call_id: "report".into(),
            cancellation: Default::default(),
            extensions: Arc::new(extensions),
            catalog: Arc::from([]),
            extension_sequence: 7,
        }
    }

    #[tokio::test]
    async fn progress_preserves_detail_and_updates_by_cas_without_ending_turn() {
        let input = OpaquePayload::text(
            serde_json::json!({
                "stage": "readyForCompletion", "summary": "  完成\r\n",
                "nextStep": "等待协调者", "detail": "  原文\u{0}\r\n "
            })
            .to_string(),
        );
        let first = ReportProgress
            .execute(input.clone(), context(BTreeMap::new()))
            .await
            .unwrap();
        let report = decode(first.payload()).unwrap();
        assert_eq!(report.report.revision, 1);
        assert_eq!(report.report.summary, "  完成\r\n");
        assert_eq!(report.detail.as_deref(), Some("  原文\u{0}\r\n "));
        assert_eq!(first.control(), pl_core::tool::ToolControl::Continue);
        assert!(first.interaction().is_none());
        assert_eq!(
            first.extension_mutations(),
            &[ExtensionMutation::Put {
                id: EXTENSION.into(),
                expected_revision: None,
                payload: first.payload().clone(),
            }]
        );
        let second = ReportProgress
            .execute(
                input,
                context(BTreeMap::from([(
                    EXTENSION.into(),
                    ExtensionRecord {
                        revision: 7,
                        payload: first.payload().clone(),
                    },
                )])),
            )
            .await
            .unwrap();
        assert_eq!(decode(second.payload()).unwrap().report.revision, 2);
        assert_eq!(
            second.extension_mutations(),
            &[ExtensionMutation::Put {
                id: EXTENSION.into(),
                expected_revision: Some(7),
                payload: second.payload().clone(),
            }]
        );
    }

    #[tokio::test]
    async fn review_authority_and_invalid_lengths_are_rejected() {
        for (stage, summary, next_step, detail) in [
            ("readyForReview", "summary".into(), "".into(), "".into()),
            ("blocked", " \r\n ".into(), "".into(), "".into()),
            ("blocked", "字".repeat(1201), "".into(), "".into()),
            ("blocked", "summary".into(), "字".repeat(501), "".into()),
            ("blocked", "summary".into(), "".into(), "字".repeat(20001)),
        ] {
            let input = OpaquePayload::text(
                serde_json::json!({
                    "stage": stage, "summary": summary, "nextStep": next_step, "detail": detail,
                })
                .to_string(),
            );
            assert!(
                ReportProgress
                    .execute(input, context(BTreeMap::new()))
                    .await
                    .is_err()
            );
        }
    }
}
