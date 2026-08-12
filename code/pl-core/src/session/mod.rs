use std::collections::HashMap;
use std::num::NonZeroUsize;

use pl_model::{CompletionResponse, ModelTransportSession, ToolCall};
use pl_protocol::{
    AgentSessionSnapshot, AgentWorkingState, ConversationRecoveryState, Message, MessageContent,
    MessageRole, ModelContextItem, ModelContextSectionSnapshot, ModelContextSnapshot,
    PinnedContextSection, PromptPrefixChangedReason, ResponsesContextItem, SessionNote,
    ThreadPromptMetadata, ToolCallCaller, ToolCallHistoryMetadata, ToolCallKind,
    ToolResultMetadata, ToolResultReceipt,
};

use crate::working_set::canonical_content_hash;

mod fork;
pub mod tool_history;

#[cfg(test)]
mod tests;

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `TurnEngine` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct AgentSession {
    items: Vec<ModelContextItem>,
    messages: Vec<Message>,
    working_state: AgentWorkingState,
    revision: u64,
    prompt_cache_key: Option<String>,
    transport_session: ModelTransportSession,
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
        let messages = fork::forkable_messages(&self.messages);
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
            items: messages
                .iter()
                .cloned()
                .map(ModelContextItem::from)
                .collect(),
            messages,
            working_state: AgentWorkingState::default(),
            revision: 0,
            prompt_cache_key: None,
            transport_session: ModelTransportSession::default(),
        }
    }

    pub fn from_items(items: Vec<ModelContextItem>) -> Self {
        let messages = tool_history::messages_from_items(&items);
        Self {
            items,
            messages,
            working_state: AgentWorkingState::default(),
            revision: 0,
            prompt_cache_key: None,
            transport_session: ModelTransportSession::default(),
        }
    }

    pub fn from_snapshot(snapshot: AgentSessionSnapshot) -> Self {
        let messages = tool_history::messages_from_items(&snapshot.transcript);
        Self {
            items: snapshot.transcript,
            messages,
            working_state: snapshot.working_state,
            revision: 0,
            prompt_cache_key: None,
            transport_session: ModelTransportSession::default(),
        }
    }

    pub fn snapshot(&self) -> AgentSessionSnapshot {
        AgentSessionSnapshot {
            transcript: self.items.clone(),
            working_state: self.working_state.clone(),
        }
    }

    pub fn items(&self) -> &[ModelContextItem] {
        &self.items
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        self.items = messages
            .iter()
            .cloned()
            .map(ModelContextItem::from)
            .collect();
        self.messages = messages;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn replace_items(&mut self, items: Vec<ModelContextItem>) {
        self.messages = tool_history::messages_from_items(&items);
        self.items = items;
        self.revision = self.revision.saturating_add(1);
    }

    /// 只替换可压缩的时间线，保留当前所有 pinned working context 和会话笔记。
    pub fn replace_compactable_items(&mut self, items: Vec<ModelContextItem>) {
        self.items = items;
        self.messages = tool_history::messages_from_items(&self.items);
        self.revision = self.revision.saturating_add(1);
    }

    /// 原子替换所有 pinned sections；返回 canonical session 是否发生变化。
    pub fn replace_pinned_context_sections(
        &mut self,
        mut sections: Vec<PinnedContextSection>,
    ) -> bool {
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        let current = self.working_state.sections.clone();
        if current == sections {
            return false;
        }
        self.working_state.sections = sections;
        self.working_state.revision = self.working_state.revision.saturating_add(1);
        self.revision = self.revision.saturating_add(1);
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
        self.working_state.sections.iter()
    }

    /// 移除一个可替换 pinned section；不存在时保持幂等。
    pub fn remove_pinned_context(&mut self, section_id: &str) -> bool {
        let sections = self
            .working_state
            .sections
            .iter()
            .filter(|section| section.id.as_str() != section_id)
            .cloned()
            .collect::<Vec<_>>();
        self.replace_pinned_context_sections(sections)
    }

    pub fn conversation_recovery(&self) -> &ConversationRecoveryState {
        &self.working_state.conversation_recovery
    }

    /// 替换 typed conversation recovery 审计状态。
    pub fn replace_conversation_recovery(&mut self, recovery: ConversationRecoveryState) -> bool {
        if self.working_state.conversation_recovery == recovery {
            return false;
        }
        self.working_state.conversation_recovery = recovery;
        self.working_state.revision = self.working_state.revision.saturating_add(1);
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// 将当前 prompt generation 标记为上下文恢复，并废弃 transport/cache continuation。
    pub fn mark_context_recovered(&mut self, updated_at: i64) {
        let mut prompt = self.working_state.prompt.clone();
        for snapshot in prompt.slots.values_mut() {
            snapshot.generation = snapshot.generation.saturating_add(1).max(1);
            snapshot.prefix_changed_reason = PromptPrefixChangedReason::ContextRecovered;
            snapshot.updated_at = updated_at;
        }
        let _ = self.replace_prompt_metadata(prompt);
        self.prompt_cache_key = None;
        self.transport_session = ModelTransportSession::default();
    }

    pub fn session_note(&self) -> Option<&SessionNote> {
        self.working_state.session_note.as_ref()
    }

    pub fn prompt_metadata(&self) -> &ThreadPromptMetadata {
        &self.working_state.prompt
    }

    pub fn replace_prompt_metadata(&mut self, prompt: ThreadPromptMetadata) -> bool {
        if self.working_state.prompt == prompt {
            return false;
        }
        self.working_state.prompt = prompt;
        self.working_state.revision = self.working_state.revision.saturating_add(1);
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub fn working_context_snapshot(&self) -> ModelContextSnapshot {
        let mut sections = self
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
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        ModelContextSnapshot {
            sections,
            session_note_available: self.working_state.session_note.is_some(),
        }
    }

    /// 原子替换隐藏会话笔记；返回 canonical session 是否发生变化。
    pub fn replace_session_note(&mut self, note: SessionNote) -> bool {
        if self.working_state.session_note.as_ref() == Some(&note) {
            return false;
        }
        self.working_state.session_note = Some(note);
        self.working_state.revision = self.working_state.revision.saturating_add(1);
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        self.items.truncate(len);
        self.messages = tool_history::messages_from_items(&self.items);
        self.revision = self.revision.saturating_add(1);
    }

    pub fn push_user_prompt(&mut self, prompt: String) {
        self.push_user_content(MessageContent::Text(prompt));
    }

    pub fn push_user_content(&mut self, content: MessageContent) {
        self.push_message(Message {
            role: MessageRole::User,
            content,
            reasoning_content: None,
            metadata: HashMap::new(),
        });
    }

    pub fn push_assistant_response(&mut self, content: String, reasoning_content: Option<String>) {
        self.push_message(Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(content),
            reasoning_content,
            metadata: HashMap::new(),
        });
    }

    /// 推入 assistant 的 tool_calls 消息。
    ///
    /// tool_calls 序列化后存入 metadata，供 pl-model protocol 层构造正确的 wire 格式。
    pub fn push_assistant_tool_calls(
        &mut self,
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        reasoning_content: Option<String>,
    ) {
        let mut metadata = HashMap::new();
        let json = serde_json::to_string(&tool_calls)
            .expect("ToolCall serialization should be infallible");
        ToolCallHistoryMetadata::new(json).insert_into(&mut metadata);
        self.push_message(Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(content.unwrap_or_default()),
            reasoning_content,
            metadata,
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
        self.items.extend(
            items
                .into_iter()
                .map(|item| ModelContextItem::Responses { item }),
        );
        self.revision = self.revision.saturating_add(1);
    }

    /// 推入 tool result 消息。
    pub fn push_tool_result(
        &mut self,
        tool_call_id: String,
        tool_call_call_id: Option<String>,
        tool_name: String,
        tool_call_kind: ToolCallKind,
        result: String,
        tool_arguments: String,
    ) {
        let receipt = ToolResultReceipt {
            call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            arguments_hash: canonical_content_hash(tool_arguments.as_bytes()),
            result_hash: canonical_content_hash(result.as_bytes()),
            total_bytes: result.len() as u64,
            visible_bytes: result.len() as u64,
            truncated: false,
            artifacts: Vec::new(),
            continuation: None,
            reused_from_call_id: None,
        };
        self.push_tool_result_with_receipt(
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            result,
            tool_arguments,
            receipt,
        );
    }

    /// 推入带 compact receipt 的 canonical tool result。
    #[allow(clippy::too_many_arguments)]
    pub fn push_tool_result_with_receipt(
        &mut self,
        tool_call_id: String,
        tool_call_call_id: Option<String>,
        tool_name: String,
        tool_call_kind: ToolCallKind,
        result: String,
        tool_arguments: String,
        receipt: ToolResultReceipt,
    ) {
        self.push_tool_result_with_receipt_and_caller(
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            result,
            tool_arguments,
            receipt,
            None,
        );
    }

    /// 推入带 Programmatic caller 与 compact receipt 的 canonical tool result。
    #[allow(clippy::too_many_arguments)]
    pub fn push_tool_result_with_receipt_and_caller(
        &mut self,
        tool_call_id: String,
        tool_call_call_id: Option<String>,
        tool_name: String,
        tool_call_kind: ToolCallKind,
        result: String,
        tool_arguments: String,
        receipt: ToolResultReceipt,
        caller: Option<ToolCallCaller>,
    ) {
        let mut metadata = HashMap::new();
        ToolResultMetadata::new(
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            tool_arguments,
        )
        .with_caller(caller)
        .insert_into(&mut metadata);
        let message = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text(result),
            reasoning_content: None,
            metadata,
        };
        self.items.push(ModelContextItem::ToolResult {
            message: message.clone(),
            receipt,
        });
        self.messages.push(message);
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_prompt_cache_key(&mut self, key: String) {
        self.prompt_cache_key = Some(key);
    }

    pub fn replace_prompt_cache_key(&mut self, key: Option<String>) {
        self.prompt_cache_key = key;
    }

    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.prompt_cache_key.as_deref()
    }

    pub fn transport_session(&self) -> ModelTransportSession {
        self.transport_session.clone()
    }

    fn push_message(&mut self, message: Message) {
        self.items.push(ModelContextItem::from(message.clone()));
        self.messages.push(message);
    }
}
