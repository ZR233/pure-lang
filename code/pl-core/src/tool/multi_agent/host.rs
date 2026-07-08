use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use pl_protocol::{AgentStatus, Message, PureError, Result};
use serde::{Deserialize, Serialize};

use crate::agent::AgentInputTurnMode;

use super::super::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeLockPolicy,
};
use super::schema::AgentControlToolKind;
use super::{ForkTurns, fork_session};

const DEFAULT_AGENT_CONTROL_WAIT_TIMEOUT_MS: i64 = 30_000;
const MIN_AGENT_CONTROL_WAIT_TIMEOUT_MS: i64 = 100;

/// 宿主产品提供的 agent-control 执行后端。
///
/// pl-core 负责统一模型可见 schema、输入解析、输出 JSON、trace 和 tool result
/// history；实现方只负责把共享控制语义映射到自己的 agent 生命周期、权限策略和
/// 持久化系统。trait 方法使用 RPITIT，便于宿主用轻量 async 实现接入。
pub trait AgentControlBackend: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn spawn_agent(
        &self,
        request: AgentControlSpawnRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlSpawnOutput, Self::Error>> + Send;

    fn send_input(
        &self,
        request: AgentControlSendInputRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlSendInputOutput, Self::Error>> + Send;

    fn wait_agent(
        &self,
        request: AgentControlWaitRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlWaitOutput, Self::Error>> + Send;

    fn list_agents(
        &self,
        request: AgentControlListRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlListOutput, Self::Error>> + Send;

    fn close_agent(
        &self,
        request: AgentControlTargetRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlMessageOutput, Self::Error>> + Send;

    fn resume_agent(
        &self,
        request: AgentControlTargetRequest,
    ) -> impl Future<Output = std::result::Result<AgentControlMessageOutput, Self::Error>> + Send;
}

/// 宿主产品提供的 agent-control 权限策略。
///
/// pl-core 在共享 agent-control 工具执行 backend 之前统一调用该策略，保证工具
/// 可见性、目标通信边界和产品权限检查属于 shared tool lifecycle，而不是散落在
/// backend 的业务执行分支里。实现方应只检查权限，不执行状态变更。
pub trait AgentControlPolicy: fmt::Debug + Send + Sync {
    type Error: fmt::Display + Send + 'static;

    fn check_tool(
        &self,
        kind: AgentControlToolKind,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn check_target(
        &self,
        kind: AgentControlToolKind,
        target: &str,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;
}

/// 共享 `spawn_agent.agentType` 可识别的 agent 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentControlAgentType {
    Planner,
    Explorer,
    Executor,
    Reviewer,
}

impl AgentControlAgentType {
    /// 按模型可见协议解析 agent 类型，并保留历史 executor alias。
    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "planner" => Some(Self::Planner),
            "explorer" => Some(Self::Explorer),
            "executor" | "worker" | "default" | "" => Some(Self::Executor),
            "reviewer" => Some(Self::Reviewer),
            _other => None,
        }
    }

    /// 返回模型可见 canonical 名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Explorer => "explorer",
            Self::Executor => "executor",
            Self::Reviewer => "reviewer",
        }
    }
}

/// `spawn_agent.agentType` 解析后的共享策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentControlAgentTypePolicy {
    pub kind: AgentControlAgentType,
    pub role_profile_requested: bool,
}

/// 默认允许所有 agent-control 调用的策略。
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllAgentControlPolicy;

impl AgentControlPolicy for AllowAllAgentControlPolicy {
    type Error = String;

