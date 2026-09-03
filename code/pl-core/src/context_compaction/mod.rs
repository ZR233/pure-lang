use pl_protocol::{Message, ModelContextItem, PureError, Result};
use pl_trace::AgentEventSender;

use crate::core::progress::ProgressEmitter;
use crate::instruction::InstructionSnapshot;
use crate::session::AgentSession;
use crate::{AgentExecutionPolicy, TraceRecorder};

mod history;
mod local;
mod remote;

#[cfg(test)]
use history::build_compacted_history;
use history::{estimate_text_tokens, has_compactable_history};
use local::compact_local;
use pl_model::completion::{OpenAiCompactionMode, ReasoningConfig};
use pl_model::provider::ProviderWireProtocol;
use pl_model::runtime::ModelRuntime;
use pl_protocol::{TokenUsage, ToolSpec};
use std::future::Future;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const DEFAULT_COMPACT_PROMPT: &str = include_str!("../../prompts/compact.md");
const DEFAULT_SUMMARY_REQUEST: &str = "请根据以上完整上下文生成压缩摘要。";
const RECENT_USER_TOKEN_BUDGET: u64 = 20_000;
const APPROX_CHARS_PER_TOKEN: u64 = 4;
pub(crate) const SUMMARY_METADATA_KEY: &str = "context_compaction";
pub(crate) const SUMMARY_METADATA_VALUE: &str = "summary";
const DEFAULT_SUMMARY_PREFIX: &str = "以下是此前对话的压缩摘要。";
const DEFAULT_EMPTY_SUMMARY_ERROR: &str = "context compaction returned an empty summary";
const CONTEXT_COMPACTION_TIMEOUT: Duration = Duration::from_secs(120);
const RETAINED_FULL_TOOL_RESULTS_FOR_COMPACTION: usize = 3;

fn compact_old_tool_results_for_request(input: &mut [ModelContextItem]) {
    let compact_count = input
        .iter()
        .filter(|item| {
            item.as_message()
                .is_some_and(|message| message.role == pl_protocol::MessageRole::Tool)
        })
        .count()
        .saturating_sub(RETAINED_FULL_TOOL_RESULTS_FOR_COMPACTION);
    let mut remaining = compact_count;
    for item in input {
        if remaining == 0 {
            break;
        }
        match item {
            ModelContextItem::ToolResult { message, receipt }
                if message.role == pl_protocol::MessageRole::Tool =>
            {
                message.content = pl_protocol::MessageContent::text(
                    serde_json::json!({
                        "compactedToolResult": true,
                        "receipt": receipt,
                    })
                    .to_string(),
                );
                remaining -= 1;
            }
            ModelContextItem::Message { message }
                if message.role == pl_protocol::MessageRole::Tool =>
            {
                let text = serde_json::to_string(&message.content).unwrap_or_default();
                message.content = pl_protocol::MessageContent::text(
                    serde_json::json!({
                        "compactedToolResult": true,
                        "resultHash": crate::canonical_content_hash(text.as_bytes()),
                        "visibleBytes": text.len(),
                    })
                    .to_string(),
                );
                remaining -= 1;
            }
            ModelContextItem::Message { .. }
            | ModelContextItem::ToolResult { .. }
            | ModelContextItem::ToolMedia { .. }
            | ModelContextItem::Compaction { .. }
            | ModelContextItem::Responses { .. } => {}
        }
    }
}

/// 上下文压缩的产品级文案与实现配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionConfig {
    pub instructions: String,
    pub summary_request: String,
    pub summary_prefix: String,
    pub empty_summary_error: String,
    pub replacement: ContextCompactionReplacement,
    pub openai_mode: OpenAiCompactionMode,
}

impl ContextCompactionConfig {
    pub fn new(
        instructions: impl Into<String>,
        summary_request: impl Into<String>,
        summary_prefix: impl Into<String>,
        empty_summary_error: impl Into<String>,
    ) -> Self {
        Self {
            instructions: instructions.into(),
            summary_request: summary_request.into(),
            summary_prefix: summary_prefix.into(),
            empty_summary_error: empty_summary_error.into(),
            replacement: ContextCompactionReplacement::default(),
            openai_mode: OpenAiCompactionMode::default(),
        }
    }

