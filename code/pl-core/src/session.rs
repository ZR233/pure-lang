use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;

use pl_model::{CompletionResponse, ModelTransportSession, ToolCall};
use pl_protocol::{
    Message, MessageContent, MessageRole, ModelContextItem, PinnedContextSection, SessionNote,
    TOOL_CALLS_METADATA_KEY, ToolCallHistoryMetadata, ToolCallKind, ToolResultMetadata,
    ToolResultReceipt,
};

use crate::working_set::canonical_content_hash;

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `TurnEngine` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct AgentSession {
    items: Vec<ModelContextItem>,
    messages: Vec<Message>,
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
        let messages = forkable_messages(&self.messages);
        match policy {
            AgentSessionForkPolicy::Empty => Self::new(),
            AgentSessionForkPolicy::AllMessages => Self::from_messages(messages),
            AgentSessionForkPolicy::LastUserTurns(turns) => {
                Self::from_messages(last_user_turns(messages, turns.get()))
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
            revision: 0,
            prompt_cache_key: None,
            transport_session: ModelTransportSession::default(),
        }
    }

    pub fn from_items(items: Vec<ModelContextItem>) -> Self {
        let messages = messages_from_items(&items);
        Self {
            items,
            messages,
            revision: 0,
            prompt_cache_key: None,
            transport_session: ModelTransportSession::default(),
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
        let note = self.session_note().cloned();
        self.items = messages
            .iter()
            .cloned()
            .map(ModelContextItem::from)
            .collect();
        self.items.extend(
            note.into_iter()
                .map(|note| ModelContextItem::SessionNote { note }),
        );
        self.messages = messages;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn replace_items(&mut self, items: Vec<ModelContextItem>) {
        self.messages = messages_from_items(&items);
        self.items = items;
        self.revision = self.revision.saturating_add(1);
    }

    /// 只替换可压缩的时间线，保留当前所有 pinned working context 和会话笔记。
    pub fn replace_compactable_items(&mut self, items: Vec<ModelContextItem>) {
        let retained = self
            .items
            .iter()
            .filter(|item| is_durable_context(item))
            .cloned()
            .collect::<Vec<_>>();
        self.items = items
            .into_iter()
            .filter(|item| !is_durable_context(item))
            .chain(retained)
            .collect();
        self.messages = messages_from_items(&self.items);
        self.revision = self.revision.saturating_add(1);
    }

    /// 原子替换所有 pinned sections；返回 canonical session 是否发生变化。
    pub fn replace_pinned_context_sections(
        &mut self,
        mut sections: Vec<PinnedContextSection>,
    ) -> bool {
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        let current = self.pinned_context_sections().cloned().collect::<Vec<_>>();
        if current == sections {
            return false;
        }
        self.items.retain(|item| !item.is_pinned_context());
        self.items.extend(
            sections
                .into_iter()
                .map(|section| ModelContextItem::PinnedContext { section }),
        );
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
        self.items
            .iter()
            .filter_map(ModelContextItem::as_pinned_context)
    }

    pub fn session_note(&self) -> Option<&SessionNote> {
        self.items
            .iter()
            .find_map(ModelContextItem::as_session_note)
    }

    /// 原子替换隐藏会话笔记；返回 canonical session 是否发生变化。
    pub fn replace_session_note(&mut self, note: SessionNote) -> bool {
        if self.session_note() == Some(&note) {
            return false;
        }
        self.items.retain(|item| !item.is_session_note());
        self.items.push(ModelContextItem::SessionNote { note });
        self.revision = self.revision.saturating_add(1);
        true
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        let mut chronological = self
            .items
            .iter()
            .filter(|item| !is_durable_context(item))
            .take(len)
            .cloned()
            .collect::<Vec<_>>();
        chronological.extend(
            self.items
                .iter()
                .filter(|item| is_durable_context(item))
                .cloned(),
        );
        self.items = chronological;
        self.messages = messages_from_items(&self.items);
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
        let mut metadata = HashMap::new();
        ToolResultMetadata::new(
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            tool_arguments,
        )
        .insert_into(&mut metadata);
        let message = Message {
            role: MessageRole::Tool,
            content: MessageContent::Text(result),
            reasoning_content: None,
            metadata,
        };
        let insertion = self
            .items
            .iter()
            .position(is_durable_context)
            .unwrap_or(self.items.len());
        self.items.insert(
            insertion,
            ModelContextItem::ToolResult {
                message: message.clone(),
                receipt,
            },
        );
        self.messages.push(message);
    }

    pub fn len(&self) -> usize {
        self.items
            .iter()
            .filter(|item| !is_durable_context(item))
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn set_prompt_cache_key(&mut self, key: String) {
        self.prompt_cache_key = Some(key);
    }

    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.prompt_cache_key.as_deref()
    }

    pub fn transport_session(&self) -> ModelTransportSession {
        self.transport_session.clone()
    }

    fn push_message(&mut self, message: Message) {
        let insertion = self
            .items
            .iter()
            .position(is_durable_context)
            .unwrap_or(self.items.len());
        self.items
            .insert(insertion, ModelContextItem::from(message.clone()));
        self.messages.push(message);
    }
}

fn is_durable_context(item: &ModelContextItem) -> bool {
    item.is_pinned_context() || item.is_session_note()
}

fn forkable_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .filter(|message| match message.role {
            MessageRole::System | MessageRole::User => true,
            MessageRole::Assistant => !message.metadata.contains_key(TOOL_CALLS_METADATA_KEY),
            MessageRole::Tool => false,
        })
        .map(|message| Message {
            role: message.role,
            content: message.content.clone(),
            reasoning_content: None,
            metadata: HashMap::new(),
        })
        .collect()
}

fn last_user_turns(messages: Vec<Message>, turns: usize) -> Vec<Message> {
    let (system, conversation): (Vec<_>, Vec<_>) = messages
        .into_iter()
        .partition(|message| message.role == MessageRole::System);
    let start = conversation
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, message)| message.role == MessageRole::User)
        .nth(turns.saturating_sub(1))
        .map_or(0, |(index, _)| index);
    system
        .into_iter()
        .chain(conversation.into_iter().skip(start))
        .collect()
}

