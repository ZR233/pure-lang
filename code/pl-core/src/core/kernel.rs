use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use pl_protocol::{PureError, Result};
use serde_json::Value;

use crate::tool::{Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeLockPolicy};
use crate::trace::TraceRecorder;
use crate::turn::{TurnOptions, TurnRequest, TurnResult};

use super::{CoreRuntimeProfile, PureCore, PureCoreBuilder, ToolProfile};

/// 产品 agent 运行 profile。
///
/// 该类型复用 `CoreRuntimeProfile` 的 workspace、工具能力和运行选项配置，
/// 用于表达宿主产品的定制化 agent，而不是在宿主侧重新实现底层 turn loop。
pub type CoreAgentProfile = CoreRuntimeProfile;

/// 产品工具定义。
///
/// 宿主只通过该结构向 `AgentKernel` 暴露产品语义工具；通用 file、git、
/// container、subagent 工具应由 pl-core 自身的 tool set 注册。
#[derive(Debug, Clone, PartialEq)]
pub struct ProductToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub supports_parallel_tool_calls: bool,
    pub runtime_lock_policy: Option<ToolRuntimeLockPolicy>,
}

impl ProductToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            supports_parallel_tool_calls: false,
            runtime_lock_policy: None,
        }
    }

    pub fn with_parallel_tool_calls(mut self) -> Self {
        self.supports_parallel_tool_calls = true;
        self
    }

    pub fn with_runtime_lock_policy(mut self, policy: ToolRuntimeLockPolicy) -> Self {
        self.runtime_lock_policy = Some(policy);
        self
    }
}

/// 产品工具执行请求。
///
/// `AgentKernel` 已经完成模型 tool call 到 `ToolInput` 的转换、trace 记录和
/// 权限上下文注入；产品层只需要执行自身业务并返回模型可见的工具输出。
#[derive(Clone)]
pub struct ProductToolRequest {
    pub definition: ProductToolDefinition,
    pub input: ToolInput,
    pub context: ToolContext,
}

impl fmt::Debug for ProductToolRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductToolRequest")
            .field("tool", &self.definition.name)
            .field("input", &self.input)
            .field("context", &self.context)
            .finish()
    }
}

/// 产品工具路由器。
///
/// 宿主实现该 trait 来注入 GitHub、review queue、artifact、task plan 等
/// 产品工具。实现必须保持无共享底层 turn loop 责任：tool call 生命周期、
/// trace、subagent 控制和通用 backend 均由 `AgentKernel` / pl-core 负责。
pub trait ProductToolRouter: fmt::Debug + Clone + Send + Sync {
    fn tool_definitions(&self) -> Vec<ProductToolDefinition>;

    fn execute(
        &self,
        request: ProductToolRequest,
    ) -> impl std::future::Future<Output = Result<ToolOutput>> + Send;
}

/// 不暴露任何产品工具的默认 router。
#[derive(Debug, Clone, Default)]
pub struct EmptyProductToolRouter;

impl ProductToolRouter for EmptyProductToolRouter {
    fn tool_definitions(&self) -> Vec<ProductToolDefinition> {
        Vec::new()
    }

    async fn execute(&self, request: ProductToolRequest) -> Result<ToolOutput> {
        Err(PureError::ToolExecutionFailed {
            tool: request.definition.name,
            error: "product tool router is not configured".to_string(),
        })
    }
}

/// pl-core agent runtime kernel。
///
/// 该类型统一持有 `PureCore`、profile 注册出的共享工具和产品工具 router，
/// 让宿主只配置定制化 agent，不再复刻模型 turn loop 与通用 tool dispatch。
#[derive(Debug)]
pub struct AgentKernel {
    core: PureCore,
}

impl AgentKernel {
    pub fn builder(core_builder: PureCoreBuilder) -> AgentKernelBuilder<EmptyProductToolRouter> {
        AgentKernelBuilder::new(core_builder)
    }

