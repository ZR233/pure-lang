use pl_model::{
    ModelProvider, OpenAiCompactionMode, ProviderKind, ReasoningConfig, TokenUsage, ToolSchema,
};
use pl_protocol::{Message, ModelContextItem, PureError, Result};
use pl_trace::AgentEventSender;

use crate::core::progress::ProgressEmitter;
use crate::session::CoreSession;
use crate::{CompileMode, InstructionSnapshot};

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
}

impl ContextCompactionTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EstimatedTokens => "estimatedTokens",
            Self::ProviderPromptTokens => "providerPromptTokens",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCompactionImplementation {
    Local,
    RemoteLegacy,
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
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<InstructionSnapshot>,
}

impl ManualContextCompactionRequest {
    pub fn new(mode: CompileMode) -> Self {
        Self {
            turn_id: None,
            mode,
            workspace_instructions: None,
            instruction_snapshot: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactionTrigger {
    EstimatedTokens,
    ProviderPromptTokens(u64),
    Manual,
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
    pub tools: &'a [ToolSchema],
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
    pub trigger: CompactionTrigger,
    pub phase: ContextCompactionPhase,
    pub event_tx: AgentEventSender,
    pub progress: Option<&'a mut ProgressEmitter>,
}

pub(crate) async fn maybe_compact_session(
    session: &mut CoreSession,
    request: ContextCompactionRequest<'_, impl ModelProvider + ?Sized>,
) -> Result<CompactionOutcome> {
    let ContextCompactionRequest {
        provider,
        model,
        config,
        request_instructions,
        request_messages,
        tools,
        parallel_tool_calls,
        reasoning,
        prompt_cache_key,
        trigger,
        phase,
        event_tx,
        mut progress,
    } = request;
    if !has_compactable_history(session.items(), trigger) {
        return Ok(CompactionOutcome::Skipped);
    }
    ensure_provider_can_consume_session(provider.info().provider_kind, session)?;
    let model_info = provider.model_info(model);
    let limit = match (trigger, model_info.resolved_auto_compact_limit()) {
        (CompactionTrigger::Manual, limit) => limit.unwrap_or_default(),
        (_, Some(limit)) => limit,
        (_, None) => return Ok(CompactionOutcome::Skipped),
    };
    let estimated_tokens =
        estimate_context_request_tokens(request_instructions, request_messages, session.items());
    let should_compact = match trigger {
        CompactionTrigger::EstimatedTokens => estimated_tokens >= limit,
        CompactionTrigger::ProviderPromptTokens(prompt_tokens) => {
            prompt_tokens >= limit || estimated_tokens >= limit
        }
        CompactionTrigger::Manual => true,
    };
    if !should_compact {
        return Ok(CompactionOutcome::Skipped);
    }

    if let Some(progress) = progress.as_mut() {
        progress.milestone("上下文接近上限，正在压缩历史。");
    }
    let use_remote = provider.info().provider_kind == ProviderKind::OpenAi
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
                tools,
                parallel_tool_calls,
                reasoning,
                prompt_cache_key,
            },
        )
        .await?;
        let implementation = match config.openai_mode {
            OpenAiCompactionMode::RemoteV2 => ContextCompactionImplementation::RemoteV2,
            OpenAiCompactionMode::RemoteLegacy => ContextCompactionImplementation::RemoteLegacy,
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
            event_tx,
            &mut progress,
            model_info.max_output_tokens,
        )
        .await?;
        let replacement_tokens = Some(estimate_context_request_tokens(
            request_instructions,
            request_messages,
            &replacement,
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
    session.replace_items(replacement);
    if let Some(progress) = progress {
        progress.milestone("上下文已压缩，继续准备模型调用。");
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
    }
}

fn provider_prompt_tokens(trigger: CompactionTrigger) -> Option<u64> {
    match trigger {
        CompactionTrigger::ProviderPromptTokens(tokens) => Some(tokens),
        CompactionTrigger::EstimatedTokens | CompactionTrigger::Manual => None,
    }
}

pub(crate) fn ensure_provider_can_consume_session(
    provider_kind: ProviderKind,
    session: &CoreSession,
) -> Result<()> {
    if provider_kind != ProviderKind::OpenAi
        && session.items().iter().any(ModelContextItem::is_compaction)
    {
        return Err(PureError::ConfigError(
            "当前会话包含仅 OpenAI 可读取的远程压缩 checkpoint；请继续使用 OpenAI provider，或新建会话后再切换 provider。"
                .to_string(),
        ));
    }
    Ok(())
}

fn estimate_context_request_tokens(
    instructions: &str,
    request_messages: &[Message],
    session_items: &[ModelContextItem],
) -> u64 {
    estimate_text_tokens(instructions)
        + request_messages
            .iter()
            .map(history::estimate_message_tokens)
            .sum::<u64>()
        + session_items
            .iter()
            .map(|item| match item {
                ModelContextItem::Message { message } => history::estimate_message_tokens(message),
                ModelContextItem::Compaction { encrypted_content } => {
                    estimate_text_tokens(encrypted_content)
                }
            })
            .sum::<u64>()
}

#[cfg(test)]
mod tests;
