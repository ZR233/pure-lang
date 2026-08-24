use std::path::PathBuf;

use pl_protocol::PureError;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, MAX_MODEL_TOOL_OUTPUT_BYTES, OutputTruncation,
    ToolRuntimeEvent, enforce_model_output_limit, model_visible_tool_output,
    model_visible_tool_output_with_budget, model_visible_tool_output_with_tokens,
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
    pub end_turn_content: Option<String>,
    pub(crate) completed_plan_content: Option<String>,
    pub output_artifacts: Vec<Artifact>,
    /// 声明的模型可见输出硬字节上限；`Some` 时要求 dispatch 越过默认 12KB 安全阈值。
    pub output_bytes_budget: Option<usize>,
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

    /// 结束当前 turn，并把业务层提供的 canonical 摘要投影为最终 assistant 回复。
    pub fn ending_turn_with_content(mut self, content: impl Into<String>) -> Self {
        self.ends_turn = true;
        let content = model_visible_tool_output(&content.into());
        self.end_turn_content = (!content.trim().is_empty()).then_some(content);
        self
    }

    /// 声明该工具已生成完整计划，由 turn 循环建立 canonical plan item。
    pub fn with_completed_plan(mut self, content: impl Into<String>) -> Self {
        let content = content.into();
        if !content.trim().is_empty() {
            self.completed_plan_content = Some(content);
        }
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

    /// 构造一个同时抬高软 token 预算与硬字节上限的结果。
    ///
    /// 用于 `task_status`、`read_agent_submissions`、`read_review_round` 等只读概览工具：
    /// 它们需要越过默认 [`super::MAX_MODEL_TOOL_OUTPUT_BYTES`] 安全阈值完整返回结构化
    /// 数据，但仍由分页控制总体体积。其他工具应继续使用 [`Self::new`] / [`Self::json`]。
    pub fn with_model_budget(
        success: bool,
        output: String,
        ends_turn: bool,
        max_output_tokens: usize,
        max_output_bytes: usize,
        output_artifacts: Vec<Artifact>,
    ) -> Self {
        let model_output =
            model_visible_tool_output_with_budget(&output, max_output_tokens, max_output_bytes);
        Self {
            success,
            output,
            model_output,
            ends_turn,
            end_turn_content: None,
            completed_plan_content: None,
            output_artifacts,
            output_bytes_budget: Some(max_output_bytes),
        }
    }

    /// 序列化 JSON 值并用抬高的预算构造结果。
    pub fn json_with_budget(
        value: impl Serialize,
        max_output_tokens: usize,
        max_output_bytes: usize,
    ) -> Result<Self, PureError> {
        let output =
            serde_json::to_string(&value).map_err(|error| PureError::ToolExecutionFailed {
                tool: "registered_tool".to_string(),
                error: format!("failed to serialize JSON output: {error}"),
            })?;
        Ok(Self::with_model_budget(
            true,
            output,
            false,
            max_output_tokens,
            max_output_bytes,
            Vec::new(),
        ))
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
            end_turn_content: None,
            completed_plan_content: None,
            output_artifacts,
            output_bytes_budget: None,
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
        // self.model_output 已由 with_model_tokens / with_model_budget 投影过，
        // 这里直接作为 description，不再二次夹紧；最终字节预算由 dispatch 根据
        // OutputBudget 事件统一应用。
        let mut runtime_events = Vec::new();
        if !artifacts.is_empty() {
            runtime_events.push(ToolRuntimeEvent::OutputArtifacts { artifacts });
        }
        if let Some(content) = self.completed_plan_content {
            runtime_events.push(ToolRuntimeEvent::PlanCompleted { content });
        }
        runtime_events.push(ToolRuntimeEvent::OutputMetrics {
            raw_bytes,
            model_visible_bytes,
            artifact_bytes,
            result_hash: crate::canonical_content_hash(self.output.as_bytes()),
        });
        if let Some(max_bytes) = self.output_bytes_budget {
            runtime_events.push(ToolRuntimeEvent::OutputBudget { max_bytes });
        }
        if self.ends_turn {
            runtime_events.push(ToolRuntimeEvent::EndTurn {
                final_content: self.end_turn_content,
            });
        }
        ToolOutput {
            description: self.model_output,
            truncated: crate::OutputTruncation::empty(),
            output_file: std::path::PathBuf::new(),
            exit_code: if self.success { Some(0) } else { Some(1) },
            timed_out: false,
            runtime_events,
        }
    }
}