    pub fn with_replacement(mut self, replacement: ContextCompactionReplacement) -> Self {
        self.replacement = replacement;
        self
    }

    pub fn with_openai_mode(mut self, mode: OpenAiCompactionMode) -> Self {
        self.openai_mode = mode;
        self
    }
}

impl Default for ContextCompactionConfig {
    fn default() -> Self {
        Self {
            instructions: DEFAULT_COMPACT_PROMPT.to_string(),
            summary_request: DEFAULT_SUMMARY_REQUEST.to_string(),
            summary_prefix: DEFAULT_SUMMARY_PREFIX.to_string(),
            empty_summary_error: DEFAULT_EMPTY_SUMMARY_ERROR.to_string(),
            replacement: ContextCompactionReplacement::default(),
            openai_mode: OpenAiCompactionMode::default(),
        }
    }
}

/// 压缩后历史的保留策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCompactionReplacement {
    SummaryThenRecentUsers { token_budget: u64 },
    RecentUsersThenSummary { token_budget: u64 },
    RecentInteractionTail(RecentInteractionTailConfig),
}

impl Default for ContextCompactionReplacement {
    fn default() -> Self {
        Self::RecentUsersThenSummary {
            token_budget: RECENT_USER_TOKEN_BUDGET,
        }
    }
}

/// 压缩后保留近期交互尾部的限制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentInteractionTailConfig {
    pub max_user_chars: usize,
    pub max_assistant_chars: usize,
    pub max_tool_output_chars: usize,
    pub assistant_items: usize,
    pub tool_output_items: usize,
}

/// 触发上下文压缩的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens,
    Manual,
    WallClockRollover,
}

impl ContextCompactionTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EstimatedTokens => "estimatedTokens",
            Self::ProviderPromptTokens => "providerPromptTokens",
            Self::Manual => "manual",
            Self::WallClockRollover => "wallClockRollover",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionImplementation {
    Local,
    RemoteV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionPhase {
    PreTurn,
    MidTurn,
    Standalone,
}

/// 单次上下文压缩的可观测快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCompactionSnapshot {
    pub trigger: ContextCompactionTrigger,
    pub tokens_before: u64,
    pub estimated_request_tokens: u64,
    pub provider_prompt_tokens: Option<u64>,
    pub auto_compact_limit: u64,
    pub replacement_tokens: Option<u64>,
    pub summary: Option<String>,
    pub implementation: ContextCompactionImplementation,
    pub phase: ContextCompactionPhase,
}

/// 核心手动压缩请求。
#[derive(Debug, Clone)]
pub struct ManualContextCompactionRequest {
    pub turn_id: Option<String>,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<InstructionSnapshot>,
    pub execution_policy: Option<AgentExecutionPolicy>,
    pub trigger: ContextCompactionTrigger,
}

impl ManualContextCompactionRequest {
    pub fn new() -> Self {
        Self {
            turn_id: None,
            workspace_instructions: None,
            instruction_snapshot: None,
            execution_policy: None,
            trigger: ContextCompactionTrigger::Manual,
        }
    }
}

impl Default for ManualContextCompactionRequest {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens(u64),
    Manual,
    WallClockRollover,
}

#[derive(Debug, Clone)]
pub(crate) enum CompactionOutcome {
    Skipped,
    Compacted {
        usage: Option<TokenUsage>,
        snapshot: ContextCompactionSnapshot,
    },
}

pub(crate) struct ContextCompactionRequest<'a> {
    pub runtime: &'a ModelRuntime,
    pub config: &'a ContextCompactionConfig,
    pub request_instructions: &'a str,
    pub request_messages: &'a [Message],
    pub working_context_tail: Option<Message>,
    pub tools: &'a [ToolSpec],
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
    pub trigger: CompactionTrigger,
    pub phase: ContextCompactionPhase,
    pub event_tx: AgentEventSender,
    pub recorder: &'a mut TraceRecorder,
    pub progress: Option<&'a mut ProgressEmitter>,
    pub control: ContextCompactionControl,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextCompactionControl {
    cancellation_token: Option<CancellationToken>,
    timeout: Duration,
}

