use pl_protocol::Result;

use crate::context_assembler::{AssembledModelContext, ContextAssembler, TurnContextSnapshot};
use crate::context_compaction::{
    CompactionOutcome, ContextCompactionConfig, ContextCompactionPhase, ContextCompactionRequest,
    ContextCompactionSnapshot, maybe_compact_session,
};
use crate::instruction::InstructionBundle;
use crate::runtime_usage::{InferenceBillingInput, inference_billing_record};
use crate::session::AgentSession;
use crate::time::unix_seconds;
use crate::tool::SubagentContext;
use crate::trace::TraceRecorder;
use crate::turn::{TurnOptions, TurnResult};
use crate::working_set::TurnWorkingSetHandle;
use crate::{PromptCacheInput, prepare_prompt_context};

use super::super::permission::cancellation_reason;
use super::super::progress::ProgressEmitter;
use super::super::turn_result::{
    failed_turn_result, interrupted_turn_result, is_cancelled, normalize_provider_error,
};
use super::{checkpoint, inference, prompt_cache};
use pl_model::completion::ReasoningConfig;
use pl_model::provider::EffectivePromptCachePolicy;
use pl_model::runtime::ModelRuntime;
use pl_protocol::ToolSpec;

/// 单次 turn 循环内执行上下文压缩所需的共享状态。
///
/// 由 `run_turn_with_trace` 在每次迭代以原变量名填充；字段即循环局部变量，
/// 保持压缩步骤与编排循环之间的依赖显式可见。
pub(super) struct CompactionStep<'a> {
    pub(super) session: &'a mut AgentSession,
    pub(super) runtime: &'a ModelRuntime,
    pub(super) config: &'a ContextCompactionConfig,
    pub(super) options: &'a TurnOptions,
    pub(super) recorder: &'a mut TraceRecorder,
    pub(super) progress: &'a mut ProgressEmitter,
    pub(super) turn_id: &'a str,
    pub(super) model: &'a str,
    pub(super) active_subagent: Option<&'a SubagentContext>,
    pub(super) instruction_bundle: &'a InstructionBundle,
    pub(super) assembled_context: &'a mut AssembledModelContext,
    pub(super) turn_context: &'a mut TurnContextSnapshot,
    pub(super) working_set: &'a TurnWorkingSetHandle,
    pub(super) iteration_tools: &'a [ToolSpec],
    pub(super) parallel_tool_calls: bool,
    pub(super) reasoning: Option<&'a ReasoningConfig>,
    pub(super) prompt_cache_policy: EffectivePromptCachePolicy,
    pub(super) iteration: u32,
    pub(super) last_content: &'a str,
    pub(super) last_reasoning_content: &'a Option<String>,
    pub(super) last_model: &'a str,
    pub(super) total_usage: &'a mut pl_protocol::TokenUsage,
    pub(super) inference_count: &'a mut u64,
    pub(super) context_compactions: &'a mut Vec<ContextCompactionSnapshot>,
    pub(super) safe_message_count: &'a mut usize,
    pub(super) session_message_count: &'a mut usize,
    pub(super) last_compacted_state: &'a mut Option<(u64, usize)>,
    pub(super) provider_prompt_tokens_for_compaction: &'a mut Option<u64>,
}

