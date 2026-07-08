use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use pl_protocol::Result;

use crate::tool::{RegisteredTool, Tool};
use crate::trace::TraceRecorder;
use crate::turn::{TurnOptions, TurnRequest, TurnResult};

use super::{CoreRuntimeProfile, PureCore, PureCoreBuilder, ToolProfile};

/// 产品 agent 运行 profile。
///
/// 该类型复用 `CoreRuntimeProfile` 的 workspace、工具能力和运行选项配置，
/// 用于表达宿主产品的定制化 agent，而不是在宿主侧重新实现底层 turn loop。
pub type CoreAgentProfile = CoreRuntimeProfile;

/// pl-core agent runtime kernel。
///
/// 该类型统一持有 `PureCore`、profile 注册出的共享工具和宿主动态工具，
/// 让宿主只配置定制化 agent，不再复刻模型 turn loop 与通用 tool dispatch。
#[derive(Debug)]
pub struct AgentKernel {
    core: PureCore,
}

impl AgentKernel {
    pub fn builder(core_builder: PureCoreBuilder) -> AgentKernelBuilder {
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
pub struct AgentKernelBuilder {
    core_builder: PureCoreBuilder,
    profile: CoreAgentProfile,
    runtime_tools: Vec<Arc<dyn Tool>>,
    registered_tools: Vec<RegisteredTool>,
}

impl AgentKernelBuilder {
    fn new(core_builder: PureCoreBuilder) -> Self {
        Self {
            core_builder,
            profile: CoreAgentProfile::minimal(),
            runtime_tools: Vec::new(),
            registered_tools: Vec::new(),
        }
    }

    pub fn with_profile(mut self, profile: CoreAgentProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn with_registered_tool(mut self, tool: RegisteredTool) -> Self {
        self.registered_tools.push(tool);
        self
    }

    pub fn with_tool(mut self, tool: impl Tool + 'static) -> Self {
        self.runtime_tools.push(Arc::new(tool));
        self
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Arc<dyn Tool>>) -> Self {
        self.runtime_tools.extend(tools);
        self
    }

    pub fn with_registered_tools(
        mut self,
        tools: impl IntoIterator<Item = RegisteredTool>,
    ) -> Self {
        self.registered_tools.extend(tools);
        self
    }

    pub async fn build(self) -> AgentKernel {
        let mut core = self
            .core_builder
            .with_runtime_profile(self.profile.clone())
            .build();
        core.register_profile_tools().await;
        for tool in &self.runtime_tools {
            core.register_tool(tool.clone());
        }
        for tool in &self.registered_tools {
            core.register_tool(tool.clone());
        }
        core.agent_tool_registrar = Some(Arc::new(KernelToolRegistrar {
            profile: self.profile,
            runtime_tools: self.runtime_tools,
            registered_tools: self.registered_tools,
        }));
        AgentKernel { core }
    }
}

#[derive(Debug, Clone)]
struct KernelToolRegistrar {
    profile: CoreAgentProfile,
    runtime_tools: Vec<Arc<dyn Tool>>,
    registered_tools: Vec<RegisteredTool>,
}

impl crate::AgentToolRegistrar for KernelToolRegistrar {
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
            for tool in &self.runtime_tools {
                core.register_tool(tool.clone());
            }
            for tool in &self.registered_tools {
                core.register_tool(tool.clone());
            }
            Ok(())
        })
    }
}
