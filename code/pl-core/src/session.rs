use std::collections::HashMap;

use pl_model::{ToolCall, ToolCallKind};
use pl_protocol::{Message, MessageContent, MessageRole};

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `PureCore` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct CoreSession {
    messages: Vec<Message>,
    revision: u64,
}

impl CoreSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_messages(messages: Vec<Message>) -> Self {
        Self {
            messages,
            revision: 0,
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
    }

    pub(crate) fn truncate_messages(&mut self, len: usize) {
        self.messages.truncate(len);
    }

    pub fn push_user_prompt(&mut self, prompt: String) {
        self.messages.push(Message {
            role: MessageRole::User,
            content: MessageContent::Text(prompt),
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
        metadata.insert("tool_calls".to_string(), json);
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
        if let Some(call_id) = tool_call_call_id {
            metadata.insert("tool_call_call_id".to_string(), call_id);
        }
        metadata.insert("tool_call_id".to_string(), tool_call_id);
        metadata.insert("tool_name".to_string(), tool_name);
        metadata.insert(
            "tool_call_kind".to_string(),
            tool_call_kind.as_str().to_string(),
        );
        metadata.insert("tool_call_arguments".to_string(), tool_arguments);
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
}

#[cfg(test)]
mod tests {
    use pl_model::{ToolCall, ToolCallKind};

    use super::*;
    use pretty_assertions::assert_eq;

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
}