/// 执行本轮的上下文压缩步骤。
///
/// 返回 `Some(result)` 表示压缩失败或被取消，turn 循环需要立即以该结果终止；
/// 返回 `None` 表示继续后续 inference 步骤。
pub(super) async fn run(step: CompactionStep<'_>) -> Result<Option<TurnResult>> {
    let CompactionStep {
        session,
        runtime,
        config,
        options,
        recorder,
        progress,
        turn_id,
        model,
        active_subagent,
        instruction_bundle,
        assembled_context,
        turn_context,
        working_set,
        iteration_tools,
        parallel_tool_calls,
        reasoning,
        prompt_cache_policy,
        iteration,
        last_content,
        last_reasoning_content,
        last_model,
        total_usage,
        inference_count,
        context_compactions,
        safe_message_count,
        session_message_count,
        last_compacted_state,
        provider_prompt_tokens_for_compaction,
    } = step;
    let compaction_trigger = provider_prompt_tokens_for_compaction.take().map_or(
        crate::context_compaction::CompactionTrigger::EstimatedTokens,
        |prompt_tokens| {
            crate::context_compaction::CompactionTrigger::ProviderPromptTokens(prompt_tokens)
        },
    );
    let current_compaction_state = (session.revision(), session.len());
    if *last_compacted_state != Some(current_compaction_state) {
        let compaction_result = maybe_compact_session(
            session,
            ContextCompactionRequest {
                runtime,
                config,
                request_instructions: &assembled_context.instructions,
                request_messages: &assembled_context.prelude_messages,
                working_context_tail: assembled_context.working_context_tail.clone(),
                tools: iteration_tools,
                parallel_tool_calls,
                reasoning: reasoning.cloned(),
                prompt_cache_key: session.prompt_cache_key().map(ToString::to_string),
                trigger: compaction_trigger,
                phase: if iteration == 0 {
                    ContextCompactionPhase::PreTurn
                } else {
                    ContextCompactionPhase::MidTurn
                },
                event_tx: recorder.sender().clone(),
                recorder,
                progress: Some(progress),
                control: options.context_compaction_control(),
            },
        )
        .await;
        match compaction_result {
            Ok(CompactionOutcome::Skipped) => {}
            Ok(CompactionOutcome::Compacted { usage, snapshot }) => {
                *last_compacted_state = Some((session.revision(), session.len()));
                *safe_message_count = session.len();
                *session_message_count = *safe_message_count;
                let compaction_inference = usage.map(|usage| {
                    total_usage.prompt_tokens += usage.prompt_tokens;
                    total_usage.completion_tokens += usage.completion_tokens;
                    total_usage.cached_prompt_tokens += usage.cached_prompt_tokens;
                    total_usage.cache_write_tokens += usage.cache_write_tokens;
                    total_usage.reasoning_tokens += usage.reasoning_tokens;
                    total_usage.total_tokens += usage
                        .total_tokens
                        .max(usage.prompt_tokens.saturating_add(usage.completion_tokens));
                    *inference_count = (*inference_count).saturating_add(1);
                    let model_info = runtime.model().clone();
                    let inference_id = format!("{turn_id}-compact-{iteration}");
                    let recorded_at = unix_seconds();
                    let billing = inference_billing_record(InferenceBillingInput {
                        inference_id,
                        provider_instance_id: runtime.provider_instance_id(),
                        provider: &runtime.endpoint().name,
                        model,
                        usage: &usage,
                        model_info: &model_info,
                        prompt_cache_policy,
                        prompt: prompt_cache::current(session, &options.prompt_scope),
                        orchestration: Default::default(),
                        timing: None,
                        recorded_at,
                    });
                    inference::from_billing(active_subagent, billing)
                });
                context_compactions.push(snapshot);
                working_set.sync_session(session)?;
                turn_context.refresh_working_context(working_set.model_context_snapshot(session));
                prepare_prompt_context(
                    session,
                    PromptCacheInput {
                        scope: &options.prompt_scope,
                        provider: runtime.endpoint(),
                        model,
                        instructions: &instruction_bundle.instructions,
                        prelude_messages: &instruction_bundle.prelude_messages,
                        working_context: Some(turn_context.model_context()),
                        fixed_prefix_section_hashes: instruction_bundle
                            .prefix_section_hashes
                            .clone(),
                        tools: iteration_tools,
                        tool_choice: "auto",
                        parallel_tool_calls,
                        reasoning,
                        output_schema: None,
                        service_tier: None,
                        compacted: true,
                        prompt_cache_policy,
                        updated_at: unix_seconds(),
                    },
                )?;
                prompt_cache::sync(session, options, prompt_cache_policy)?;
                turn_context.rebase(session.items());
                *assembled_context = ContextAssembler::assemble_turn(
                    &instruction_bundle.instructions,
                    &instruction_bundle.prelude_messages,
                    session.items(),
                    turn_context,
                )?;
                if let Some(inference) = compaction_inference {
                    inference::record(options, session, recorder, inference).await?;
                } else {
                    checkpoint::persist(
                        options,
                        session,
                        crate::TurnCheckpointReason::ContextCompacted,
                    )
                    .await?;
                }
            }
            Err(error) => {
                if is_cancelled(options) {
                    session.truncate_messages(*safe_message_count);
                    return Ok(Some(interrupted_turn_result(
                        recorder,
                        turn_id,
                        last_content.to_string(),
                        last_reasoning_content.clone(),
                        last_model.to_string(),
                        total_usage.clone(),
                        *safe_message_count,
                        cancellation_reason(),
                    )));
                }
                let (error, severity, failure) = normalize_provider_error(active_subagent, error);
                return Ok(Some(failed_turn_result(
                    recorder,
                    turn_id,
                    last_content.to_string(),
                    last_reasoning_content.clone(),
                    last_model.to_string(),
                    total_usage.clone(),
                    session.len(),
                    error,
                    severity,
                    failure,
                )));
            }
        }
    }
    Ok(None)
}