impl ContextCompactionControl {
    #[cfg(test)]
    pub(crate) fn with_cancellation(mut self, cancellation_token: CancellationToken) -> Self {
        self.cancellation_token = Some(cancellation_token);
        self
    }

    pub(crate) fn with_optional_cancellation(
        mut self,
        cancellation_token: Option<CancellationToken>,
    ) -> Self {
        self.cancellation_token = cancellation_token;
        self
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for ContextCompactionControl {
    fn default() -> Self {
        Self {
            cancellation_token: None,
            timeout: CONTEXT_COMPACTION_TIMEOUT,
        }
    }
}

pub(crate) async fn maybe_compact_session(
    session: &mut AgentSession,
    request: ContextCompactionRequest<'_>,
) -> Result<CompactionOutcome> {
    let ContextCompactionRequest {
        runtime,
        config,
        request_instructions,
        request_messages,
        working_context_tail,
        tools,
        parallel_tool_calls,
        reasoning,
        prompt_cache_key,
        trigger,
        phase,
        event_tx,
        recorder,
        mut progress,
        control,
    } = request;
    if !has_compactable_history(session.items(), trigger) {
        return Ok(CompactionOutcome::Skipped);
    }
    let model_info = runtime.model();
    ensure_provider_can_consume_session(model_info.transport.protocol, session)?;
    let limit = match (trigger, model_info.resolved_auto_compact_limit()) {
        (CompactionTrigger::Manual | CompactionTrigger::WallClockRollover, limit) => {
            limit.unwrap_or_default()
        }
        (_, Some(limit)) => limit,
        (_, None) => return Ok(CompactionOutcome::Skipped),
    };
    let estimated_tokens = estimate_context_request_tokens(
        request_instructions,
        request_messages,
        session.items(),
        working_context_tail.as_ref(),
        tools,
    );
    let should_compact = match trigger {
        CompactionTrigger::EstimatedTokens => estimated_tokens >= limit,
        CompactionTrigger::ProviderPromptTokens(prompt_tokens) => {
            prompt_tokens >= limit || estimated_tokens >= limit
        }
        CompactionTrigger::Manual | CompactionTrigger::WallClockRollover => true,
    };
    if !should_compact {
        return Ok(CompactionOutcome::Skipped);
    }

    if let Some(progress) = progress.as_mut() {
        progress.milestone(recorder, "上下文接近上限，正在压缩历史。");
    }
    let use_remote = model_info.transport.protocol == ProviderWireProtocol::Responses
        && config.openai_mode != OpenAiCompactionMode::Local;
    let operation = async {
        if use_remote {
            let (replacement, usage) = remote::compact_remote(
                session,
                remote::RemoteCompactionRequest {
                    runtime,
                    config,
                    request_instructions,
                    request_messages,
                    working_context_tail: working_context_tail.clone(),
                    tools,
                    parallel_tool_calls,
                    reasoning,
                    prompt_cache_key,
                },
            )
            .await?;
            let implementation = match config.openai_mode {
                OpenAiCompactionMode::RemoteV2 => ContextCompactionImplementation::RemoteV2,
                OpenAiCompactionMode::Local => unreachable!("local mode was excluded"),
            };
            Ok((replacement, usage, None, implementation, None))
        } else {
            let (replacement, usage, summary) = compact_local(
                runtime,
                config,
                request_instructions,
                request_messages,
                session.items(),
                working_context_tail.as_ref(),
                event_tx,
                prompt_cache_key,
                recorder,
                &mut progress,
                model_info.max_output_tokens,
            )
            .await?;
            let replacement_tokens = Some(estimate_context_request_tokens(
                request_instructions,
                request_messages,
                &replacement,
                working_context_tail.as_ref(),
                tools,
            ));
            Ok((
                replacement,
                Some(usage),
                Some(summary),
                ContextCompactionImplementation::Local,
                replacement_tokens,
            ))
        }
    };
    let (replacement, usage, summary, implementation, replacement_tokens) =
        run_compaction_operation(control, operation).await?;
    let snapshot = ContextCompactionSnapshot {
        trigger: public_trigger(trigger),
        tokens_before: provider_prompt_tokens(trigger).unwrap_or(estimated_tokens),
        estimated_request_tokens: estimated_tokens,
        provider_prompt_tokens: provider_prompt_tokens(trigger),
        auto_compact_limit: limit,
        replacement_tokens,
        summary,
        implementation,
        phase,
    };
    session.replace_compactable_items(replacement);
    if let Some(progress) = progress {
        progress.milestone(recorder, "上下文已压缩，继续准备模型调用。");
    }
    Ok(CompactionOutcome::Compacted { usage, snapshot })
}

async fn run_compaction_operation<T>(
    control: ContextCompactionControl,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    let timed = tokio::time::timeout(control.timeout, operation);
    let outcome = match control.cancellation_token {
        Some(cancellation_token) => {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    return Err(PureError::MemoryError(
                        "context compaction cancelled with the current turn".to_string(),
                    ));
                }
                outcome = timed => outcome,
            }
        }
        None => timed.await,
    };
    outcome.map_err(|_| {
        PureError::transient_model_transport(format!(
            "context compaction timed out after {}ms",
            control.timeout.as_millis()
        ))
    })?
}