    async fn check_tool(
        &self,
        _kind: AgentControlToolKind,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    async fn check_target(
        &self,
        _kind: AgentControlToolKind,
        _target: &str,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControlSpawnRequest {
    pub task_name: String,
    pub message: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub fork_turns: Option<String>,
    #[serde(default)]
    pub skill_mentions: Vec<String>,
    #[serde(skip)]
    pub forked_messages: Option<Vec<Message>>,
}

impl AgentControlSpawnRequest {
    /// 返回可注入子 agent 首轮输入的初始消息。
    ///
    /// 模型可能用空白字符串表示只创建 agent、不立刻发起协作输入；该归一化属于
    /// `spawn_agent` 的共享输入语义，由 pl-core 统一提供给宿主 adapter。
    pub fn initial_message(&self) -> Option<String> {
        (!self.message.trim().is_empty()).then(|| self.message.clone())
    }

    /// 返回共享 agent 类型策略。
    ///
    /// 省略、空值和历史 alias 都按 executor 执行，但只有模型可见 canonical
    /// role 名称会请求宿主使用对应角色 profile。
    pub fn agent_type_policy(&self) -> AgentControlAgentTypePolicy {
        let value = self.agent_type.as_deref().unwrap_or_default();
        let kind =
            AgentControlAgentType::from_label(value).unwrap_or(AgentControlAgentType::Executor);
        let role_profile_requested = self.agent_type.as_deref().is_some_and(|value| {
            matches!(
                value.trim().to_lowercase().as_str(),
                "planner" | "explorer" | "executor" | "reviewer"
            )
        });
        AgentControlAgentTypePolicy {
            kind,
            role_profile_requested,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlSpawnOutput {
    pub agent_id: String,
    pub task_name: String,
    pub path: String,
    pub status: AgentStatus,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControlSendInputRequest {
    pub target: String,
    pub message: String,
    #[serde(default)]
    pub trigger_turn: bool,
    #[serde(default)]
    pub interrupt: bool,
    #[serde(default)]
    pub skill_mentions: Vec<String>,
}

impl AgentControlSendInputRequest {
    /// 返回共享 input turn mode，并在 pl-core 内统一表达 `interrupt` 隐含启动 turn。
    pub fn turn_mode(&self) -> AgentInputTurnMode {
        AgentInputTurnMode::from_codex_flags(self.trigger_turn || self.interrupt, self.interrupt)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlSendInputOutput {
    pub target: String,
    pub status: AgentStatus,
    pub interrupt: bool,
    pub queued: bool,
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControlWaitRequest {
    pub target: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
    pub timeout_ms: Option<i64>,
}

impl AgentControlWaitRequest {
    /// 返回经过共享默认值和下限归一化后的等待时长。
    pub fn timeout_duration(&self) -> Duration {
        let timeout_ms = self
            .timeout_ms
            .and_then(|value| (value >= 0).then_some(value))
            .unwrap_or(DEFAULT_AGENT_CONTROL_WAIT_TIMEOUT_MS)
            .max(MIN_AGENT_CONTROL_WAIT_TIMEOUT_MS) as u64;
        Duration::from_millis(timeout_ms)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlWaitOutput {
    pub message: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControlListRequest {
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlListOutput {
    pub agents: Vec<AgentControlAgentRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlAgentRecord {
    pub path: String,
    pub status: AgentStatus,
    pub role: String,
    pub task: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentControlTargetRequest {
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentControlMessageOutput {
    pub target: String,
    pub status: AgentStatus,
}

/// 使用宿主后端执行共享 agent-control schema 的工具。
#[derive(Debug, Clone)]
pub struct AgentControlTool<B, P = AllowAllAgentControlPolicy> {
    kind: AgentControlToolKind,
    backend: Arc<B>,
    policy: Arc<P>,
}

impl<B> AgentControlTool<B, AllowAllAgentControlPolicy> {
    pub fn new(kind: AgentControlToolKind, backend: Arc<B>) -> Self {
        Self {
            kind,
            backend,
            policy: Arc::new(AllowAllAgentControlPolicy),
        }
    }
}

impl<B, P> AgentControlTool<B, P> {
    pub fn with_policy(kind: AgentControlToolKind, backend: Arc<B>, policy: Arc<P>) -> Self {
        Self {
            kind,
            backend,
            policy,
        }
    }
}

impl<B, P> Tool for AgentControlTool<B, P>
where
    B: AgentControlBackend + 'static,
    P: AgentControlPolicy + 'static,
{
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        self.kind.description()
    }

    fn input_schema(&self) -> serde_json::Value {
        self.kind.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        matches!(self.kind, AgentControlToolKind::SpawnAgent)
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        match self.kind {
            AgentControlToolKind::SpawnAgent
            | AgentControlToolKind::WaitAgent
            | AgentControlToolKind::ListAgents => ToolRuntimeLockPolicy::Shared,
            AgentControlToolKind::SendInput
            | AgentControlToolKind::CloseAgent
            | AgentControlToolKind::ResumeAgent => ToolRuntimeLockPolicy::Exclusive,
        }
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        Box::pin(async move {
            match self.kind {
                AgentControlToolKind::SpawnAgent => {
                    let mut request: AgentControlSpawnRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    let fork_turns = ForkTurns::parse(request.fork_turns.as_deref())?;
                    request.forked_messages = match fork_turns {
                        ForkTurns::None => None,
                        ForkTurns::All | ForkTurns::Last(_) => Some(
                            fork_session(&context.parent_session, fork_turns)
                                .messages()
                                .to_vec(),
                        ),
                    };
                    let output = self
                        .backend
                        .spawn_agent(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
                AgentControlToolKind::SendInput => {
                    let request: AgentControlSendInputRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    self.policy
                        .check_target(self.kind, &request.target)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    let output = self
                        .backend
                        .send_input(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
                AgentControlToolKind::WaitAgent => {
                    let request: AgentControlWaitRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    if let Some(target) = &request.target {
                        self.policy
                            .check_target(self.kind, target)
                            .await
                            .map_err(|error| tool_error(self.name(), error))?;
                    }
                    for target in &request.targets {
                        self.policy
                            .check_target(self.kind, target)
                            .await
                            .map_err(|error| tool_error(self.name(), error))?;
                    }
                    let output = self
                        .backend
                        .wait_agent(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
                AgentControlToolKind::ListAgents => {
                    let request: AgentControlListRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    let output = self
                        .backend
                        .list_agents(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
                AgentControlToolKind::CloseAgent => {
                    let request: AgentControlTargetRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    self.policy
                        .check_target(self.kind, &request.target)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    let output = self
                        .backend
                        .close_agent(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
                AgentControlToolKind::ResumeAgent => {
                    let request: AgentControlTargetRequest = parse_input(self.name(), input)?;
                    self.policy
                        .check_tool(self.kind)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    self.policy
                        .check_target(self.kind, &request.target)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    let output = self
                        .backend
                        .resume_agent(request)
                        .await
                        .map_err(|error| tool_error(self.name(), error))?;
                    json_output(output)
                }
            }
        })
    }
}

fn parse_input<T: serde::de::DeserializeOwned>(tool: &str, input: ToolInput) -> Result<T> {
    serde_json::from_value(input.arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn tool_error(tool: &str, error: impl fmt::Display) -> PureError {
    PureError::ToolExecutionFailed {
        tool: tool.to_string(),
        error: error.to_string(),
    }
}

fn json_output(value: impl Serialize) -> Result<ToolOutput> {
    Ok(ToolOutput {
        description: serde_json::to_string(&value)?,
        truncated: OutputTruncation::empty(),
        output_file: PathBuf::new(),
        exit_code: None,
        timed_out: false,
        runtime_events: Vec::new(),
    })
}