fn messages_from_items(items: &[ModelContextItem]) -> Vec<Message> {
    items
        .iter()
        .filter_map(ModelContextItem::as_message)
        .cloned()
        .collect()
}

/// 构造包含 assistant tool_calls metadata 的历史消息。
///
/// 宿主测试或迁移工具需要手工构造历史时，应复用该 helper，而不是直接拼
/// `tool_calls` metadata JSON。生产 turn loop 仍应优先通过 `AgentSession`
/// 记录模型返回的真实 `ToolCall`。
pub fn tool_call_history_message(
    call_id: String,
    tool_name: String,
    raw_arguments: String,
) -> Message {
    let arguments =
        serde_json::from_str(&raw_arguments).unwrap_or(serde_json::Value::String(raw_arguments));
    let tool_calls = serde_json::json!([{
        "id": call_id,
        "name": tool_name,
        "payload": {
            "kind": "function",
            "arguments": arguments
        },
        "call_id": call_id
    }])
    .to_string();
    let mut metadata = HashMap::new();
    ToolCallHistoryMetadata::new(tool_calls).insert_into(&mut metadata);
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(String::new()),
        reasoning_content: None,
        metadata,
    }
}

/// 构造包含 tool result metadata 的历史消息。
///
/// 该函数集中维护模型历史里工具结果的 metadata 形状，避免宿主产品在测试或
/// 历史修复场景复制 pl-core 的协议细节。
pub fn tool_result_history_message(
    call_id: String,
    tool_name: String,
    raw_arguments: String,
    output: String,
) -> Message {
    let mut metadata = HashMap::new();
    ToolResultMetadata::new(
        call_id,
        None,
        tool_name,
        ToolCallKind::Function,
        raw_arguments,
    )
    .insert_into(&mut metadata);
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text(output),
        reasoning_content: None,
        metadata,
    }
}