    pub fn core(&self) -> &PureCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut PureCore {
        &mut self.core
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.core
            .tools
            .names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut crate::CoreSession,
        request: TurnRequest,
        event_tx: pl_trace::AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.core.run_turn(session, request, event_tx)
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut crate::CoreSession,
        request: TurnRequest,
        event_tx: pl_trace::AgentEventSender,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        self.core
            .run_turn_with_options(session, request, event_tx, options)
            .await
    }

    pub async fn run_turn_with_trace(
        &self,
        session: &mut crate::CoreSession,
        request: TurnRequest,
        recorder: &mut TraceRecorder,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        self.core
            .run_turn_with_trace(session, request, recorder, options)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct AgentKernelBuilder<R = EmptyProductToolRouter> {
    core_builder: PureCoreBuilder,
    profile: CoreAgentProfile,
    product_tool_router: R,
}

impl AgentKernelBuilder<EmptyProductToolRouter> {
    fn new(core_builder: PureCoreBuilder) -> Self {
        Self {
            core_builder,
            profile: CoreAgentProfile::minimal(),
            product_tool_router: EmptyProductToolRouter,
        }
    }
}

impl<R> AgentKernelBuilder<R>
where
    R: ProductToolRouter + 'static,
{
    pub fn with_profile(mut self, profile: CoreAgentProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_product_tool_router<N>(self, product_tool_router: N) -> AgentKernelBuilder<N>
    where
        N: ProductToolRouter + 'static,
    {
        AgentKernelBuilder {
            core_builder: self.core_builder,
            profile: self.profile,
            product_tool_router,
        }
    }

    pub async fn build(self) -> AgentKernel {
        let mut core = self
            .core_builder
            .with_runtime_profile(self.profile.clone())
            .build();
        core.register_profile_tools().await;
        for definition in self.product_tool_router.tool_definitions() {
            core.register_tool(ProductTool::new(
                definition,
                self.product_tool_router.clone(),
            ));
        }
        core.agent_tool_registrar = Some(Arc::new(ProductToolRegistrar {
            profile: self.profile,
            router: self.product_tool_router,
        }));
        AgentKernel { core }
    }
}

#[derive(Debug, Clone)]
struct ProductToolRegistrar<R> {
    profile: CoreAgentProfile,
    router: R,
}

impl<R> crate::AgentToolRegistrar for ProductToolRegistrar<R>
where
    R: ProductToolRouter + 'static,
{
    fn register_tools<'a>(
        &'a self,
        core: &'a mut PureCore,
        workspace_root: PathBuf,
        workspace_instructions: Option<String>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            match self.profile.tool_profile {
                ToolProfile::LocalWorkspace => {
                    core.register_default_tools(workspace_root, workspace_instructions)
                        .await;
                }
                ToolProfile::HostProvided | ToolProfile::Minimal => {
                    core.workspace_root = Some(workspace_root);
                    core.workspace_instructions = workspace_instructions;
                }
            }
            for definition in self.router.tool_definitions() {
                core.register_tool(ProductTool::new(definition, self.router.clone()));
            }
            Ok(())
        })
    }
}

#[derive(Debug, Clone)]
struct ProductTool<R> {
    definition: ProductToolDefinition,
    router: R,
}

impl<R> ProductTool<R> {
    fn new(definition: ProductToolDefinition, router: R) -> Self {
        Self { definition, router }
    }
}

impl<R> Tool for ProductTool<R>
where
    R: ProductToolRouter + 'static,
{
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn description(&self) -> &str {
        &self.definition.description
    }

    fn input_schema(&self) -> Value {
        self.definition.input_schema.clone()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.definition.supports_parallel_tool_calls
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        self.definition.runtime_lock_policy.unwrap_or_else(|| {
            if self.supports_parallel_tool_calls() {
                ToolRuntimeLockPolicy::Shared
            } else {
                ToolRuntimeLockPolicy::Exclusive
            }
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput>> + Send + 'a>> {
        let router = self.router.clone();
        let definition = self.definition.clone();
        Box::pin(async move {
            router
                .execute(ProductToolRequest {
                    definition,
                    input,
                    context,
                })
                .await
        })
    }
}
