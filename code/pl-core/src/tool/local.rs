use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

use futures::FutureExt;
use futures::future::BoxFuture;
use pl_protocol::{InteractionRequest, Result, SkillActivation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    DynTool, StaticTool, StaticToolDefinition, ToolCallContext, ToolDefinition, ToolExecution,
    ToolExecutor, ToolInvocation, ToolPolicy, ToolResult,
};

/// 与执行授权隔离的工具展示元数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDisplayMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
/// Typed runtime facts emitted alongside model-visible tool output.
pub enum ToolDirective {
    /// 持久化一个由当前 Turn 发起、需要宿主后续处理的交互。
    InteractionRequested {
        interaction: Box<InteractionRequest>,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    ToolResultRevision {
        revision: u64,
    },
    OutputArtifacts {
        artifacts: Vec<serde_json::Value>,
    },
    /// Reveal deferred tools for the next model step.
    RevealTools {
        catalog_fingerprint: String,
        tool_names: Vec<String>,
    },
    /// 与模型可见文本分离的 typed 审计数据。
    AuditMetadata {
        metadata: serde_json::Value,
    },
    /// handler 已产生完整输出，但该输出表示一次模型可恢复的工具失败。
    ExecutionFailed,
    CacheHit {
        reused_from_call_id: String,
        result_hash: String,
        total_bytes: u64,
    },
    OutputMetrics {
        raw_bytes: u64,
        model_visible_bytes: u64,
        artifact_bytes: u64,
        result_hash: String,
    },
    /// 声明该工具输出需要越过默认 12KB 安全阈值的硬字节上限。
    OutputBudget {
        max_bytes: usize,
    },
    /// 当前工具结果结束本轮；可选内容成为该 Turn 的 canonical 最终 assistant 回复。
    EndTurn {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        final_content: Option<String>,
    },
}

/// Starts building a statically typed function tool.
pub fn static_tool<Input>(definition: StaticToolDefinition) -> StaticToolBuilder<Input> {
    StaticToolBuilder {
        definition,
        policy: ToolPolicy::default(),
        marker: PhantomData,
    }
}

/// Public typed builder for embedding applications and simple built-in tools.
pub struct StaticToolBuilder<Input> {
    definition: StaticToolDefinition,
    policy: ToolPolicy,
    marker: PhantomData<fn() -> Input>,
}

impl<Input> fmt::Debug for StaticToolBuilder<Input> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StaticToolBuilder")
            .field("definition", &self.definition)
            .field("policy", &self.policy)
            .finish()
    }
}

impl<Input> StaticToolBuilder<Input> {
    /// Replaces the default tool policy.
    pub fn policy(mut self, policy: ToolPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl<Input> StaticToolBuilder<Input>
where
    Input: DeserializeOwned + JsonSchema + Send + 'static,
{
    /// Builds and type-erases a closure-backed static tool.
    pub fn build<F, Fut>(self, handler: F) -> DynTool
    where
        F: Fn(Input, ToolCallContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult>> + Send + 'static,
    {
        ClosureStaticTool {
            definition: self.definition,
            policy: self.policy,
            handler,
            marker: PhantomData,
        }
        .into()
    }
}

struct ClosureStaticTool<Input, Handler> {
    definition: StaticToolDefinition,
    policy: ToolPolicy,
    handler: Handler,
    marker: PhantomData<fn() -> Input>,
}

impl<Input, Handler> fmt::Debug for ClosureStaticTool<Input, Handler> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClosureStaticTool")
            .field("definition", &self.definition)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl<Input, Handler, Fut> StaticTool for ClosureStaticTool<Input, Handler>
where
    Input: DeserializeOwned + JsonSchema + Send + 'static,
    Handler: Fn(Input, ToolCallContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<ToolResult>> + Send + 'static,
{
    type Input = Input;

    fn definition(&self) -> StaticToolDefinition {
        self.definition.clone()
    }

    fn policy(&self) -> ToolPolicy {
        self.policy.clone()
    }

    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        (self.handler)(input, context)
    }
}

type DynamicToolHandler =
    dyn Fn(ToolInvocation) -> BoxFuture<'static, Result<ToolResult>> + Send + Sync;

/// Closure-backed dynamic executor for runtime-discovered tool definitions.
pub struct DynamicToolExecutor {
    definition: ToolDefinition,
    policy: ToolPolicy,
    execution: ToolExecution,
    handler: Arc<DynamicToolHandler>,
}

impl DynamicToolExecutor {
    /// Builds an executor from a runtime definition, policy, owner and handler.
    pub fn new<F, Fut>(
        definition: ToolDefinition,
        policy: ToolPolicy,
        execution: ToolExecution,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInvocation) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolResult>> + Send + 'static,
    {
        Self {
            definition,
            policy,
            execution,
            handler: Arc::new(move |invocation| handler(invocation).boxed()),
        }
    }
}

impl fmt::Debug for DynamicToolExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DynamicToolExecutor")
            .field("name", &self.definition.name())
            .field("policy", &self.policy)
            .field("execution", &self.execution)
            .finish_non_exhaustive()
    }
}

impl ToolExecutor for DynamicToolExecutor {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    fn execution(&self) -> ToolExecution {
        self.execution
    }

    fn execute(&self, invocation: ToolInvocation) -> BoxFuture<'_, Result<ToolResult>> {
        if invocation.context.is_cancelled() {
            let tool = self.definition.name().wire_name().to_string();
            return async move {
                Err(pl_protocol::PureError::ToolExecutionFailed {
                    tool,
                    error: "tool execution cancelled".to_string(),
                })
            }
            .boxed();
        }
        (self.handler)(invocation)
    }
}