/// 修复不完整的工具调用历史。
///
/// 宿主恢复中断 turn 时，历史里可能保留 assistant tool call，但缺少对应 tool
/// result。模型协议要求每个 tool call 都有结果；该函数会在下一条非 tool 消息前
/// 插入 synthetic interrupted tool result，并返回历史是否发生变化。
pub fn repair_incomplete_tool_history(history: &mut Vec<Message>) -> bool {
    let mut insertions: Vec<(usize, Vec<Message>)> = Vec::new();
    let mut i = 0;
    while i < history.len() {
        let mut pending_calls = Vec::new();
        while i < history.len() {
            if history[i].metadata.contains_key(TOOL_CALLS_METADATA_KEY) {
                pending_calls.extend(tool_calls(&history[i]));
                i += 1;
            } else {
                break;
            }
        }
        if pending_calls.is_empty() {
            i += 1;
            continue;
        }

        let mut answered = HashSet::new();
        while i < history.len() {
            if history[i].role == MessageRole::Tool
                && let Ok(metadata) = ToolResultMetadata::from_metadata(&history[i].metadata)
                && pending_calls
                    .iter()
                    .any(|call| call.id == metadata.tool_call_id)
            {
                answered.insert(metadata.tool_call_id);
                i += 1;
                continue;
            }
            break;
        }

        let missing_outputs = pending_calls
            .into_iter()
            .filter(|call| !answered.contains(&call.id))
            .map(interrupted_tool_result_message)
            .collect::<Vec<_>>();
        if !missing_outputs.is_empty() {
            insertions.push((i, missing_outputs));
        }
    }

    let changed = !insertions.is_empty();
    for (pos, items) in insertions.into_iter().rev() {
        for item in items.into_iter().rev() {
            history.insert(pos, item);
        }
    }
    changed
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingToolCall {
    id: String,
    call_id: Option<String>,
    name: String,
    kind: ToolCallKind,
    arguments: String,
}

fn tool_calls(message: &Message) -> Vec<PendingToolCall> {
    ToolCallHistoryMetadata::from_metadata(&message.metadata)
        .and_then(|metadata| {
            serde_json::from_str::<serde_json::Value>(&metadata.tool_calls_json).ok()
        })
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let id = item
                .get("id")
                .or_else(|| item.get("call_id"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)?;
            let call_id = item
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let name = item
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let payload = item.get("payload");
            let kind = payload
                .and_then(|payload| payload.get("kind"))
                .and_then(serde_json::Value::as_str)
                .map(tool_call_kind_from_str)
                .unwrap_or(ToolCallKind::Function);
            let arguments = payload
                .and_then(|payload| payload.get("arguments"))
                .or_else(|| item.get("arguments"))
                .map(tool_call_arguments)
                .unwrap_or_else(|| "{}".to_string());
            Some(PendingToolCall {
                id,
                call_id,
                name,
                kind,
                arguments,
            })
        })
        .collect()
}

fn interrupted_tool_result_message(call: PendingToolCall) -> Message {
    let mut metadata = HashMap::new();
    ToolResultMetadata::new(call.id, call.call_id, call.name, call.kind, call.arguments)
        .insert_into(&mut metadata);
    Message {
        role: MessageRole::Tool,
        content: MessageContent::Text("error: tool execution interrupted".to_string()),
        reasoning_content: None,
        metadata,
    }
}

fn tool_call_kind_from_str(value: &str) -> ToolCallKind {
    match value {
        "custom" => ToolCallKind::Custom,
        "function" => ToolCallKind::Function,
        _ => ToolCallKind::Function,
    }
}