/// 汇总 artifact 描述符声明的完整输出字节数。
pub(crate) fn tool_output_artifact_bytes(artifacts: &[serde_json::Value]) -> u64 {
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
                vec![ToolRuntimeEvent::EndTurn {
                    final_content: None,
                }]
            } else {
                Vec::new()
            },
        }
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
                ToolRuntimeEvent::InteractionRequested { .. }
                | ToolRuntimeEvent::SkillActivated { .. }
                | ToolRuntimeEvent::PlanCompleted { .. }
                | ToolRuntimeEvent::AuditMetadata { .. }
                | ToolRuntimeEvent::ExecutionFailed => None,
                ToolRuntimeEvent::ToolResultRevision {
                    revision: _revision,
                } => None,
                ToolRuntimeEvent::CacheHit { .. } => None,
                ToolRuntimeEvent::OutputMetrics { .. } => None,
                ToolRuntimeEvent::OutputBudget { .. } => None,
                ToolRuntimeEvent::EndTurn { .. } => None,
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
            ToolRuntimeEvent::EndTurn { .. } => true,
            ToolRuntimeEvent::InteractionRequested { .. }
            | ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::PlanCompleted { .. }
            | ToolRuntimeEvent::AuditMetadata { .. }
            | ToolRuntimeEvent::ExecutionFailed => false,
            ToolRuntimeEvent::ToolResultRevision {
                revision: _revision,
            } => false,
            ToolRuntimeEvent::OutputArtifacts {
                artifacts: _artifacts,
            } => false,
            ToolRuntimeEvent::CacheHit { .. } => false,
            ToolRuntimeEvent::OutputMetrics { .. } => false,
            ToolRuntimeEvent::OutputBudget { .. } => false,
        })
    }

    /// 返回结束工具声明的 canonical 最终 assistant 回复。
    pub fn end_turn_content(&self) -> Option<&str> {
        self.runtime_events.iter().find_map(|event| match event {
            ToolRuntimeEvent::EndTurn {
                final_content: Some(content),
            } => Some(content.as_str()),
            ToolRuntimeEvent::InteractionRequested { .. }
            | ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::PlanCompleted { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::AuditMetadata { .. }
            | ToolRuntimeEvent::ExecutionFailed
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::OutputMetrics { .. }
            | ToolRuntimeEvent::OutputBudget { .. }
            | ToolRuntimeEvent::EndTurn {
                final_content: None,
            } => None,
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
        let mut result = ToolExecutionResult::with_model_output(
            self.exit_code.unwrap_or(0) == 0,
            self.description.clone(),
            self.description.clone(),
            self.ends_turn(),
            self.output_artifacts_as(),
        );
        result.end_turn_content = self.end_turn_content().map(str::to_string);
        result.completed_plan_content = self.runtime_events.iter().find_map(|event| match event {
            ToolRuntimeEvent::PlanCompleted { content } => Some(content.clone()),
            ToolRuntimeEvent::InteractionRequested { .. }
            | ToolRuntimeEvent::SkillActivated { .. }
            | ToolRuntimeEvent::ToolResultRevision { .. }
            | ToolRuntimeEvent::OutputArtifacts { .. }
            | ToolRuntimeEvent::AuditMetadata { .. }
            | ToolRuntimeEvent::ExecutionFailed
            | ToolRuntimeEvent::CacheHit { .. }
            | ToolRuntimeEvent::OutputMetrics { .. }
            | ToolRuntimeEvent::OutputBudget { .. }
            | ToolRuntimeEvent::EndTurn { .. } => None,
        });
        result
    }
}