fn public_trigger(trigger: CompactionTrigger) -> ContextCompactionTrigger {
    match trigger {
        CompactionTrigger::EstimatedTokens => ContextCompactionTrigger::EstimatedTokens,
        CompactionTrigger::ProviderPromptTokens(_) => {
            ContextCompactionTrigger::ProviderPromptTokens
        }
        CompactionTrigger::Manual => ContextCompactionTrigger::Manual,
        CompactionTrigger::WallClockRollover => ContextCompactionTrigger::WallClockRollover,
    }
}

fn provider_prompt_tokens(trigger: CompactionTrigger) -> Option<u64> {
    match trigger {
        CompactionTrigger::ProviderPromptTokens(tokens) => Some(tokens),
        CompactionTrigger::EstimatedTokens
        | CompactionTrigger::Manual
        | CompactionTrigger::WallClockRollover => None,
    }
}

pub(crate) fn ensure_provider_can_consume_session(
    protocol: ProviderWireProtocol,
    session: &AgentSession,
) -> Result<()> {
    if protocol != ProviderWireProtocol::Responses
        && session
            .items()
            .iter()
            .any(|item| item.is_compaction() || matches!(item, ModelContextItem::Responses { .. }))
    {
        return Err(PureError::ConfigError(
            "当前会话包含仅 Responses provider 可回放的原生上下文；请继续使用 Responses provider，或新建会话后再切换 provider。"
                .to_string(),
        ));
    }
    Ok(())
}

