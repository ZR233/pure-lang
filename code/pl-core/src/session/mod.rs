use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use pl_model::{CompletionResponse, ModelSession, ToolCall};
use pl_protocol::{
    AgentSessionSnapshot, AgentWorkingState, ConversationRecoveryState, Message, MessageContent,
    MessageRole, ModelContextItem, ModelContextSectionSnapshot, ModelContextSnapshot,
    PinnedContextSection, PromptPrefixChangedReason, ResponsesContextItem, SessionNote,
    ThreadPromptMetadata, ToolDiscoveryState, ToolMediaContext, ToolResultReceipt,
    ToolResultRecord,
};

use crate::working_set::canonical_content_hash;

mod fork;
pub mod tool_history;

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `TurnEngine` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct AgentSession {
    state: Arc<AgentSessionState>,
}

#[derive(Debug, Clone, Default)]
struct AgentSessionState {
    items: Vec<ModelContextItem>,
    messages: Vec<Message>,
    working_state: AgentWorkingState,
    revision: u64,
    prompt_cache_key: Option<String>,
    model_session: ModelSession,
}

/// child agent 从 parent canonical session 继承历史的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionForkPolicy {
    /// 不继承对话历史。
    Empty,
    /// 继承全部已闭合的用户/助手消息。
    AllMessages,
    /// 只继承最后若干个用户轮次。
    LastUserTurns(NonZeroUsize),
}

