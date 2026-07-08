use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use pl_protocol::{AgentStatus, PureError, Result};
use serde::{Deserialize, Serialize};

use super::super::{
    BoxFuture, OutputTruncation, Tool, ToolContext, ToolInput, ToolOutput, ToolRuntimeLockPolicy,
};
use super::schema::AgentControlToolKind;

/// 宿主产品提供的 agent-control 执行后端。
///
/// pl-core 负责统一模型可见 schema、输入解析、输出 JSON、trace 和 tool result
/// history；实现方只负责把共享控制语义映射到自己的 agent 生命周期、权限策略和
/// 持久化系统。trait 方法使用 RPITIT，便于宿主用轻量 async 实现接入。
pub trait AgentControlBackend: fmt::Debug + Send + Sync {
    fn spawn_agent(
        &self,
        request: AgentControlSpawnRequest,
    ) -> impl Future<Output = Result<AgentControlSpawnOutput>> + Send;

    fn send_input(
        &self,
        request: AgentControlSendInputRequest,
    ) -> impl Future<Output = Result<AgentControlSendInputOutput>> + Send;

    fn wait_agent(
        &self,
        request: AgentControlWaitRequest,
    ) -> impl Future<Output = Result<AgentControlWaitOutput>> + Send;

    fn list_agents(
        &self,
        request: AgentControlListRequest,
    ) -> impl Future<Output = Result<AgentControlListOutput>> + Send;

    fn close_agent(
        &self,
        request: AgentControlTargetRequest,
    ) -> impl Future<Output = Result<AgentControlMessageOutput>> + Send;

    fn resume_agent(
        &self,
        request: AgentControlTargetRequest,
    ) -> impl Future<Output = Result<AgentControlMessageOutput>> + Send;
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
pub struct AgentControlTool<B> {
    kind: AgentControlToolKind,
    backend: Arc<B>,
}

impl<B> AgentControlTool<B> {
    pub fn new(kind: AgentControlToolKind, backend: Arc<B>) -> Self {
        Self { kind, backend }
    }
}

impl<B> Tool for AgentControlTool<B>
where
    B: AgentControlBackend + 'static,
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
        _context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput>> {
        Box::pin(async move {
            match self.kind {
                AgentControlToolKind::SpawnAgent => json_output(
                    self.backend
                        .spawn_agent(parse_input(self.name(), input)?)
                        .await?,
                ),
                AgentControlToolKind::SendInput => json_output(
                    self.backend
                        .send_input(parse_input(self.name(), input)?)
                        .await?,
                ),
                AgentControlToolKind::WaitAgent => json_output(
                    self.backend
                        .wait_agent(parse_input(self.name(), input)?)
                        .await?,
                ),
                AgentControlToolKind::ListAgents => json_output(
                    self.backend
                        .list_agents(parse_input(self.name(), input)?)
                        .await?,
                ),
                AgentControlToolKind::CloseAgent => json_output(
                    self.backend
                        .close_agent(parse_input(self.name(), input)?)
                        .await?,
                ),
                AgentControlToolKind::ResumeAgent => json_output(
                    self.backend
                        .resume_agent(parse_input(self.name(), input)?)
                        .await?,
                ),
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
