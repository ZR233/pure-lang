use std::collections::HashMap;

use pl_protocol::{Message, MessageContent, MessageRole};

/// 核心编译会话。
///
/// 保存多轮 turn 之间的消息历史，供 `PureCore` 构造模型请求。
#[derive(Debug, Clone, Default)]
pub struct CoreSession {
    messages: Vec<Message>,
}

impl CoreSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_messages(messages: Vec<Message>) -> Self {
        Self { messages }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
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
}
