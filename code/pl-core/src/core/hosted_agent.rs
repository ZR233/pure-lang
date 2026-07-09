use std::fmt;
use std::future::Future;

use pl_protocol::{Message, PureError};
use pl_trace::{AgentEvent, TraceEvent};

use crate::RegisteredTool;
use crate::session::CoreSession;
use crate::trace::TraceRecorder;
use crate::turn::{TurnOptions, TurnRequest, TurnResult};

use super::AgentKernel;

/// 宿主产品发起一次 agent turn 的稳定标识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedTurnRequest {
    session_id: String,
    turn_id: String,
}

impl HostedTurnRequest {
    pub fn new(session_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }
}

/// 宿主为 pl-core 准备好的 turn 执行上下文。
///
/// 宿主负责选择模型并注入产品语义；pl-core runner 负责统一装载
/// CoreSession、trace、turn loop 和 completion 投影。
#[derive(Debug)]
pub struct HostedTurnPreparation {
    request: HostedTurnRequest,
    kernel: AgentKernel,
    history: Vec<Message>,
    turn_request: TurnRequest,
    options: TurnOptions,
    trace_revision_base: u64,
    event_channel_capacity: usize,
}

impl HostedTurnPreparation {
    pub fn new(
        request: HostedTurnRequest,
        kernel: AgentKernel,
        history: Vec<Message>,
        turn_request: TurnRequest,
        options: TurnOptions,
    ) -> Self {
        Self {
            request,
            kernel,
            history,
            turn_request,
            options,
            trace_revision_base: 0,
            event_channel_capacity: 64,
        }
    }

    pub fn with_trace_revision_base(mut self, revision_base: u64) -> Self {
        self.trace_revision_base = revision_base;
        self
    }

    pub fn with_event_channel_capacity(mut self, capacity: usize) -> Self {
        self.event_channel_capacity = capacity.max(1);
        self
    }
}

/// 产品宿主向 hosted agent runner 提供自定义工具的注册端口。
///
/// 实现方只暴露产品工具的 schema 与 handler；工具生命周期、trace 记录、
/// tool result 投影和调度错误归一化仍由 pl-core 的 registry 负责。
pub trait HostedProductToolRegistrar: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn register_product_tools(
        &self,
        request: HostedTurnRequest,
    ) -> impl Future<Output = std::result::Result<Vec<RegisteredTool>, Self::Error>> + Send;
}

/// pl-core 完成一次 hosted agent turn 后交还给宿主的结果。
#[derive(Debug)]
pub struct HostedTurnCompletion {
    request: HostedTurnRequest,
    session: CoreSession,
    result: TurnResult,
}

impl HostedTurnCompletion {
    fn new(request: HostedTurnRequest, session: CoreSession, result: TurnResult) -> Self {
        Self {
            request,
            session,
            result,
        }
    }

    pub fn session_id(&self) -> &str {
        self.request.session_id()
    }

    pub fn turn_id(&self) -> &str {
        self.request.turn_id()
    }

    pub fn session(&self) -> &CoreSession {
        &self.session
    }

    pub fn result(&self) -> &TurnResult {
        &self.result
    }

    pub fn trace_events(&self) -> &[TraceEvent] {
        &self.result.trace_events
    }

    pub fn into_parts(self) -> (HostedTurnRequest, CoreSession, TurnResult) {
        (self.request, self.session, self.result)
    }
}

/// 产品宿主接入 pl-core hosted agent runner 的端口。
///
/// 实现方只提供产品语义：准备模型和工具、消费运行事件、持久化完成结果。
/// 通用 turn loop、trace recorder、session 更新和工具生命周期由
/// `HostedAgentRunner` 统一维护。
pub trait HostedAgentRuntime: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn prepare_turn(
        &self,
        request: HostedTurnRequest,
    ) -> impl Future<Output = std::result::Result<HostedTurnPreparation, Self::Error>> + Send;

    fn handle_event(
        &self,
        event: AgentEvent,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;

    fn complete_turn(
        &self,
        completion: HostedTurnCompletion,
    ) -> impl Future<Output = std::result::Result<(), Self::Error>> + Send;
}

/// hosted agent turn 执行错误。
#[derive(Debug)]
pub enum HostedAgentRunError<E> {
    Prepare(E),
    Core(PureError),
    Event(E),
    EventTaskJoin(String),
    Complete(E),
}

impl<E> fmt::Display for HostedAgentRunError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Prepare(error) => write!(formatter, "hosted turn preparation failed: {error}"),
            Self::Core(error) => write!(formatter, "hosted turn failed: {error}"),
            Self::Event(error) => write!(formatter, "hosted turn event handling failed: {error}"),
            Self::EventTaskJoin(error) => {
                write!(formatter, "hosted turn event task failed: {error}")
            }
            Self::Complete(error) => write!(formatter, "hosted turn completion failed: {error}"),
        }
    }
}

impl<E> std::error::Error for HostedAgentRunError<E> where E: std::error::Error + 'static {}

/// pl-core hosted agent runner。
#[derive(Debug, Clone)]
pub struct HostedAgentRunner<R> {
    runtime: R,
}

impl<R> HostedAgentRunner<R>
where
    R: HostedAgentRuntime,
{
    pub fn new(runtime: R) -> Self {
        Self { runtime }
    }

    pub async fn run(
        &self,
        request: HostedTurnRequest,
    ) -> std::result::Result<(), HostedAgentRunError<R::Error>> {
        let preparation = self
            .runtime
            .prepare_turn(request)
            .await
            .map_err(HostedAgentRunError::Prepare)?;
        let (event_tx, mut event_rx) =
            tokio::sync::broadcast::channel(preparation.event_channel_capacity);
        let event_runtime = self.runtime.clone();
        let event_task = tokio::spawn(async move {
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        let terminal = matches!(event, AgentEvent::Done | AgentEvent::Error { .. });
                        event_runtime
                            .handle_event(event)
                            .await
                            .map_err(HostedAgentRunError::Event)?;
                        if terminal {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            Ok::<(), HostedAgentRunError<R::Error>>(())
        });
        let mut recorder = TraceRecorder::new(
            preparation.request.session_id().to_string(),
            event_tx,
            preparation.trace_revision_base,
        );
        let mut session = CoreSession::from_messages(preparation.history);
        let result = preparation
            .kernel
            .run_turn_with_trace(
                &mut session,
                preparation.turn_request,
                &mut recorder,
                preparation.options,
            )
            .await
            .map_err(HostedAgentRunError::Core);
        drop(recorder);
        event_task
            .await
            .map_err(|error| HostedAgentRunError::EventTaskJoin(error.to_string()))??;
        let result = result?;
        self.runtime
            .complete_turn(HostedTurnCompletion::new(
                preparation.request,
                session,
                result,
            ))
            .await
            .map_err(HostedAgentRunError::Complete)
    }
}
