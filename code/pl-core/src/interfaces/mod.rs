use std::future::Future;

use anyhow::Result;
use pl_protocol::{AgentEvent, Message, TraceEvent};

use crate::{
    ConfigStore, CoreSession, ProjectRecord, SessionRecord, SessionRuntimeRecord,
    StudioAgentEventRecord, StudioStore, SubagentEventRecord, ToolApprovalRecord, TurnResult,
};

/// 会话与项目存储端口。
///
/// application 层通过该端口访问会话、消息和项目数据，不感知具体数据库细节。
pub trait SessionRepository: Send + Sync {
    fn list_projects(&self) -> impl Future<Output = Result<Vec<ProjectRecord>>> + Send;
    fn upsert_project(
        &self,
        path: &std::path::Path,
    ) -> impl Future<Output = Result<ProjectRecord>> + Send;
    fn list_sessions(
        &self,
        project_id: &str,
    ) -> impl Future<Output = Result<Vec<SessionRecord>>> + Send;
    fn create_session(
        &self,
        project_id: &str,
        title: &str,
        mode: crate::CompileMode,
    ) -> impl Future<Output = Result<SessionRecord>> + Send;
    fn load_core_session(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<CoreSession>> + Send;
    fn load_messages(&self, session_id: &str) -> impl Future<Output = Result<Vec<Message>>> + Send;
    fn append_messages(
        &self,
        session_id: &str,
        messages: &[Message],
    ) -> impl Future<Output = Result<()>> + Send;
    fn rename_session(
        &self,
        session_id: &str,
        title: &str,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// 配置读取端口。
///
/// application 层通过该端口加载运行配置，不直接操作 TOML 文件。
pub trait ConfigRepository: Send + Sync {
    fn load_or_default(&self) -> impl Future<Output = Result<crate::PureConfig>> + Send;
}

/// 运行时事件与 trace 落盘端口。
///
/// 用于将运行中的事件统一持久化，保证 timeline 可回放。
pub trait EventSink: Send + Sync {
    fn append_trace_events(&self, events: &[TraceEvent])
    -> impl Future<Output = Result<()>> + Send;
    fn record_subagent_event(
        &self,
        record: SubagentEventRecord,
    ) -> impl Future<Output = Result<()>> + Send;
    fn record_agent_event(
        &self,
        record: StudioAgentEventRecord,
    ) -> impl Future<Output = Result<()>> + Send;
    fn record_tool_approval(
        &self,
        record: ToolApprovalRecord,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// 模型 turn 结果持久化端口。
///
/// application 层通过该端口更新运行快照与 turn 结果相关数据。
pub trait TurnSnapshotRepository: Send + Sync {
    fn upsert_session_runtime(
        &self,
        session_id: &str,
        result: &TurnResult,
        model: Option<&pl_model::ModelInfo>,
    ) -> impl Future<Output = Result<()>> + Send;
    fn load_session_runtime(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Option<SessionRuntimeRecord>>> + Send;
}

/// 运行时事件分发端口。
///
/// 默认实现由内存通道承担，便于替换为其它事件总线。
pub trait RuntimeEventEmitter: Send + Sync {
    fn emit_event(&self, event: AgentEvent) -> impl Future<Output = Result<()>> + Send;
}

impl SessionRepository for StudioStore {
    async fn list_projects(&self) -> Result<Vec<ProjectRecord>> {
        self.list_projects().await
    }

    async fn upsert_project(&self, path: &std::path::Path) -> Result<ProjectRecord> {
        self.upsert_project(path).await
    }

    async fn list_sessions(&self, project_id: &str) -> Result<Vec<SessionRecord>> {
        self.list_sessions(project_id).await
    }

    async fn create_session(
        &self,
        project_id: &str,
        title: &str,
        mode: crate::CompileMode,
    ) -> Result<SessionRecord> {
        self.create_session(project_id, title, mode).await
    }

    async fn load_core_session(&self, session_id: &str) -> Result<CoreSession> {
        self.load_core_session(session_id).await
    }

    async fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        self.load_messages(session_id).await
    }

    async fn append_messages(&self, session_id: &str, messages: &[Message]) -> Result<()> {
        self.append_messages(session_id, messages).await
    }

    async fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        self.rename_session(session_id, title).await
    }
}

impl ConfigRepository for ConfigStore {
    async fn load_or_default(&self) -> Result<crate::PureConfig> {
        Ok(self.load_or_default()?)
    }
}

impl EventSink for StudioStore {
    async fn append_trace_events(&self, events: &[TraceEvent]) -> Result<()> {
        self.append_trace_events(events).await
    }

    async fn record_subagent_event(&self, record: SubagentEventRecord) -> Result<()> {
        self.record_subagent_event(record).await
    }

    async fn record_agent_event(&self, record: StudioAgentEventRecord) -> Result<()> {
        self.record_agent_event(record).await
    }

    async fn record_tool_approval(&self, record: ToolApprovalRecord) -> Result<()> {
        self.record_tool_approval(record).await
    }
}

impl TurnSnapshotRepository for StudioStore {
    async fn upsert_session_runtime(
        &self,
        session_id: &str,
        result: &TurnResult,
        model: Option<&pl_model::ModelInfo>,
    ) -> Result<()> {
        self.upsert_session_runtime(session_id, result, model).await
    }

    async fn load_session_runtime(&self, session_id: &str) -> Result<Option<SessionRuntimeRecord>> {
        self.load_session_runtime(session_id).await
    }
}
