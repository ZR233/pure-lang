use pl_model::{
    ModelProvider, OpenAiCompactionMode, ProviderWireProtocol, ReasoningConfig, TokenUsage,
    ToolSchema,
};
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

const DEFAULT_COMPACT_PROMPT: &str = include_str!("../../prompts/compact.md");
const DEFAULT_SUMMARY_REQUEST: &str = "请根据以上完整上下文生成压缩摘要。";
const RECENT_USER_TOKEN_BUDGET: u64 = 20_000;
const APPROX_CHARS_PER_TOKEN: u64 = 4;
pub(crate) const SUMMARY_METADATA_KEY: &str = "context_compaction";
pub(crate) const SUMMARY_METADATA_VALUE: &str = "summary";
const DEFAULT_SUMMARY_PREFIX: &str = "以下是此前对话的压缩摘要。";
const DEFAULT_EMPTY_SUMMARY_ERROR: &str = "context compaction returned an empty summary";
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
                message.content = pl_protocol::MessageContent::Text(
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
                message.content = pl_protocol::MessageContent::Text(
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

pub(crate) struct ContextCompactionRequest<'a, P: ModelProvider + ?Sized> {
    pub provider: &'a P,
    pub model: &'a str,
    pub config: &'a ContextCompactionConfig,
    pub request_instructions: &'a str,
    pub request_messages: &'a [Message],
    pub working_context_tail: Option<Message>,
    pub tools: &'a [ToolSchema],
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
    pub trigger: CompactionTrigger,
    pub phase: ContextCompactionPhase,
    pub event_tx: AgentEventSender,
    pub recorder: &'a mut TraceRecorder,
    pub progress: Option<&'a mut ProgressEmitter>,
}

pub(crate) async fn maybe_compact_session(
    session: &mut AgentSession,
    request: ContextCompactionRequest<'_, impl ModelProvider + ?Sized>,
) -> Result<CompactionOutcome> {
    let ContextCompactionRequest {
        provider,
        model,
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
    } = request;
    if !has_compactable_history(session.items(), trigger) {
        return Ok(CompactionOutcome::Skipped);
    }
    let model_info = provider.model_info(model);
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
    let (replacement, usage, summary, implementation, replacement_tokens) = if use_remote {
        let (replacement, usage) = remote::compact_remote(
            session,
            remote::RemoteCompactionRequest {
                provider,
                model,
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
        (replacement, usage, None, implementation, None)
    } else {
        let (replacement, usage, summary) = compact_local(
            provider,
            model,
            config,
            request_instructions,
            request_messages,
            session.items(),
            working_context_tail.as_ref(),
            event_tx,
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
        (
            replacement,
            Some(usage),
            Some(summary),
            ContextCompactionImplementation::Local,
            replacement_tokens,
        )
    };
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
    tools: &[ToolSchema],
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
mod tests;
