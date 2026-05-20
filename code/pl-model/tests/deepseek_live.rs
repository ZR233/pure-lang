use std::collections::HashMap;

use pl_core::{AgentEvent, Message, MessageContent, MessageRole};
use pl_model::{
    CompletionRequest, ModelProvider, ProviderInfo, ReasoningConfig, ReasoningSummary,
    create_provider,
};

fn user_message(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: MessageContent::Text(content.to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    }
}

fn assistant_message(content: String, reasoning_content: String) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(content),
        reasoning_content: Some(reasoning_content),
        metadata: HashMap::new(),
    }
}

fn skip_reason_without_api_key(info: &ProviderInfo) -> Option<String> {
    let env_key = info.env_key.as_deref()?;
    match std::env::var(env_key) {
        Ok(value) if !value.trim().is_empty() => None,
        _ => Some(format!(
            "{env_key} is not set; skipping live DeepSeek API test"
        )),
    }
}

fn deepseek_request(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: ProviderInfo::deepseek(None).default_model,
        instructions: Some("请用简短中文回答。".to_string()),
        messages,
        tools: Vec::new(),
        tool_choice: "auto".to_string(),
        parallel_tool_calls: false,
        temperature: None,
        max_tokens: Some(512),
        reasoning: Some(ReasoningConfig {
            effort: Some("high".to_string()),
            summary: Some(ReasoningSummary::Enabled),
        }),
        stream: true,
    }
}

async fn run_turn(messages: Vec<Message>) -> (String, String, usize, usize) {
    let provider = create_provider(ProviderInfo::deepseek(None)).unwrap();
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(256);

    let event_counter = tokio::spawn(async move {
        let mut text_delta_count = 0;
        let mut thinking_delta_count = 0;

        loop {
            match event_rx.recv().await {
                Ok(AgentEvent::TextDelta { .. }) => text_delta_count += 1,
                Ok(AgentEvent::ThinkingDelta { .. }) => thinking_delta_count += 1,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            }
        }

        (text_delta_count, thinking_delta_count)
    });

    let response = provider
        .stream_complete(deepseek_request(messages), event_tx)
        .await
        .unwrap();
    let (text_delta_count, thinking_delta_count) = event_counter.await.unwrap();
    let content = response.content.unwrap_or_default();
    let reasoning_content = response.reasoning_content.unwrap_or_default();

    (
        content,
        reasoning_content,
        text_delta_count,
        thinking_delta_count,
    )
}

#[tokio::test]
async fn deepseek_streams_multi_turn_with_thinking_mode() {
    let info = ProviderInfo::deepseek(None);
    if let Some(reason) = skip_reason_without_api_key(&info) {
        eprintln!("{reason}");
        return;
    }

    let first_messages = vec![user_message(
        "比较 9.11 和 9.8 哪个更大，只给结论和一句理由。",
    )];
    let (first_answer, first_reasoning_content, first_text_delta_count, first_thinking_delta_count) =
        run_turn(first_messages.clone()).await;

    assert!(!first_answer.trim().is_empty());
    assert!(!first_reasoning_content.trim().is_empty());
    assert!(first_text_delta_count > 0);
    assert!(first_thinking_delta_count > 0);

    let second_messages = vec![
        first_messages[0].clone(),
        assistant_message(first_answer, first_reasoning_content),
        user_message("继续上一题，把答案改写成不超过 12 个字。"),
    ];
    let (
        second_answer,
        second_reasoning_content,
        second_text_delta_count,
        second_thinking_delta_count,
    ) = run_turn(second_messages).await;

    assert!(!second_answer.trim().is_empty());
    assert!(!second_reasoning_content.trim().is_empty());
    assert!(second_text_delta_count > 0);
    assert!(second_thinking_delta_count > 0);
}