fn estimate_context_request_tokens(
    instructions: &str,
    request_messages: &[Message],
    session_items: &[ModelContextItem],
    working_context_tail: Option<&Message>,
    tools: &[ToolSpec],
) -> u64 {
    estimate_text_tokens(instructions)
        + request_messages
            .iter()
            .map(history::estimate_message_tokens)
            .sum::<u64>()
        + session_items
            .iter()
            .map(|item| match item {
                ModelContextItem::Message { message }
                | ModelContextItem::ToolResult { message, .. } => {
                    history::estimate_message_tokens(message)
                }
                ModelContextItem::Compaction { encrypted_content } => {
                    estimate_text_tokens(encrypted_content)
                }
                ModelContextItem::Responses { item } => serde_json::to_string(&item.value)
                    .map_or(0, |value| estimate_text_tokens(&value)),
                ModelContextItem::ToolMedia { items } => serde_json::to_string(items.as_slice())
                    .map_or(0, |value| estimate_text_tokens(&value)),
            })
            .sum::<u64>()
        + working_context_tail.map_or(0, history::estimate_message_tokens)
        + tools
            .iter()
            .map(|tool| {
                serde_json::to_string(tool).map_or(0, |schema| estimate_text_tokens(&schema))
            })
            .sum::<u64>()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::future::pending;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use pl_protocol::{
        Message, MessageContent, MessagePresentation, MessageRole, ModelContextItem,
    };
    use pl_trace::{AgentEvent, AgentEventSender, TracePartSource};
    use pretty_assertions::assert_eq;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::history::{
        has_compactable_history, is_compaction_summary, message_text, recent_user_messages,
    };
    use super::*;
    use crate::core::progress::{ProgressEmitter, ProgressVerbosity};
    use pl_model::completion::OpenAiCompactionMode;
    use pl_model::model::{ModelInfo, ModelTransportProfile};
    use pl_model::provider::{ProviderEndpoint, ProviderWireProtocol};
    use pl_model::runtime::ModelRuntime;

    fn text_message(role: MessageRole, text: &str) -> Message {
        Message {
            presentation: Default::default(),
            role,
            content: MessageContent::text(text.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn compacted_history_filters_old_summary_and_places_new_summary_last() {
        let mut old_summary = text_message(MessageRole::User, "old summary");
        old_summary.metadata.insert(
            SUMMARY_METADATA_KEY.to_string(),
            SUMMARY_METADATA_VALUE.to_string(),
        );
        let messages = vec![
            old_summary,
            text_message(MessageRole::User, "old request"),
            text_message(MessageRole::Assistant, "old answer"),
            text_message(MessageRole::Tool, "tool output"),
            text_message(MessageRole::User, "latest request"),
        ];

        let compacted = build_compacted_history(
            &messages,
            "new summary",
            &ContextCompactionConfig::default(),
        );

        assert_eq!(compacted.len(), 3);
        assert_eq!(message_text(&compacted[0]), "old request");
        assert_eq!(message_text(&compacted[1]), "latest request");
        assert!(is_compaction_summary(&compacted[2]));
        assert_eq!(compacted[2].presentation, MessagePresentation::Hidden);
    }

    #[test]
    fn recent_user_boundary_is_truncated_to_token_budget() {
        let messages = vec![
            text_message(MessageRole::User, "first request"),
            text_message(MessageRole::User, "latest request with a long body"),
        ];

        let users = recent_user_messages(&messages, 1);

        assert_eq!(users.len(), 1);
        assert_eq!(message_text(&users[0]), "body");
    }

    #[test]
    fn manual_compaction_accepts_one_real_message_but_auto_does_not() {
        let items = vec![ModelContextItem::from(text_message(
            MessageRole::User,
            "latest request",
        ))];

        assert!(!has_compactable_history(
            &items,
            CompactionTrigger::EstimatedTokens
        ));
        assert!(has_compactable_history(&items, CompactionTrigger::Manual));
        assert!(!has_compactable_history(
            &[ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string(),
            }],
            CompactionTrigger::Manual,
        ));
    }

    #[tokio::test]
    async fn local_context_pressure_retry_emits_progress_and_preserves_summary_order() {
        let provider =
            FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure).await;
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());
        let mut progress = ProgressEmitter::new("turn-compact", ProgressVerbosity::Normal);
        let mut session = test_session();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

        let outcome = maybe_compact_session(
            &mut session,
            compaction_request(
                &provider,
                &config,
                event_tx,
                &mut recorder,
                Some(&mut progress),
            ),
        )
        .await
        .unwrap();

        assert!(matches!(outcome, CompactionOutcome::Compacted { .. }));
        // Chat wire messages include the compaction instruction as a system message.
        assert_eq!(provider.recorded_wire_item_counts(), vec![5, 4]);
        assert!(session.messages().last().is_some_and(is_compaction_summary));
        assert_eq!(
            session.messages().last().unwrap().presentation,
            MessagePresentation::Hidden
        );
        assert_eq!(
            runtime_progress_texts(&mut event_rx),
            vec![
                "上下文接近上限，正在压缩历史。".to_string(),
                "上下文压缩请求过大，正在缩小历史后重试。".to_string(),
                "上下文已压缩，继续准备模型调用。".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn local_retries_without_unsupported_max_output_tokens() {
        let provider = FakeCompactionProvider::new(
            test_model(),
            FakeCompactionFailure::UnsupportedMaxOutputTokens,
        )
        .await;
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());
        let mut session = test_session();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

        maybe_compact_session(
            &mut session,
            compaction_request(&provider, &config, event_tx, &mut recorder, None),
        )
        .await
        .unwrap();

        // Chat wire messages include the compaction instruction as a system message.
        assert_eq!(provider.recorded_wire_item_counts(), vec![5, 5]);
        assert_eq!(provider.recorded_max_tokens(), vec![Some(4096), None]);
    }

    #[tokio::test]
    async fn local_empty_summary_preserves_session_history_and_revision() {
        let provider =
            FakeCompactionProvider::new(test_model(), FakeCompactionFailure::EmptySummary).await;
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());
        let mut session = test_session();
        let original_items = session.items().to_vec();
        let original_revision = session.revision();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);

        let error = maybe_compact_session(
            &mut session,
            compaction_request(&provider, &config, event_tx, &mut recorder, None),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains(&config.empty_summary_error));
        assert_eq!(session.items(), original_items.as_slice());
        assert_eq!(session.revision(), original_revision);
    }

    #[tokio::test]
    async fn remote_failure_does_not_replace_session_history_or_revision() {
        let provider = FakeCompactionProvider::new(
            responses_test_model(),
            FakeCompactionFailure::RemoteFailure,
        )
        .await;
        let mut session = test_session();
        let original_items = session.items().to_vec();
        let original_revision = session.revision();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());

        let error = maybe_compact_session(
            &mut session,
            compaction_request(&provider, &config, event_tx, &mut recorder, None),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("remote compaction failed"));
        assert_eq!(session.items(), original_items.as_slice());
        assert_eq!(session.revision(), original_revision);
    }

    #[tokio::test]
    async fn chat_completions_provider_always_uses_local_compaction() {
        let provider =
            FakeCompactionProvider::new(test_model(), FakeCompactionFailure::ContextPressure).await;
        let mut session = test_session();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());

        let outcome = maybe_compact_session(
            &mut session,
            compaction_request(&provider, &config, event_tx, &mut recorder, None),
        )
        .await
        .unwrap();

        let CompactionOutcome::Compacted { snapshot, .. } = outcome else {
            panic!("expected compaction");
        };
        assert_eq!(
            snapshot.implementation,
            ContextCompactionImplementation::Local
        );
        // Chat wire messages include the compaction instruction as a system message.
        assert_eq!(provider.recorded_wire_item_counts(), vec![5, 4]);
    }

    #[test]
    fn encrypted_checkpoint_rejects_chat_completions_provider() {
        let session = AgentSession::from_items(vec![ModelContextItem::Compaction {
            encrypted_content: "encrypted".to_string(),
        }]);

        let error =
            ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
                .unwrap_err();

        assert!(error.to_string().contains("继续使用 Responses provider"));
    }

    #[test]
    fn responses_native_context_rejects_chat_completions_provider() {
        let session = AgentSession::from_items(vec![ModelContextItem::Responses {
            item: pl_protocol::ResponsesContextItem {
                kind: pl_protocol::ResponsesContextItemKind::Program,
                value: serde_json::json!({"type": "program", "id": "program-1"}),
            },
        }]);

        let error =
            ensure_provider_can_consume_session(ProviderWireProtocol::ChatCompletions, &session)
                .unwrap_err();

        assert!(error.to_string().contains("Responses provider"));
    }

    fn test_model() -> ModelInfo {
        let mut model = ModelInfo::fallback("compact-test");
        model.context_window = Some(100);
        model.max_context_window = Some(100);
        model.auto_compact_token_limit = Some(1);
        model.max_output_tokens = Some(4096);
        model
    }

    fn responses_test_model() -> ModelInfo {
        let mut model = test_model();
        model.transport = ModelTransportProfile::responses_http();
        model
    }

    fn test_session() -> AgentSession {
        let mut session = AgentSession::new();
        session.push_user_prompt("old request ".repeat(20));
        session.push_assistant_response("old answer ".repeat(20), None);
        session.push_user_prompt("latest request ".repeat(20));
        session
    }

    fn compaction_request<'a>(
        provider: &'a FakeCompactionProvider,
        config: &'a ContextCompactionConfig,
        event_tx: AgentEventSender,
        recorder: &'a mut TraceRecorder,
        progress: Option<&'a mut ProgressEmitter>,
    ) -> ContextCompactionRequest<'a> {
        ContextCompactionRequest {
            runtime: &provider.runtime,
            config,
            request_instructions: "",
            request_messages: &[],
            working_context_tail: None,
            tools: &[],
            parallel_tool_calls: false,
            reasoning: None,
            prompt_cache_key: None,
            trigger: CompactionTrigger::EstimatedTokens,
            phase: ContextCompactionPhase::PreTurn,
            event_tx,
            recorder,
            progress,
            control: super::ContextCompactionControl::default(),
        }
    }

    #[tokio::test]
    async fn remote_compaction_succeeds_through_the_shared_controller() {
        let provider =
            FakeCompactionProvider::new(responses_test_model(), FakeCompactionFailure::Success)
                .await;
        let mut session = test_session();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::RemoteV2);
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());

        let outcome = maybe_compact_session(
            &mut session,
            compaction_request(&provider, &config, event_tx, &mut recorder, None),
        )
        .await
        .unwrap();

        let CompactionOutcome::Compacted { snapshot, .. } = outcome else {
            panic!("expected compaction");
        };
        assert_eq!(
            snapshot.implementation,
            ContextCompactionImplementation::RemoteV2
        );
        assert!(session.revision() > 0);
    }

    #[tokio::test]
    async fn local_and_remote_compaction_timeouts_preserve_session_atomically() {
        for mode in [OpenAiCompactionMode::Local, OpenAiCompactionMode::RemoteV2] {
            let model = match mode {
                OpenAiCompactionMode::Local => test_model(),
                _ => responses_test_model(),
            };
            let provider = FakeCompactionProvider::new(model, FakeCompactionFailure::Hang).await;
            let mut session = test_session();
            let original_items = session.items().to_vec();
            let original_revision = session.revision();
            let config = ContextCompactionConfig::default().with_openai_mode(mode);
            let (event_tx, _) = tokio::sync::broadcast::channel(8);
            let mut recorder = TraceRecorder::disabled(event_tx.clone());
            let mut request = compaction_request(&provider, &config, event_tx, &mut recorder, None);
            request.control =
                ContextCompactionControl::default().with_timeout(Duration::from_millis(20));

            let error = maybe_compact_session(&mut session, request)
                .await
                .unwrap_err();

            assert!(
                error.to_string().contains("timed out after 20ms"),
                "{mode:?}"
            );
            assert_eq!(session.items(), original_items.as_slice(), "{mode:?}");
            assert_eq!(session.revision(), original_revision, "{mode:?}");
        }
    }

    #[tokio::test]
    async fn compaction_cancellation_preserves_session_atomically() {
        let provider = FakeCompactionProvider::new(test_model(), FakeCompactionFailure::Hang).await;
        let mut session = test_session();
        let original_items = session.items().to_vec();
        let original_revision = session.revision();
        let config =
            ContextCompactionConfig::default().with_openai_mode(OpenAiCompactionMode::Local);
        let (event_tx, _) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::disabled(event_tx.clone());
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let mut request = compaction_request(&provider, &config, event_tx, &mut recorder, None);
        request.control = ContextCompactionControl::default()
            .with_timeout(Duration::from_secs(1))
            .with_cancellation(cancellation);

        let error = maybe_compact_session(&mut session, request)
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cancelled with the current turn")
        );
        assert_eq!(session.items(), original_items.as_slice());
        assert_eq!(session.revision(), original_revision);
    }

    fn runtime_progress_texts(
        event_rx: &mut tokio::sync::broadcast::Receiver<AgentEvent>,
    ) -> Vec<String> {
        let mut texts = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            if let AgentEvent::TracePartCompleted { item } = event
                && item.source() == TracePartSource::Runtime
            {
                texts.push(
                    item.text()
                        .expect("runtime progress text")
                        .content()
                        .to_string(),
                );
            }
        }
        texts
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeCompactionFailure {
        Success,
        ContextPressure,
        UnsupportedMaxOutputTokens,
        EmptySummary,
        RemoteFailure,
        Hang,
    }

    #[derive(Debug)]
    struct FakeCompactionProvider {
        runtime: ModelRuntime,
        requests: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl FakeCompactionProvider {
        async fn new(model: ModelInfo, failure: FakeCompactionFailure) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let protocol = model.transport.protocol;
            tokio::spawn(async move {
                let response_count = match failure {
                    FakeCompactionFailure::Success
                    | FakeCompactionFailure::EmptySummary
                    | FakeCompactionFailure::RemoteFailure
                    | FakeCompactionFailure::Hang => 1,
                    FakeCompactionFailure::ContextPressure
                    | FakeCompactionFailure::UnsupportedMaxOutputTokens => 2,
                };
                for attempt in 0..response_count {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let request = read_http_json_request(&mut socket).await;
                    captured.lock().unwrap().push(request);
                    if failure == FakeCompactionFailure::Hang {
                        pending::<()>().await;
                    }
                    let response = match (failure, attempt) {
                        (FakeCompactionFailure::Success, _) => remote_compaction_response(),
                        (FakeCompactionFailure::ContextPressure, 0) => {
                            error_response("context token limit exceeded")
                        }
                        (FakeCompactionFailure::UnsupportedMaxOutputTokens, 0) => {
                            error_response("Unsupported parameter: max_output_tokens")
                        }
                        (FakeCompactionFailure::RemoteFailure, 0) => {
                            error_response("remote compaction failed")
                        }
                        (FakeCompactionFailure::RemoteFailure, _) => unreachable!(),
                        (FakeCompactionFailure::EmptySummary, _) => {
                            completion_response(protocol, "   ")
                        }
                        (FakeCompactionFailure::ContextPressure, _)
                        | (FakeCompactionFailure::UnsupportedMaxOutputTokens, _) => {
                            completion_response(protocol, "summary")
                        }
                        (FakeCompactionFailure::Hang, _) => unreachable!(),
                    };
                    socket.write_all(response.as_bytes()).await.unwrap();
                    socket.shutdown().await.unwrap();
                }
            });
            let endpoint = ProviderEndpoint::openai(Some(format!("http://{address}/v1")));
            Self {
                runtime: ModelRuntime::new(endpoint, model).unwrap(),
                requests,
            }
        }

        fn recorded_wire_item_counts(&self) -> Vec<usize> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| {
                    request
                        .get("input")
                        .or_else(|| request.get("messages"))
                        .and_then(serde_json::Value::as_array)
                        .map_or(0, Vec::len)
                })
                .collect()
        }

        fn recorded_max_tokens(&self) -> Vec<Option<u64>> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| {
                    request
                        .get("max_output_tokens")
                        .or_else(|| request.get("max_completion_tokens"))
                        .or_else(|| request.get("max_tokens"))
                        .and_then(serde_json::Value::as_u64)
                })
                .collect()
        }
    }

    async fn read_http_json_request(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0);
            bytes.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&bytes);
            let Some((headers, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or_default();
            if body.len() >= content_length {
                return serde_json::from_str(&body[..content_length]).unwrap();
            }
        }
    }

    fn error_response(message: &str) -> String {
        let body = serde_json::json!({
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "code": "invalid_request"
            }
        })
        .to_string();
        format!(
            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn completion_response(protocol: ProviderWireProtocol, content: &str) -> String {
        let body = match protocol {
            ProviderWireProtocol::Responses => format!(
                "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg-1\",\"delta\":{}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp-1\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}}}}\n\ndata: [DONE]\n\n",
                serde_json::to_string(content).unwrap()
            ),
            ProviderWireProtocol::ChatCompletions => format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}}}\n\ndata: [DONE]\n\n",
                serde_json::to_string(content).unwrap()
            ),
        };
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn remote_compaction_response() -> String {
        let body = concat!(
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"encrypted-v2\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        );
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }
}
