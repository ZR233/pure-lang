use std::path::PathBuf;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, MAX_MODEL_TOOL_OUTPUT_BYTES, OutputTruncation, ToolDirective,
    enforce_model_output_limit, model_visible_tool_output, model_visible_tool_output_with_budget,
    model_visible_tool_output_with_tokens,
};

/// Canonical input passed to a local tool executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInput {
    pub arguments: serde_json::Value,
}

/// Canonical full-fidelity result content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "value")]
pub enum ToolResultContent {
    Text(String),
    Json(serde_json::Value),
}

impl ToolResultContent {
    pub fn canonical_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Json(value) => crate::working_set::canonical_json_string(value),
        }
    }
}

/// Unified result returned by every local tool.
///
/// `content` is the canonical full result, while `model_output` is its bounded model
/// projection. Artifacts, interaction requests and end-turn behavior are expressed by
/// typed [`ToolDirective`] values. The manager owns execution caching and final trace
/// projection; tools only report backend facts such as exit status and captured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub success: bool,
    pub content: ToolResultContent,
    pub model_output: String,
    /// Durable model-facing attachments committed before this result is returned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_attachments: Vec<pl_protocol::ThreadAttachment>,
    pub truncated: OutputTruncation,
    pub output_file: PathBuf,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_events: Vec<ToolDirective>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self::new(true, output.into(), false)
    }

    pub fn failure(output: impl Into<String>) -> Self {
        Self::new(false, output.into(), false)
    }

    pub fn json(value: impl Serialize) -> Result<Self, PureError> {
        let value =
            serde_json::to_value(value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "tool_result".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        let output = crate::working_set::canonical_json_string(&value);
        let mut result = Self::success(output);
        result.content = ToolResultContent::Json(value);
        Ok(result)
    }

    pub fn json_with_budget(
        value: impl Serialize,
        max_output_tokens: usize,
        max_output_bytes: usize,
    ) -> Result<Self, PureError> {
        let value =
            serde_json::to_value(value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "tool_result".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        let output = crate::working_set::canonical_json_string(&value);
        let mut result = Self::with_model_budget(
            true,
            output,
            false,
            max_output_tokens,
            max_output_bytes,
            Vec::<serde_json::Value>::new(),
        );
        result.content = ToolResultContent::Json(value);
        Ok(result)
    }

    pub fn new(success: bool, output: String, ends_turn: bool) -> Self {
        Self::with_model_tokens(
            success,
            output,
            ends_turn,
            DEFAULT_MODEL_TOOL_OUTPUT_TOKENS,
            Vec::<serde_json::Value>::new(),
        )
    }

    pub fn with_model_tokens<Artifact>(
        success: bool,
        output: String,
        ends_turn: bool,
        max_output_tokens: usize,
        output_artifacts: Vec<Artifact>,
    ) -> Self
    where
        Artifact: Serialize,
    {
        let model_output = model_visible_tool_output_with_tokens(&output, max_output_tokens);
        Self::with_model_output(success, output, model_output, ends_turn, output_artifacts)
    }

    pub fn with_model_budget<Artifact>(
        success: bool,
        output: String,
        ends_turn: bool,
        max_output_tokens: usize,
        max_output_bytes: usize,
        output_artifacts: Vec<Artifact>,
    ) -> Self
    where
        Artifact: Serialize,
    {
        let model_output =
            model_visible_tool_output_with_budget(&output, max_output_tokens, max_output_bytes);
        let mut result =
            Self::with_model_output(success, output, model_output, ends_turn, output_artifacts);
        result.runtime_events.push(ToolDirective::OutputBudget {
            max_bytes: max_output_bytes,
        });
        result
    }

    pub fn with_model_output<Artifact>(
        success: bool,
        output: String,
        model_output: String,
        ends_turn: bool,
        output_artifacts: Vec<Artifact>,
    ) -> Self
    where
        Artifact: Serialize,
    {
        let raw_bytes = output.len() as u64;
        let model_output = enforce_model_output_limit(&model_output, MAX_MODEL_TOOL_OUTPUT_BYTES);
        let artifacts = output_artifacts
            .into_iter()
            .map(|artifact| {
                serde_json::to_value(artifact).unwrap_or_else(
                    |error| serde_json::json!({ "serializationError": error.to_string() }),
                )
            })
            .collect::<Vec<_>>();
        let artifact_bytes = tool_result_artifact_bytes(&artifacts);
        let mut runtime_events = Vec::new();
        if !artifacts.is_empty() {
            runtime_events.push(ToolDirective::OutputArtifacts { artifacts });
        }
        runtime_events.push(ToolDirective::OutputMetrics {
            raw_bytes,
            model_visible_bytes: model_output.len() as u64,
            artifact_bytes,
            result_hash: crate::canonical_content_hash(output.as_bytes()),
        });
        if ends_turn {
            runtime_events.push(ToolDirective::EndTurn {
                final_content: None,
            });
        }
        Self {
            success,
            content: ToolResultContent::Text(output),
            model_output,
            model_attachments: Vec::new(),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: Some(if success { 0 } else { 1 }),
            timed_out: false,
            runtime_events,
        }
    }

    pub fn from_runtime_text(
        output: impl Into<String>,
        truncated: OutputTruncation,
        output_file: PathBuf,
        exit_code: Option<i32>,
        timed_out: bool,
        runtime_events: Vec<ToolDirective>,
    ) -> Self {
        let output = output.into();
        Self {
            success: !timed_out && exit_code.unwrap_or_default() == 0,
            content: ToolResultContent::Text(output.clone()),
            model_output: output,
            model_attachments: Vec::new(),
            truncated,
            output_file,
            exit_code,
            timed_out,
            runtime_events,
        }
    }

    pub fn ending_turn(mut self) -> Self {
        if !self.ends_turn() {
            self.runtime_events.push(ToolDirective::EndTurn {
                final_content: None,
            });
        }
        self
    }

    pub fn with_model_attachment(mut self, attachment: pl_protocol::ThreadAttachment) -> Self {
        self.model_attachments.push(attachment);
        self
    }

    pub fn ending_turn_with_content(mut self, content: impl Into<String>) -> Self {
        let content = model_visible_tool_output(&content.into());
        self.runtime_events
            .retain(|directive| !matches!(directive, ToolDirective::EndTurn { .. }));
        self.runtime_events.push(ToolDirective::EndTurn {
            final_content: (!content.trim().is_empty()).then_some(content),
        });
        self
    }

    pub fn canonical_output(&self) -> String {
        self.content.canonical_text()
    }

    pub fn model_output(&self) -> &str {
        &self.model_output
    }

    pub fn into_model_output(self) -> String {
        self.model_output
    }

    pub fn output_artifacts_as<T>(&self) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        self.runtime_events
            .iter()
            .filter_map(|event| match event {
                ToolDirective::OutputArtifacts { artifacts } => Some(artifacts.as_slice()),
                ToolDirective::InteractionRequested { .. }
                | ToolDirective::SkillActivated { .. }
                | ToolDirective::ToolResultRevision { .. }
                | ToolDirective::RevealTools { .. }
                | ToolDirective::AuditMetadata { .. }
                | ToolDirective::ExecutionFailed
                | ToolDirective::CacheHit { .. }
                | ToolDirective::OutputMetrics { .. }
                | ToolDirective::OutputBudget { .. }
                | ToolDirective::EndTurn { .. } => None,
            })
            .flatten()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect()
    }

    pub fn ends_turn(&self) -> bool {
        self.runtime_events
            .iter()
            .any(|event| matches!(event, ToolDirective::EndTurn { .. }))
    }

    pub fn end_turn_content(&self) -> Option<&str> {
        self.runtime_events.iter().find_map(|event| match event {
            ToolDirective::EndTurn {
                final_content: Some(content),
            } => Some(content.as_str()),
            ToolDirective::InteractionRequested { .. }
            | ToolDirective::SkillActivated { .. }
            | ToolDirective::ToolResultRevision { .. }
            | ToolDirective::OutputArtifacts { .. }
            | ToolDirective::RevealTools { .. }
            | ToolDirective::AuditMetadata { .. }
            | ToolDirective::ExecutionFailed
            | ToolDirective::CacheHit { .. }
            | ToolDirective::OutputMetrics { .. }
            | ToolDirective::OutputBudget { .. }
            | ToolDirective::EndTurn {
                final_content: None,
            } => None,
        })
    }
}

pub(crate) fn tool_result_artifact_bytes(artifacts: &[serde_json::Value]) -> u64 {
    artifacts
        .iter()
        .filter_map(|artifact| {
            ["sizeBytes", "size_bytes", "size"]
                .into_iter()
                .find_map(|field| artifact.get(field).and_then(serde_json::Value::as_u64))
        })
        .fold(0_u64, u64::saturating_add)
}
