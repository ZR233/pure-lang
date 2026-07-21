use std::path::PathBuf;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, MAX_MODEL_TOOL_OUTPUT_BYTES, OutputTruncation,
    ToolRuntimeEvent, enforce_model_output_limit, model_visible_tool_output,
    model_visible_tool_output_with_tokens,
};

/// 通用工具输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInput {
    pub arguments: serde_json::Value,
    pub session_id: String,
    pub tool_id: String,
    #[serde(default)]
    pub revision_base: u64,
}

/// 通用工具输出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub description: String,
    pub truncated: OutputTruncation,
    pub output_file: PathBuf,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_events: Vec<ToolRuntimeEvent>,
}

/// 根据产品工具的模型可见输出构造 pl-core 工具输出。
///
/// 产品工具 handler 仍负责业务执行和输出文本生成；pl-core 统一把成功状态和
/// 结束回合语义映射成 canonical `ToolOutput`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputModelOutputRequest {
    pub model_output: String,
    pub success: bool,
    pub ends_turn: bool,
}

/// 工具执行结果的通用中间形态。
///
/// handler 可以保留完整输出和 artifact 元数据，同时由 pl-core 统一生成模型可见
/// 输出、成功状态和结束回合事件，避免产品层各自维护一套截断和 `ToolOutput`
/// 映射规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionResult<Artifact = serde_json::Value> {
    pub success: bool,
    pub output: String,
    pub model_output: String,
    pub ends_turn: bool,
    pub output_artifacts: Vec<Artifact>,
}

impl<Artifact> ToolExecutionResult<Artifact> {
    pub fn json(value: impl Serialize) -> Result<Self, PureError> {
        let output =
            serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "registered_tool".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        Ok(Self::success(output))
    }

    pub fn success(output: impl Into<String>) -> Self {
        Self::new(true, output.into(), false)
    }

    pub fn failure(output: impl Into<String>) -> Self {
        Self::new(false, output.into(), false)
    }

    /// 标记成功或失败结果写入 history 后立即结束当前 turn。
    pub fn ending_turn(mut self) -> Self {
        self.ends_turn = true;
        self
    }

    pub fn new(success: bool, output: String, ends_turn: bool) -> Self {
        Self::with_model_tokens(
            success,
            output,
            ends_turn,
            DEFAULT_MODEL_TOOL_OUTPUT_TOKENS,
            Vec::new(),
        )
    }

    pub fn with_model_tokens(
        success: bool,
        output: String,
        ends_turn: bool,
        max_output_tokens: usize,
        output_artifacts: Vec<Artifact>,
    ) -> Self {
        let model_output = model_visible_tool_output_with_tokens(&output, max_output_tokens);
        Self::with_model_output(success, output, model_output, ends_turn, output_artifacts)
    }

    pub fn with_model_output(
        success: bool,
        output: String,
        model_output: String,
        ends_turn: bool,
        output_artifacts: Vec<Artifact>,
    ) -> Self {
        Self {
            success,
            output,
            model_output: enforce_model_output_limit(&model_output, MAX_MODEL_TOOL_OUTPUT_BYTES),
            ends_turn,
            output_artifacts,
        }
    }

    pub fn into_tool_output(self) -> ToolOutput
    where
        Artifact: Serialize,
    {
        let raw_bytes = self.output.len() as u64;
        let model_visible_bytes = self.model_output.len() as u64;
        let artifacts = self
            .output_artifacts
            .into_iter()
            .map(|artifact| {
                serde_json::to_value(artifact).unwrap_or_else(
                    |error| serde_json::json!({ "serializationError": error.to_string() }),
                )
            })
            .collect::<Vec<_>>();
        let artifact_bytes = tool_output_artifact_bytes(&artifacts);
        let mut output = ToolOutput::from_model_output(ToolOutputModelOutputRequest {
            model_output: self.model_output,
            success: self.success,
            ends_turn: self.ends_turn,
        });
        if !artifacts.is_empty() {
            output
                .runtime_events
                .push(ToolRuntimeEvent::OutputArtifacts { artifacts });
        }
        output.runtime_events.push(ToolRuntimeEvent::OutputMetrics {
            raw_bytes,
            model_visible_bytes,
            artifact_bytes,
            result_hash: crate::canonical_content_hash(self.output.as_bytes()),
        });
        output
    }
}