fn tool_call_arguments(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use pl_model::{CompletionResponse, FinishReason, TokenUsage, ToolCall, ToolCallKind};

    use super::*;
    use pretty_assertions::assert_eq;

    fn text_message(text: &str) -> Message {
        Message {
            role: MessageRole::User,
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn push_assistant_tool_calls_stores_metadata() {
        let mut session = AgentSession::new();
        let tool_calls = vec![ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({"command": "ls"}),
            Some("call-1".to_string()),
        )];
        session.push_assistant_tool_calls(Some("running...".to_string()), tool_calls, None);

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Assistant);
        assert!(session.messages()[0].metadata.contains_key("tool_calls"));
    }

    #[test]
    fn push_assistant_completion_response_adds_text_message() {
        let mut session = AgentSession::new();
        let response = CompletionResponse {
            content: Some("reply".to_string()),
            raw_content: Some("reply".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: Vec::new(),
            hosted_web_search_calls: Vec::new(),
            trace_events: Vec::new(),
            next_sequence: 0,
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
            model: "test-model".to_string(),
            response_id: Some("resp-1".to_string()),
        };

        session.push_assistant_completion_response(&response);

        assert_eq!(
            session.messages(),
            &[Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text("reply".to_string()),
                reasoning_content: Some("thinking".to_string()),
                metadata: HashMap::new(),
            }]
        );
    }

    #[test]
    fn push_assistant_completion_response_preserves_tool_call_history() {
        let mut session = AgentSession::new();
        let tool_calls = vec![ToolCall::function(
            "call-1",
            "exec",
            serde_json::json!({"command": "pwd"}),
            Some("call-1".to_string()),
        )];
        let response = CompletionResponse {
            content: Some("running".to_string()),
            raw_content: Some("running".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: tool_calls.clone(),
            hosted_web_search_calls: Vec::new(),
            trace_events: Vec::new(),
            next_sequence: 0,
            usage: TokenUsage::default(),
            finish_reason: FinishReason::ToolCalls,
            model: "test-model".to_string(),
            response_id: Some("resp-1".to_string()),
        };

        session.push_assistant_completion_response(&response);

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Assistant);
        assert_eq!(
            session.messages()[0].content,
            MessageContent::Text("running".to_string())
        );
        assert_eq!(
            session.messages()[0].reasoning_content,
            Some("thinking".to_string())
        );
        let metadata = ToolCallHistoryMetadata::from_metadata(&session.messages()[0].metadata)
            .expect("tool calls");
        assert_eq!(
            metadata.tool_calls_json,
            serde_json::to_string(&tool_calls).expect("tool call json")
        );
    }

    #[test]
    fn push_tool_result_stores_metadata() {
        let mut session = AgentSession::new();
        session.push_tool_result(
            "provider-item-1".to_string(),
            Some("call-1".to_string()),
            "exec".to_string(),
            ToolCallKind::Function,
            "output".to_string(),
            r#"{"command":"echo hi"}"#.to_string(),
        );

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Tool);
        assert_eq!(
            session.messages()[0].metadata.get("tool_call_id").unwrap(),
            "provider-item-1"
        );
        assert_eq!(
            session.messages()[0]
                .metadata
                .get("tool_call_call_id")
                .unwrap(),
            "call-1"
        );
        assert_eq!(
            session.messages()[0].metadata.get("tool_name").unwrap(),
            "exec"
        );
        assert_eq!(
            session.messages()[0]
                .metadata
                .get("tool_call_kind")
                .unwrap(),
            "function"
        );
        assert_eq!(
            session.messages()[0]
                .metadata
                .get("tool_call_arguments")
                .unwrap(),
            r#"{"command":"echo hi"}"#
        );
    }

    #[test]
    fn tool_call_history_message_parses_arguments_into_metadata() {
        let message = tool_call_history_message(
            "call-1".to_string(),
            "read_file".to_string(),
            r#"{"path":"README.md"}"#.to_string(),
        );

        assert_eq!(message.role, MessageRole::Assistant);
        let metadata =
            ToolCallHistoryMetadata::from_metadata(&message.metadata).expect("tool calls");
        let value: serde_json::Value =
            serde_json::from_str(&metadata.tool_calls_json).expect("tool call json");
        assert_eq!(value[0]["id"], "call-1");
        assert_eq!(value[0]["name"], "read_file");
        assert_eq!(value[0]["payload"]["arguments"]["path"], "README.md");
    }

    #[test]
    fn tool_result_history_message_stores_result_metadata() {
        let message = tool_result_history_message(
            "call-1".to_string(),
            "read_file".to_string(),
            r#"{"path":"README.md"}"#.to_string(),
            "ok".to_string(),
        );

        assert_eq!(message.role, MessageRole::Tool);
        assert_eq!(message.content, MessageContent::Text("ok".to_string()));
        let metadata =
            ToolResultMetadata::from_metadata(&message.metadata).expect("tool result metadata");
        assert_eq!(metadata.tool_call_id, "call-1");
        assert_eq!(metadata.tool_name, "read_file");
        assert_eq!(
            metadata.tool_call_arguments.as_deref(),
            Some(r#"{"path":"README.md"}"#)
        );
    }

    #[test]
    fn from_messages_preserves_order() {
        let msgs = vec![
            Message {
                role: MessageRole::User,
                content: MessageContent::Text("q".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            },
            Message {
                role: MessageRole::Assistant,
                content: MessageContent::Text("a".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            },
        ];
        let session = AgentSession::from_messages(msgs.clone());
        assert_eq!(session.len(), 2);
        assert_eq!(session.messages()[0].role, MessageRole::User);
        assert_eq!(session.messages()[1].role, MessageRole::Assistant);
    }

    #[test]
    fn child_fork_excludes_open_and_completed_tool_protocol_messages() {
        let mut parent = AgentSession::new();
        parent.push_user_prompt("implement".to_string());
        parent.push_assistant_response("working".to_string(), None);
        parent.push_assistant_tool_calls(
            None,
            vec![ToolCall::function(
                "call-1",
                "task_request_review",
                serde_json::json!({}),
                Some("call-1".to_string()),
            )],
            None,
        );
        parent.push_tool_result(
            "call-1".to_string(),
            Some("call-1".to_string()),
            "task_request_review".to_string(),
            ToolCallKind::Function,
            "ok".to_string(),
            "{}".to_string(),
        );

        let child = parent.fork(AgentSessionForkPolicy::AllMessages);

        assert_eq!(
            child
                .messages()
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            vec![MessageRole::User, MessageRole::Assistant]
        );
        assert!(
            child
                .messages()
                .iter()
                .all(|message| message.metadata.is_empty())
        );
    }

    #[test]
    fn from_items_preserves_checkpoint_order_but_message_view_filters_it() {
        let user = text_message("retained user");
        let items = vec![
            ModelContextItem::from(user.clone()),
            ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string(),
            },
        ];

        let session = AgentSession::from_items(items.clone());

        assert_eq!(session.items(), items.as_slice());
        assert_eq!(session.messages(), &[user]);
        assert_eq!(session.len(), 2);
    }

    #[test]
    fn replace_items_increments_revision_and_preserves_prompt_cache_key() {
        let mut session = AgentSession::from_messages(vec![text_message("old")]);
        session.set_prompt_cache_key("cache-1".to_string());
        let original_revision = session.revision();

        session.replace_items(vec![ModelContextItem::Compaction {
            encrypted_content: "encrypted".to_string(),
        }]);

        assert_eq!(session.revision(), original_revision + 1);
        assert_eq!(session.prompt_cache_key(), Some("cache-1"));
        assert!(session.messages().is_empty());
    }

    #[test]
    fn replace_messages_updates_history_and_revision() {
        let mut session = AgentSession::new();
        session.push_user_prompt("old".to_string());
        let note = session_note(3, "durable");
        session.replace_session_note(note.clone());
        let original_revision = session.revision();
        let messages = vec![Message {
            role: MessageRole::User,
            content: MessageContent::Text("summary".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }];

        session.replace_messages(messages.clone());

        assert_eq!(session.revision(), original_revision + 1);
        assert_eq!(session.messages(), messages.as_slice());
        assert_eq!(session.session_note(), Some(&note));
    }

    #[test]
    fn truncate_messages_keeps_prefix_and_invalidates_history_revision() {
        let mut session = AgentSession::new();
        session.push_user_prompt("first".to_string());
        session.push_assistant_response("second".to_string(), None);
        let note = session_note(4, "survives truncation");
        session.replace_session_note(note.clone());
        let original_revision = session.revision();

        session.truncate_messages(1);

        assert_eq!(session.revision(), original_revision + 1);
        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::User);
        assert_eq!(
            session.messages()[0].content,
            MessageContent::Text("first".to_string())
        );
        assert_eq!(session.session_note(), Some(&note));
    }

    #[test]
    fn compaction_preserves_note_and_child_forks_do_not_inherit_it() {
        let mut parent = AgentSession::from_messages(vec![text_message("before")]);
        let note = session_note(7, "important checkpoint");
        parent.replace_session_note(note.clone());

        parent.replace_compactable_items(vec![ModelContextItem::from(text_message("summary"))]);
        let child = parent.fork(AgentSessionForkPolicy::AllMessages);

        assert_eq!(parent.session_note(), Some(&note));
        assert_eq!(parent.messages(), &[text_message("summary")]);
        assert_eq!(child.messages(), &[text_message("summary")]);
        assert_eq!(child.session_note(), None);
    }

    fn session_note(revision: u64, content: &str) -> SessionNote {
        SessionNote {
            revision,
            content: content.to_string(),
            content_hash: canonical_content_hash(content.as_bytes()),
            updated_at: 1,
        }
    }

    #[test]
    fn repair_incomplete_tool_history_inserts_missing_result_before_next_user_message() {
        let mut session = AgentSession::new();
        session.push_assistant_tool_calls(
            None,
            vec![ToolCall::function(
                "call-1",
                "exec",
                serde_json::json!({"command": "pwd"}),
                Some("call-1".to_string()),
            )],
            None,
        );
        let mut history = session.messages().to_vec();
        history.push(text_message("continue"));

        assert!(repair_incomplete_tool_history(&mut history));

        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, MessageRole::Tool);
        let metadata =
            ToolResultMetadata::from_metadata(&history[1].metadata).expect("tool metadata");
        assert_eq!(metadata.tool_call_id, "call-1");
        assert_eq!(metadata.tool_call_call_id.as_deref(), Some("call-1"));
        assert_eq!(metadata.tool_name, "exec");
        assert_eq!(
            metadata.tool_call_arguments.as_deref(),
            Some(r#"{"command":"pwd"}"#)
        );
        assert_eq!(history[2].role, MessageRole::User);
    }
}