impl AgentSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// 为 child agent 创建 provider 协议完整的 session 副本。
    ///
    /// 工具调用与 tool result 不直接跨 agent 继承，避免在当前工具尚未返回时
    /// 复制出孤立 assistant tool call。
    pub fn fork(&self, policy: AgentSessionForkPolicy) -> Self {
        let messages = fork::forkable_messages(&self.state.messages);
        match policy {
            AgentSessionForkPolicy::Empty => Self::new(),
            AgentSessionForkPolicy::AllMessages => Self::from_messages(messages),
            AgentSessionForkPolicy::LastUserTurns(turns) => {
                Self::from_messages(fork::last_user_turns(messages, turns.get()))
            }
        }
    }

    pub fn from_messages(messages: Vec<Message>) -> Self {
        Self {
            state: Arc::new(AgentSessionState {
                items: messages
                    .iter()
                    .cloned()
                    .map(ModelContextItem::from)
                    .collect(),
                messages,
                ..AgentSessionState::default()
            }),
        }
    }

    pub fn from_items(items: Vec<ModelContextItem>) -> Self {
        let messages = tool_history::messages_from_items(&items);
        Self {
            state: Arc::new(AgentSessionState {
                items,
                messages,
                ..AgentSessionState::default()
            }),
        }
    }

    pub fn from_snapshot(snapshot: AgentSessionSnapshot) -> Self {
        let messages = tool_history::messages_from_items(&snapshot.transcript);
        Self {
            state: Arc::new(AgentSessionState {
                items: snapshot.transcript,
                messages,
                working_state: snapshot.working_state,
                ..AgentSessionState::default()
            }),
        }
    }

    pub fn snapshot(&self) -> AgentSessionSnapshot {
        AgentSessionSnapshot {
            transcript: self.state.items.clone(),
            working_state: self.state.working_state.clone(),
        }
    }

    pub fn items(&self) -> &[ModelContextItem] {
        &self.state.items
    }

    pub fn messages(&self) -> &[Message] {
        &self.state.messages
    }

    pub fn revision(&self) -> u64 {
        self.state.revision
    }

    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        let state = Arc::make_mut(&mut self.state);
        state.items = messages
            .iter()
            .cloned()
            .map(ModelContextItem::from)
            .collect();
        state.messages = messages;
        state.revision = state.revision.saturating_add(1);
    }

    pub fn replace_items(&mut self, items: Vec<ModelContextItem>) {
        let state = Arc::make_mut(&mut self.state);
        state.messages = tool_history::messages_from_items(&items);
        state.items = items;
        state.revision = state.revision.saturating_add(1);
    }

    /// 只替换可压缩的时间线，保留当前所有 pinned working context 和会话笔记。
    pub fn replace_compactable_items(&mut self, items: Vec<ModelContextItem>) {
        let state = Arc::make_mut(&mut self.state);
        state.items = items;
        state.messages = tool_history::messages_from_items(&state.items);
        state.revision = state.revision.saturating_add(1);
    }

    /// 原子替换所有 pinned sections；返回 canonical session 是否发生变化。
    pub fn replace_pinned_context_sections(
        &mut self,
        mut sections: Vec<PinnedContextSection>,
    ) -> bool {
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        let current = self.state.working_state.sections.clone();
        if current == sections {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.sections = sections;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    pub fn upsert_pinned_context(&mut self, section: PinnedContextSection) {
        let mut sections = self
            .pinned_context_sections()
            .filter(|existing| existing.id != section.id)
            .cloned()
            .collect::<Vec<_>>();
        sections.push(section);
        self.replace_pinned_context_sections(sections);
    }

    pub fn pinned_context_sections(&self) -> impl Iterator<Item = &PinnedContextSection> {
        self.state.working_state.sections.iter()
    }

    /// 移除一个可替换 pinned section；不存在时保持幂等。
    pub fn remove_pinned_context(&mut self, section_id: &str) -> bool {
        let sections = self
            .state
            .working_state
            .sections
            .iter()
            .filter(|section| section.id.as_str() != section_id)
            .cloned()
            .collect::<Vec<_>>();
        self.replace_pinned_context_sections(sections)
    }

    pub fn conversation_recovery(&self) -> &ConversationRecoveryState {
        &self.state.working_state.conversation_recovery
    }

    pub fn tool_discovery(&self) -> &ToolDiscoveryState {
        &self.state.working_state.tool_discovery
    }

    /// Replaces the current session's deferred-tool reveal state.
    pub fn replace_tool_discovery(&mut self, mut discovery: ToolDiscoveryState) -> bool {
        discovery.revealed_tool_names.sort();
        discovery.revealed_tool_names.dedup();
        if self.state.working_state.tool_discovery == discovery {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.tool_discovery = discovery;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    /// 替换 typed conversation recovery 审计状态。
    pub fn replace_conversation_recovery(&mut self, recovery: ConversationRecoveryState) -> bool {
        if self.state.working_state.conversation_recovery == recovery {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.conversation_recovery = recovery;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    /// 将当前 prompt generation 标记为上下文恢复，并废弃 transport/cache continuation。
    pub fn mark_context_recovered(&mut self, updated_at: i64) {
        let mut prompt = self.state.working_state.prompt.clone();
        for snapshot in prompt.slots.values_mut() {
            snapshot.generation = snapshot.generation.saturating_add(1).max(1);
            snapshot.prefix_changed_reason = PromptPrefixChangedReason::ContextRecovered;
            snapshot.updated_at = updated_at;
        }
        let _ = self.replace_prompt_metadata(prompt);
        let state = Arc::make_mut(&mut self.state);
        state.prompt_cache_key = None;
        state.model_session = ModelSession::default();
    }

    pub fn session_note(&self) -> Option<&SessionNote> {
        self.state.working_state.session_note.as_ref()
    }

    /// 返回当前 canonical 工作流状态。
    pub fn workflow(&self) -> Option<&pl_protocol::WorkflowSessionState> {
        self.state.working_state.workflow.as_ref()
    }

    /// 原子替换完整工作流状态；返回 canonical session 是否发生变化。
    pub fn replace_workflow(
        &mut self,
        workflow: Option<pl_protocol::WorkflowSessionState>,
    ) -> bool {
        if self.state.working_state.workflow == workflow {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.workflow = workflow;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    /// 返回子 Agent 创建时冻结的 Profile 快照。
    pub fn agent_profile(&self) -> Option<&pl_protocol::AgentProfileSnapshot> {
        self.state.working_state.agent_profile.as_ref()
    }

    /// 原子替换冻结 Profile；根会话通常保持 `None`。
    pub fn replace_agent_profile(
        &mut self,
        profile: Option<pl_protocol::AgentProfileSnapshot>,
    ) -> bool {
        if self.state.working_state.agent_profile == profile {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.agent_profile = profile;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    /// 返回子 Agent 创建时冻结的有效工作区。
    pub fn workspace_assignment(&self) -> Option<&pl_protocol::AgentWorkspaceAssignmentSnapshot> {
        self.state.working_state.workspace_assignment.as_ref()
    }

    /// 原子替换冻结工作区；根会话通常保持 `None`。
    pub fn replace_workspace_assignment(
        &mut self,
        assignment: Option<pl_protocol::AgentWorkspaceAssignmentSnapshot>,
    ) -> bool {
        if self.state.working_state.workspace_assignment == assignment {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.workspace_assignment = assignment;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    pub fn prompt_metadata(&self) -> &ThreadPromptMetadata {
        &self.state.working_state.prompt
    }

    pub fn replace_prompt_metadata(&mut self, prompt: ThreadPromptMetadata) -> bool {
        if self.state.working_state.prompt == prompt {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.prompt = prompt;
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    pub fn working_context_snapshot(&self) -> ModelContextSnapshot {
        let mut sections = self
            .state
            .working_state
            .sections
            .iter()
            .filter(|section| section.id.as_str() != crate::EVIDENCE_LEDGER_SECTION_ID)
            .map(|section| ModelContextSectionSnapshot {
                id: section.id.clone(),
                title: section.title.clone(),
                content: section.content.clone(),
                content_hash: section.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        if let Some(workflow) = &self.state.working_state.workflow
            && let Some(section) = crate::workflow::model_context_section(workflow)
        {
            sections.push(section);
        }
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        ModelContextSnapshot {
            sections,
            session_note_available: self.state.working_state.session_note.is_some(),
        }
    }

    /// 原子替换隐藏会话笔记；返回 canonical session 是否发生变化。
    pub fn replace_session_note(&mut self, note: SessionNote) -> bool {
        if self.state.working_state.session_note.as_ref() == Some(&note) {
            return false;
        }
        let state = Arc::make_mut(&mut self.state);
        state.working_state.session_note = Some(note);
        state.working_state.revision = state.working_state.revision.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        true
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        let state = Arc::make_mut(&mut self.state);
        state.items.truncate(len);
        state.messages = tool_history::messages_from_items(&state.items);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn push_user_prompt(&mut self, prompt: String) {
        self.push_user_content(MessageContent::text(prompt));
    }

    pub fn push_user_content(&mut self, content: MessageContent) {
        self.push_message(Message {
            role: MessageRole::User,
            content,
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        });
    }

    pub fn push_assistant_response(&mut self, content: String, reasoning_content: Option<String>) {
        self.push_message(Message {
            role: MessageRole::Assistant,
            content: MessageContent::text(content),
            reasoning_content,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        });
    }

    /// 推入 assistant 的 tool_calls 消息。
    ///
    /// typed 工具调用记录直接保存在消息上，供 pl-model protocol 层构造正确的
    /// wire 格式。
    pub fn push_assistant_tool_calls(
        &mut self,
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: Option<String>,
    ) {
        let records = tool_calls
            .iter()
            .map(tool_history::tool_call_record)
            .collect::<Vec<_>>();
        self.push_message(Message {
            role: MessageRole::Assistant,
            content: MessageContent::text(content.unwrap_or_default()),
            reasoning_content,
            tool_calls: Some(records),
            tool_result: None,
            metadata: HashMap::new(),
        });
    }

    /// 将模型完成响应追加为 assistant 历史消息。
    ///
    /// 该方法集中维护 completion response 到会话历史的映射：普通响应写入
    /// assistant 文本消息，带 tool call 的响应写入带 metadata 的 assistant
    /// tool_calls 消息。协议级 continuation 状态由 transport session 独占维护。
    pub fn push_assistant_completion_response(&mut self, response: &CompletionResponse) {
        self.push_responses_context_items(response.responses_context_items.clone());
        if response.tool_calls.is_empty() {
            self.push_assistant_response(
                response.content.clone().unwrap_or_default(),
                response.reasoning_content.clone(),
            );
        } else {
            self.push_assistant_tool_calls(
                response
                    .content
                    .clone()
                    .filter(|content| !content.is_empty()),
                response.tool_calls.clone(),
                response.reasoning_content.clone(),
            );
        }
    }

    pub fn push_responses_context_items(&mut self, items: Vec<ResponsesContextItem>) {
        if items.is_empty() {
            return;
        }
        let state = Arc::make_mut(&mut self.state);
        state.items.extend(
            items
                .into_iter()
                .map(|item| ModelContextItem::Responses { item }),
        );
        state.revision = state.revision.saturating_add(1);
    }

    /// 推入 tool result 消息。
    pub fn push_tool_result(
        &mut self,
        record: ToolResultRecord,
        result: String,
        tool_arguments: String,
    ) {
        let receipt = ToolResultReceipt {
            call_id: record.item_id.clone(),
            tool_name: record.name.clone(),
            arguments_hash: canonical_content_hash(tool_arguments.as_bytes()),
            result_hash: canonical_content_hash(result.as_bytes()),
            total_bytes: result.len() as u64,
            visible_bytes: result.len() as u64,
            truncated: false,
            artifacts: Vec::new(),
            continuation: None,
            reused_from_call_id: None,
        };
        self.push_tool_result_with_receipt(record, result, receipt);
    }

    /// 推入带 compact receipt 的 canonical tool result。
    pub fn push_tool_result_with_receipt(
        &mut self,
        record: ToolResultRecord,
        result: String,
        receipt: ToolResultReceipt,
    ) {
        let message = tool_history::tool_result_message(record, &result);
        let state = Arc::make_mut(&mut self.state);
        state.items.push(ModelContextItem::ToolResult {
            message: message.clone(),
            receipt,
        });
        state.messages.push(message);
    }

    /// 在完整 tool-result 批次之后追加模型可见、但不伪装成用户消息的媒体上下文。
    pub fn push_tool_media(&mut self, items: Vec<ToolMediaContext>) {
        if items.is_empty() {
            return;
        }
        Arc::make_mut(&mut self.state)
            .items
            .push(ModelContextItem::ToolMedia { items });
    }

    pub fn len(&self) -> usize {
        self.state.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_prompt_cache_key(&mut self, key: String) {
        Arc::make_mut(&mut self.state).prompt_cache_key = Some(key);
    }

    pub fn replace_prompt_cache_key(&mut self, key: Option<String>) {
        Arc::make_mut(&mut self.state).prompt_cache_key = key;
    }

    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.state.prompt_cache_key.as_deref()
    }

    pub fn model_session(&self) -> ModelSession {
        self.state.model_session.clone()
    }

    fn push_message(&mut self, message: Message) {
        let state = Arc::make_mut(&mut self.state);
        state.items.push(ModelContextItem::from(message.clone()));
        state.messages.push(message);
    }
}

#[cfg(test)]
mod unit_tests;