/// 汇总 artifact 描述符声明的完整输出字节数。
pub fn tool_output_artifact_bytes(artifacts: &[serde_json::Value]) -> u64 {
    artifacts
        .iter()
        .filter_map(|artifact| {
            ["sizeBytes", "size_bytes", "size"]
                .into_iter()
                .find_map(|field| artifact.get(field).and_then(serde_json::Value::as_u64))
        })
        .fold(0_u64, u64::saturating_add)
}

impl ToolOutput {
    pub fn from_model_output(request: ToolOutputModelOutputRequest) -> Self {
        Self {
            description: model_visible_tool_output(&request.model_output),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: if request.success { Some(0) } else { Some(1) },
            timed_out: false,
            runtime_events: if request.ends_turn {
                vec![ToolRuntimeEvent::EndTurn]
            } else {
                Vec::new()
            },
        }
    }

    pub fn json(value: impl Serialize) -> Result<Self, PureError> {
        let description =
            serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "registered_tool".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        Ok(Self {
            description: model_visible_tool_output(&description),
            truncated: OutputTruncation::empty(),
            output_file: PathBuf::new(),
            exit_code: None,
            timed_out: false,
            runtime_events: Vec::new(),
        })
    }

    /// 消费工具输出并返回模型可见文本。
    ///
    /// `ToolOutput` 内部目前用 `description` 存储模型可见输出；产品层应通过该
    /// 语义方法读取，避免把字段名当作共享协议。
    pub fn into_model_output(self) -> String {
        model_visible_tool_output(&self.description)
    }

    /// 从工具运行时事件中提取并解码输出 artifact。
    ///
    /// `OutputArtifacts` 是 pl-core 的工具执行事件细节，产品层应通过这个方法
    /// 取得自身协议需要的 artifact 类型，而不是直接匹配 `ToolRuntimeEvent`。
    /// 无法解码的条目会被忽略，和生命周期投影的 artifact 容错语义保持一致。
    pub fn output_artifacts_as<T>(&self) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        self.runtime_events
            .iter()
            .filter_map(|event| match event {
                ToolRuntimeEvent::OutputArtifacts { artifacts } => Some(artifacts.as_slice()),
                ToolRuntimeEvent::SkillActivated {
                    activation: _activation,
                } => None,
                ToolRuntimeEvent::ToolResultRevision {
                    revision: _revision,
                } => None,
                ToolRuntimeEvent::CacheHit { .. } => None,
                ToolRuntimeEvent::OutputMetrics { .. } => None,
                ToolRuntimeEvent::EndTurn => None,
            })
            .flatten()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect()
    }

    /// 判断工具输出是否要求当前 turn 结束。
    ///
    /// 结束回合是 pl-core 工具运行时事件的一种语义；产品层应调用该方法，而不是
    /// 直接匹配 `ToolRuntimeEvent::EndTurn`。
    pub fn ends_turn(&self) -> bool {
        self.runtime_events.iter().any(|event| match event {
            ToolRuntimeEvent::EndTurn => true,
            ToolRuntimeEvent::SkillActivated {
                activation: _activation,
            } => false,
            ToolRuntimeEvent::ToolResultRevision {
                revision: _revision,
            } => false,
            ToolRuntimeEvent::OutputArtifacts {
                artifacts: _artifacts,
            } => false,
            ToolRuntimeEvent::CacheHit { .. } => false,
            ToolRuntimeEvent::OutputMetrics { .. } => false,
        })
    }

    /// 将 canonical 工具输出投影回产品层常用的执行结果形态。
    ///
    /// 该方法统一 `ToolOutput` 的成功判定、模型可见输出、结束回合语义和 artifact
    /// 解码，避免产品 adapter 直接读取 `exit_code`、`description` 或运行时事件。
    pub fn to_execution_result<T>(&self) -> ToolExecutionResult<T>
    where
        T: DeserializeOwned,
    {
        ToolExecutionResult::with_model_output(
            self.exit_code.unwrap_or(0) == 0,
            self.description.clone(),
            self.description.clone(),
            self.ends_turn(),
            self.output_artifacts_as(),
        )
    }
}
