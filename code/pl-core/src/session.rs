use std::collections::{HashMap, HashSet};

use pl_model::{CompletionResponse, ModelContinuationState, ToolCall};
use pl_protocol::{
    Message, MessageContent, MessageRole, TOOL_CALLS_METADATA_KEY, ToolCallHistoryMetadata,
    ToolCallKind, ToolResultMetadata,
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

    /// 将模型完成响应追加为 assistant 历史消息。
    ///
    /// 该方法集中维护 completion response 到会话历史的映射：普通响应写入
    /// assistant 文本消息，带 tool call 的响应写入带 metadata 的 assistant
    /// tool_calls 消息，并在写入后用 response id 更新 continuation ack。
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
        self.acknowledge_model_response(self.messages.len(), response.response_id.clone());
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

/// 构造包含 assistant tool_calls metadata 的历史消息。
///
/// 宿主测试或迁移工具需要手工构造历史时，应复用该 helper，而不是直接拼
/// `tool_calls` metadata JSON。生产 turn loop 仍应优先通过 `CoreSession`
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
    fn push_assistant_completion_response_adds_text_message() {
        let mut session = CoreSession::new();
        let response = CompletionResponse {
            content: Some("reply".to_string()),
            raw_content: Some("reply".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: Vec::new(),
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
        assert_eq!(session.previous_response_id(), Some("resp-1"));
        assert_eq!(session.acknowledged_message_count(), 1);
        assert_eq!(session.continuation_start_index(), Some(1));
    }

    #[test]
    fn push_assistant_completion_response_preserves_tool_call_history() {
        let mut session = CoreSession::new();
        let tool_calls = vec![ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({"command": "pwd"}),
            Some("call-1".to_string()),
        )];
        let response = CompletionResponse {
            content: Some("running".to_string()),
            raw_content: Some("running".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: tool_calls.clone(),
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
        assert_eq!(session.previous_response_id(), Some("resp-1"));
        assert_eq!(session.acknowledged_message_count(), 1);
        assert_eq!(session.continuation_start_index(), Some(1));
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

    #[test]
    fn repair_incomplete_tool_history_inserts_missing_result_before_next_user_message() {
        let mut session = CoreSession::new();
        session.push_assistant_tool_calls(
            None,
            vec![ToolCall::function(
                "call-1",
                "bash",
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
        assert_eq!(metadata.tool_name, "bash");
        assert_eq!(
            metadata.tool_call_arguments.as_deref(),
            Some(r#"{"command":"pwd"}"#)
        );
        assert_eq!(history[2].role, MessageRole::User);
    }
}
