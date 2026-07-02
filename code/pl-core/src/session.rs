use std::collections::HashMap;

use pl_model::{ModelContinuationState, ToolCall};
use pl_protocol::{
    Message, MessageContent, MessageRole, ToolCallHistoryMetadata, ToolCallKind, ToolResultMetadata,
};

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `PureCore` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct CoreSession {
    messages: Vec<Message>,
    revision: u64,
    continuation: ModelContinuationState,
}

impl CoreSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_messages(messages: Vec<Message>) -> Self {
        Self {
            messages,
            revision: 0,
            continuation: ModelContinuationState::default(),
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn replace_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.revision = self.revision.saturating_add(1);
        self.reset_continuation();
    }

    pub fn replace_messages_preserving_continuation(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.revision = self.revision.saturating_add(1);
        self.continuation
            .reset_if_acknowledged_messages_were_removed(self.messages.len());
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        self.messages.truncate(len);
        self.continuation
            .reset_if_acknowledged_messages_were_removed(len);
    }

    pub fn push_user_prompt(&mut self, prompt: String) {
        self.push_user_content(MessageContent::Text(prompt));
    }

    pub fn push_user_content(&mut self, content: MessageContent) {
        self.messages.push(Message {
            role: MessageRole::User,
            content,
            reasoning_content: None,
            metadata: HashMap::new(),
        });
    }

    pub fn push_assistant_response(&mut self, content: String, reasoning_content: Option<String>) {
        self.messages.push(Message {
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
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(content.unwrap_or_default()),
            reasoning_content,
            metadata,
        });
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
        let mut metadata = HashMap::new();
        ToolResultMetadata::new(
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            tool_arguments,
        )
        .insert_into(&mut metadata);
        self.messages.push(Message {
            role: MessageRole::Tool,
            content: MessageContent::Text(result),
            reasoning_content: None,
            metadata,
        });
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn set_prompt_cache_key(&mut self, key: String) {
        self.continuation.set_prompt_cache_key(key);
    }

    pub fn prompt_cache_key(&self) -> Option<&str> {
        self.continuation.prompt_cache_key()
    }

    pub fn previous_response_id(&self) -> Option<&str> {
        self.continuation.previous_response_id()
    }

    pub fn acknowledged_message_count(&self) -> usize {
        self.continuation.acknowledged_message_count()
    }

    pub fn continuation_start_index(&self) -> Option<usize> {
        self.continuation.continuation_start_index()
    }

    pub fn continuation_disabled(&self) -> bool {
        self.continuation.disabled()
    }

    pub fn acknowledge_model_response(
        &mut self,
        acknowledged_message_count: usize,
        response_id: Option<String>,
    ) {
        self.continuation.acknowledge_response(
            acknowledged_message_count,
            response_id,
            self.messages.len(),
        );
    }

    pub fn mark_continuation_unsupported(&mut self) {
        self.continuation.mark_unsupported();
    }

    pub fn reset_continuation(&mut self) {
        self.continuation.reset();
    }
}

#[cfg(test)]
mod tests {
    use pl_model::{ToolCall, ToolCallKind};

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
    fn new_session_is_empty() {
        let session = CoreSession::new();
        assert!(session.is_empty());
        assert_eq!(session.len(), 0);
    }

    #[test]
    fn push_user_prompt_adds_message() {
        let mut session = CoreSession::new();
        session.push_user_prompt("hello".to_string());

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::User);
        match &session.messages()[0].content {
            MessageContent::Text(t) => assert_eq!(t, "hello"),
            _ => panic!("expected Text content"),
        }
    }

    #[test]
    fn push_assistant_response_adds_message() {
        let mut session = CoreSession::new();
        session.push_assistant_response("reply".to_string(), Some("thinking".to_string()));

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Assistant);
        assert_eq!(
            session.messages()[0].reasoning_content,
            Some("thinking".to_string())
        );
    }

    #[test]
    fn push_assistant_tool_calls_stores_metadata() {
        let mut session = CoreSession::new();
        let tool_calls = vec![ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({"command": "ls"}),
            Some("call-1".to_string()),
        )];
        session.push_assistant_tool_calls(Some("running...".to_string()), tool_calls, None);

        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::Assistant);
        assert!(session.messages()[0].metadata.contains_key("tool_calls"));
    }

    #[test]
    fn push_tool_result_stores_metadata() {
        let mut session = CoreSession::new();
        session.push_tool_result(
            "provider-item-1".to_string(),
            Some("call-1".to_string()),
            "bash".to_string(),
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
            "bash"
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
        let session = CoreSession::from_messages(msgs.clone());
        assert_eq!(session.len(), 2);
        assert_eq!(session.messages()[0].role, MessageRole::User);
        assert_eq!(session.messages()[1].role, MessageRole::Assistant);
    }

    #[test]
    fn replace_messages_updates_history_and_revision() {
        let mut session = CoreSession::new();
        session.push_user_prompt("old".to_string());
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
    }

    #[test]
    fn truncate_messages_keeps_prefix_without_revision_change() {
        let mut session = CoreSession::new();
        session.push_user_prompt("first".to_string());
        session.push_assistant_response("second".to_string(), None);
        let original_revision = session.revision();

        session.truncate_messages(1);

        assert_eq!(session.revision(), original_revision);
        assert_eq!(session.len(), 1);
        assert_eq!(session.messages()[0].role, MessageRole::User);
        assert_eq!(
            session.messages()[0].content,
            MessageContent::Text("first".to_string())
        );
    }

    #[test]
    fn continuation_state_tracks_response_and_acknowledged_messages() {
        let mut session = CoreSession::new();
        session.push_user_prompt("hello".to_string());
        session.set_prompt_cache_key("cache-1".to_string());
        session.acknowledge_model_response(session.len(), Some("resp-1".to_string()));

        assert_eq!(session.prompt_cache_key(), Some("cache-1"));
        assert_eq!(session.previous_response_id(), Some("resp-1"));
        assert_eq!(session.acknowledged_message_count(), 1);
        assert_eq!(session.continuation_start_index(), Some(1));

        session.mark_continuation_unsupported();

        assert_eq!(session.previous_response_id(), None);
        assert_eq!(session.acknowledged_message_count(), 0);
        assert_eq!(session.continuation_start_index(), None);
        assert!(session.continuation_disabled());
    }

    #[test]
    fn replace_messages_preserving_continuation_keeps_valid_response_state() {
        let mut session = CoreSession::from_messages(vec![text_message("first")]);
        session.set_prompt_cache_key("cache-1".to_string());
        session.acknowledge_model_response(session.len(), Some("resp-1".to_string()));

        session.replace_messages_preserving_continuation(vec![
            text_message("first"),
            text_message("second"),
        ]);

        assert_eq!(session.prompt_cache_key(), Some("cache-1"));
        assert_eq!(session.previous_response_id(), Some("resp-1"));
        assert_eq!(session.acknowledged_message_count(), 1);
        assert_eq!(session.continuation_start_index(), Some(1));

        session.replace_messages_preserving_continuation(Vec::new());

        assert_eq!(session.prompt_cache_key(), Some("cache-1"));
        assert_eq!(session.previous_response_id(), None);
        assert_eq!(session.acknowledged_message_count(), 0);
        assert_eq!(session.continuation_start_index(), None);
    }
}
