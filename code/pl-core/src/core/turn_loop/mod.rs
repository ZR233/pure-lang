use pl_model::{CompletionRequest, ModelProvider, ReasoningConfig, ReasoningSummary};
use pl_protocol::{ErrorSeverity, Result, TokenUsageSnapshot, ToolResultReceipt};
use pl_trace::{AgentEvent, TracePartStatus};
use std::sync::Arc;

mod attachments;
pub(super) mod enabled_tools;
mod plan_exit;

use attachments::materialize_context_items;
use enabled_tools::record_enabled_tools;
use plan_exit::record_plan_exit_items;

use crate::context_assembler::ContextAssembler;
use crate::context_compaction::{
    CompactionOutcome, ContextCompactionPhase, ContextCompactionRequest,
    ensure_provider_can_consume_session, maybe_compact_session,
};
use crate::instruction::{InstructionAssembler, InstructionAssemblyRequest};
use crate::runtime_usage::{agent_runtime_delta, identity_for_subagent, token_usage_snapshot};
use crate::session::AgentSession;
use crate::trace::TraceRecorder;
use crate::turn::{
    BudgetLimit, BudgetTracker, TurnOptions, TurnRequest, TurnResult, TurnResultStatus,
};
use crate::working_set::{TurnWorkingSetChange, TurnWorkingSetHandle, canonical_content_hash};

use super::TurnEngine;
use super::permission::cancellation_reason;
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::tool_dispatch::{ToolExecutionContext, ToolExecutionError, execute_tool_calls};
use super::turn_result::{
    budget_limited_turn_result, default_workspace_root, failed_turn_result,
    failed_turn_result_with_abort_reason, interrupted_turn_result, is_cancelled,
    looks_like_unexecuted_tool_call_text, normalize_provider_error,
    should_request_parallel_tool_calls, unix_seconds,
};

pub(super) async fn run_turn_with_trace(
    core: &TurnEngine,
    session: &mut AgentSession,
    request: TurnRequest,
    recorder: &mut TraceRecorder,
    options: TurnOptions,
) -> Result<TurnResult> {
    let provider = core.provider.clone();
    ensure_provider_can_consume_session(provider.info().protocol, session)?;
    let reasoning_effort = core.reasoning_effort.clone();
    let workspace_root = core
        .workspace_root
        .clone()
        .unwrap_or_else(default_workspace_root);
    let workspace_instructions = core.workspace_instructions.clone();
    let active_subagent = core.active_subagent.clone();
    let cancellation_token = options.cancellation_token.clone();
    let tool_schemas = match options.execution_policy.as_ref() {
        Some(policy) => core.tools.schemas_for_policy(policy),
        None => core.tools.schemas(),
    };
    let mut budget_tracker = BudgetTracker::new(request.budget);
    let mut budget_limit: Option<BudgetLimit> = None;

    let turn_id = request
        .turn_id
        .clone()
        .unwrap_or_else(super::generate_turn_id);
    recorder.user_text_item_with_attachments(
        &turn_id,
        request.prompt.clone(),
        request.trace_attachments.clone(),
    );
    if let Some(prompt_cache_key) = options.prompt_cache_key.clone() {
        session.set_prompt_cache_key(prompt_cache_key);
    }
    session.push_user_content(request.user_content.clone());
    let working_set = TurnWorkingSetHandle::from_session(session)?;
    let tool_cache = crate::TurnToolCacheHandle::default();
    record_enabled_tools(recorder, &turn_id, &tool_schemas);
    let turn_item = recorder.turn_item(&turn_id, TracePartStatus::Running);
    recorder.start_item(turn_item.clone());
    let mut progress = ProgressEmitter::new(
        recorder.sender().clone(),
        turn_id.clone(),
        ProgressVerbosity::from_env(),
    );
    progress.milestone("已接收请求，正在准备上下文。");
    let model = provider.default_model().to_string();

    let mut last_content = String::new();
    let mut last_reasoning_content = None;
    let mut last_model = model.clone();
    let mut last_context_tokens = None;
    let mut context_compactions = Vec::new();
    let mut total_usage = pl_model::TokenUsage::default();
    let mut safe_message_count = session.len();
    let mut session_message_count = safe_message_count;

    let model_info = provider.model_info(&model);
    let instruction_snapshot = match request.instruction_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => {
            let assembly_request = InstructionAssemblyRequest {
                instructions: None,
                skills: core.skills.as_ref(),
                execution_profile: None,
                model: &model_info,
                workspace_root: &workspace_root,
                current_dir: &workspace_root,
                workspace_instructions: request.workspace_instructions.as_deref(),
                subagent_constraint: None,
            };
            match core.instruction_profile.as_ref() {
                Some(profile) => {
                    InstructionAssembler::assemble_with_profile(assembly_request, profile)?
                }
                None => InstructionAssembler::assemble(assembly_request)?,
            }
        }
    };
    let turn_instruction_snapshot = instruction_snapshot.clone();
    let reasoning = reasoning_effort.as_ref().map(|effort| ReasoningConfig {
        effort: Some(effort.as_str().to_string()),
        summary: Some(if effort.is_none() {
            ReasoningSummary::Disabled
        } else {
            ReasoningSummary::Enabled
        }),
    });

    let mut provider_prompt_tokens_for_compaction = None;
    let mut last_compacted_state = None;
    let mut iteration = 0_u32;
    let mut terminal_checkpointed = false;
    persist_mailbox_checkpoint_if_needed(&options, session).await?;
    loop {
        if drain_mailbox_inputs(&options, session, recorder, &turn_id).await? {
            safe_message_count = session.len();
            session_message_count = safe_message_count;
        }
        if working_set.sync_session(session)? {
            persist_checkpoint(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        if is_cancelled(&options) {
            session.truncate_messages(safe_message_count);
            return Ok(interrupted_turn_result(
                recorder,
                &turn_id,
                last_content,
                last_reasoning_content,
                last_model,
                total_usage,
                safe_message_count,
                cancellation_reason(),
            ));
        }
        if let Err(limit) = budget_tracker.check_wall_clock() {
            budget_limit = Some(limit);
            break;
        }

        let iteration_tools = tool_schemas.clone();
        let iteration_snapshot = turn_instruction_snapshot.clone();
        let instruction_bundle = iteration_snapshot.to_bundle();
        let mut assembled_context = ContextAssembler::assemble(
            &instruction_bundle.instructions,
            &instruction_bundle.prelude_messages,
            session.items(),
        )?;
        let model_capabilities = provider.effective_model_capabilities(&model);
        let parallel_tool_calls = should_request_parallel_tool_calls(model_capabilities, &options);

        let compaction_trigger = provider_prompt_tokens_for_compaction.take().map_or(
            crate::context_compaction::CompactionTrigger::EstimatedTokens,
            |prompt_tokens| {
                crate::context_compaction::CompactionTrigger::ProviderPromptTokens(prompt_tokens)
            },
        );
        let current_compaction_state = (session.revision(), session.len());
        if last_compacted_state != Some(current_compaction_state) {
            let compaction_result = maybe_compact_session(
                session,
                ContextCompactionRequest {
                    provider: provider.as_ref(),
                    model: &model,
                    config: &core.context_compaction,
                    request_instructions: &assembled_context.instructions,
                    request_messages: &assembled_context.prelude_messages,
                    tools: &iteration_tools,
                    parallel_tool_calls,
                    reasoning: reasoning.clone(),
                    prompt_cache_key: session.prompt_cache_key().map(ToString::to_string),
                    trigger: compaction_trigger,
                    phase: if iteration == 0 {
                        ContextCompactionPhase::PreTurn
                    } else {
                        ContextCompactionPhase::MidTurn
                    },
                    event_tx: recorder.sender().clone(),
                    progress: Some(&mut progress),
                },
            )
            .await;
            match compaction_result {
                Ok(CompactionOutcome::Skipped) => {}
                Ok(CompactionOutcome::Compacted { usage, snapshot }) => {
                    last_compacted_state = Some((session.revision(), session.len()));
                    safe_message_count = session.len();
                    session_message_count = safe_message_count;
                    if let Some(usage) = usage {
                        total_usage.prompt_tokens += usage.prompt_tokens;
                        total_usage.completion_tokens += usage.completion_tokens;
                        total_usage.cached_prompt_tokens += usage.cached_prompt_tokens;
                        total_usage.reasoning_tokens += usage.reasoning_tokens;
                        let usage_snapshot = token_usage_snapshot(&usage);
                        let model_info = provider.model_info(&model);
                        recorder.broadcast(AgentEvent::AgentRuntimeUpdated {
                            delta: agent_runtime_delta(
                                format!("{turn_id}-compact-{iteration}"),
                                identity_for_subagent(active_subagent.as_ref()),
                                &model_info,
                                usage_snapshot,
                                unix_seconds(),
                            ),
                        });
                    }
                    context_compactions.push(snapshot);
                    working_set.sync_session(session)?;
                    assembled_context = ContextAssembler::assemble(
                        &instruction_bundle.instructions,
                        &instruction_bundle.prelude_messages,
                        session.items(),
                    )?;
                    persist_checkpoint(
                        &options,
                        session,
                        crate::TurnCheckpointReason::ContextCompacted,
                    )
                    .await?;
                }
                Err(error) => {
                    let (error, severity, failure) =
                        normalize_provider_error(active_subagent.as_ref(), error);
                    return Ok(failed_turn_result(
                        recorder,
                        &turn_id,
                        last_content,
                        last_reasoning_content,
                        last_model,
                        total_usage,
                        session.len(),
                        error,
                        severity,
                        failure,
                    ));
                }
            }
        }
        if let Err(limit) = budget_tracker.check_wall_clock() {
            budget_limit = Some(limit);
            break;
        }
        budget_tracker.record_model_step();
        persist_checkpoint(
            &options,
            session,
            crate::TurnCheckpointReason::BeforeInference,
        )
        .await?;
        let history_items = materialize_context_items(
            &assembled_context.history,
            &request.materialized_attachments,
        )?;
        let mut input = assembled_context
            .prelude_messages
            .iter()
            .cloned()
            .map(pl_protocol::ModelContextItem::from)
            .collect::<Vec<_>>();
        input.extend(history_items.clone());
        if iteration == 0 {
            progress.milestone("上下文已整理，准备调用模型。");
        } else {
            progress.tool_detail("工具结果已写入上下文，准备继续调用模型。");
        }

        let inference_id = format!("{turn_id}-inf-{iteration}");
        let mut inference_item = recorder.inference_item(&turn_id, &inference_id, &model);
        recorder.start_item(inference_item.clone());
        let completion_request = CompletionRequest::builder(model.clone())
            .instructions(assembled_context.instructions.clone())
            .input(input.clone())
            .tools(iteration_tools)
            .parallel_tool_calls(parallel_tool_calls)
            .store(Some(false))
            .prompt_cache_key(session.prompt_cache_key().map(ToString::to_string))
            .reasoning(reasoning.clone())
            .trace(Some(pl_model::CompletionTraceContext {
                session_id: recorder.session_id().to_string(),
                turn_id: turn_id.clone(),
                inference_id: inference_id.clone(),
                plan_mode: tool_schemas
                    .iter()
                    .any(|schema| schema.name() == "plan_exit"),
                trace_sequence_base: recorder.current_sequence(),
            }))
            .transport_session(session.transport_session())
            .build();
        progress.heartbeat("正在等待模型响应。");
        progress.debug(format!("模型 `{model}` 流式请求已发起。"));

        let response_result = match &cancellation_token {
            Some(token) => {
                tokio::select! {
                    result = provider.stream_complete(completion_request, recorder.sender().clone()) => result,
                    _ = token.cancelled() => {
                        session.truncate_messages(safe_message_count);
                        return Ok(interrupted_turn_result(
                            recorder,
                            &turn_id,
                            last_content,
                            last_reasoning_content,
                            last_model,
                            total_usage,
                            safe_message_count,
                            cancellation_reason(),
                        ));
                    }
                }
            }
            None => {
                provider
                    .stream_complete(completion_request, recorder.sender().clone())
                    .await
            }
        };
        let response = match response_result {
            Ok(response) => response,
            Err(_) if is_cancelled(&options) => {
                session.truncate_messages(safe_message_count);
                return Ok(interrupted_turn_result(
                    recorder,
                    &turn_id,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    safe_message_count,
                    cancellation_reason(),
                ));
            }
            Err(error) => {
                let (error, severity, failure) =
                    normalize_provider_error(active_subagent.as_ref(), error);
                return Ok(failed_turn_result(
                    recorder,
                    &turn_id,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.len(),
                    error,
                    severity,
                    failure,
                ));
            }
        };
        if let Err(limit) = budget_tracker.check_wall_clock() {
            budget_limit = Some(limit);
            break;
        }

        recorder.record_events(response.trace_events.clone());
        if recorder.current_sequence() < response.next_sequence {
            recorder.advance_sequence(response.next_sequence);
        }
        let actual_model = if response.model.is_empty() {
            model.clone()
        } else {
            response.model.clone()
        };
        if let Some(inference) = &mut inference_item.inference {
            inference.model = actual_model.clone();
        }
        let usage_snapshot = token_usage_snapshot(&response.usage);
        recorder.complete_inference_item(inference_item, usage_snapshot.clone());
        let model_info = provider.model_info(&actual_model);
        let response_prompt_tokens = response.usage.prompt_tokens;
        let response_total_tokens = response
            .usage
            .total_tokens
            .max(response.usage.prompt_tokens + response.usage.completion_tokens);
        last_context_tokens = Some(response_total_tokens);
        let response_reached_auto_compact_limit = model_info
            .resolved_auto_compact_limit()
            .is_some_and(|limit| response_prompt_tokens >= limit || response_total_tokens >= limit);
        recorder.broadcast(AgentEvent::AgentRuntimeUpdated {
            delta: agent_runtime_delta(
                inference_id.clone(),
                identity_for_subagent(active_subagent.as_ref()),
                &model_info,
                usage_snapshot,
                unix_seconds(),
            ),
        });

        let content = response.content.unwrap_or_default();
        let reasoning_content = response.reasoning_content.clone();
        let tool_calls = response.tool_calls;

        total_usage.prompt_tokens += response.usage.prompt_tokens;
        total_usage.completion_tokens += response.usage.completion_tokens;
        total_usage.cached_prompt_tokens += response.usage.cached_prompt_tokens;
        total_usage.reasoning_tokens += response.usage.reasoning_tokens;

        last_model = actual_model;

        if tool_calls.is_empty() {
            progress.milestone("模型已完成正文生成。");
            if looks_like_unexecuted_tool_call_text(&content) {
                return Ok(failed_turn_result(
                    recorder,
                    &turn_id,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.len(),
                    "模型返回了未执行的工具调用文本，未产生可执行 tool call。".to_string(),
                    ErrorSeverity::Recoverable,
                    pl_protocol::TurnFailure::permanent(
                        pl_protocol::TurnFailureCategory::Validation,
                        "模型返回了未执行的工具调用文本，未产生可执行 tool call。",
                    ),
                ));
            }
            session.push_assistant_response(content.clone(), reasoning_content.clone());
            last_content = content;
            last_reasoning_content = reasoning_content;
            session_message_count = session.len();
            safe_message_count = session_message_count;
            if finish_mailbox_window(&options, session, recorder, &turn_id).await? {
                safe_message_count = session.len();
                session_message_count = safe_message_count;
                iteration = iteration.saturating_add(1);
                continue;
            }
            terminal_checkpointed = true;
            break;
        }

        session.push_assistant_tool_calls(
            if content.is_empty() {
                None
            } else {
                Some(content.clone())
            },
            tool_calls.clone(),
            reasoning_content.clone(),
        );
        if !content.is_empty() {
            last_content = content;
        }
        if reasoning_content.is_some() {
            last_reasoning_content = reasoning_content;
        }
        if response_reached_auto_compact_limit {
            provider_prompt_tokens_for_compaction = Some(response_total_tokens);
        }
        let count = tool_calls.len();
        progress.tool_detail(format!("模型请求调用 {count} 个工具。"));

        let tool_results = match execute_tool_calls(
            &tool_calls,
            &mut budget_tracker,
            recorder,
            ToolExecutionContext {
                core,
                options: &options,
                session_id: &turn_id,
                workspace_root: &workspace_root,
                workspace_instructions: workspace_instructions.clone(),
                active_subagent: active_subagent.clone(),
                instruction_snapshot: Some(instruction_snapshot.clone()),
                parent_session: Arc::new(AgentSession::from_items(materialize_context_items(
                    session.items(),
                    &request.materialized_attachments,
                )?)),
                working_set: working_set.clone(),
                tool_cache: tool_cache.clone(),
            },
        )
        .await
        {
            Ok(tool_results) => tool_results,
            Err(ToolExecutionError::Fatal(error))
            | Err(ToolExecutionError::RespondToModel(error)) => {
                session.truncate_messages(safe_message_count);
                return Ok(failed_turn_result_with_abort_reason(
                    recorder,
                    &turn_id,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    safe_message_count,
                    error,
                    ErrorSeverity::Recoverable,
                    pl_protocol::TurnFailure::permanent(
                        pl_protocol::TurnFailureCategory::Tool,
                        "tool execution failed",
                    ),
                    crate::turn::TurnAbortReason::ToolError,
                ));
            }
        };
        progress.tool_detail("工具执行完成，准备回写结果。");
        record_plan_exit_items(recorder, &turn_id, &tool_results);
        if working_set.sync_session(session)? {
            persist_checkpoint(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        let should_end_turn = tool_results.iter().any(|tool_result| {
            tool_result
                .runtime_events
                .iter()
                .any(|event| matches!(event, crate::tool::ToolRuntimeEvent::EndTurn))
        });
        let tool_results = tool_results
            .into_iter()
            .map(|tool_result| {
                let receipt = tool_result_receipt(&tool_result);
                (tool_result, receipt)
            })
            .collect::<Vec<_>>();
        // 先补齐全部 canonical tool result，再更新辅助 evidence ledger。这样即使
        // pinned working context 的大小校验或持久化失败，也不会留下只有
        // assistant tool call、没有对应 output 的不可重放 session。
        for (tool_result, receipt) in &tool_results {
            session.push_tool_result_with_receipt(
                tool_result.id.clone(),
                tool_result.call_id.clone(),
                tool_result.name.clone(),
                tool_result.kind,
                tool_result.result.clone(),
                tool_result.arguments.clone(),
                receipt.clone(),
            );
        }
        for (_, receipt) in tool_results {
            working_set.apply(TurnWorkingSetChange::AppendEvidence(receipt))?;
        }
        if working_set.sync_session(session)? {
            persist_checkpoint(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        if is_cancelled(&options) {
            session.truncate_messages(safe_message_count);
            return Ok(interrupted_turn_result(
                recorder,
                &turn_id,
                last_content,
                last_reasoning_content,
                last_model,
                total_usage,
                safe_message_count,
                cancellation_reason(),
            ));
        }

        session_message_count = session.len();
        safe_message_count = session_message_count;
        if should_end_turn {
            if finish_mailbox_window(&options, session, recorder, &turn_id).await? {
                safe_message_count = session.len();
                session_message_count = safe_message_count;
                iteration = iteration.saturating_add(1);
                continue;
            }
            terminal_checkpointed = true;
            break;
        }
        if budget_limit.is_some() {
            break;
        }
        iteration += 1;
    }

    total_usage.total_tokens = total_usage.prompt_tokens + total_usage.completion_tokens;
    if is_cancelled(&options) {
        session.truncate_messages(safe_message_count);
        return Ok(interrupted_turn_result(
            recorder,
            &turn_id,
            last_content,
            last_reasoning_content,
            last_model,
            total_usage,
            safe_message_count,
            cancellation_reason(),
        ));
    }

    if let Some(limit) = budget_limit {
        return Ok(budget_limited_turn_result(
            recorder,
            &turn_id,
            last_content,
            last_reasoning_content,
            last_model,
            total_usage,
            session_message_count,
            limit.kind,
            limit.usage,
            super::turn_result::budget_limit_message(limit.kind, &limit.usage),
        ));
    }

    recorder.ensure_assistant_text_item(&turn_id, &last_content);
    let mut completed_turn_item = recorder.turn_item(&turn_id, TracePartStatus::Completed);
    completed_turn_item.content = last_content.clone();
    completed_turn_item.usage = Some(TokenUsageSnapshot {
        prompt_tokens: total_usage.prompt_tokens,
        completion_tokens: total_usage.completion_tokens,
        cached_prompt_tokens: total_usage.cached_prompt_tokens,
        total_tokens: total_usage.total_tokens,
    });
    recorder.complete_item(completed_turn_item);
    progress.milestone("本轮已完成。");
    if !terminal_checkpointed {
        persist_checkpoint(&options, session, crate::TurnCheckpointReason::Terminal).await?;
    }
    recorder.broadcast(AgentEvent::Done);

    Ok(TurnResult {
        content: last_content,
        reasoning_content: last_reasoning_content,
        model: last_model,
        usage: total_usage,
        last_context_tokens,
        context_compactions,
        session_message_count,
        status: TurnResultStatus::Completed,
        abort_reason: None,
        error: None,
        failure: None,
        budget_limit_kind: None,
        budget_usage: None,
        trace_events: recorder.drain(),
    })
}

async fn persist_checkpoint(
    options: &TurnOptions,
    session: &AgentSession,
    reason: crate::TurnCheckpointReason,
) -> Result<()> {
    let Some(checkpoint) = &options.checkpoint else {
        return Ok(());
    };
    let consumed_mail_ids = match &options.mailbox {
        Some(mailbox) => mailbox.pending_acknowledgements().await,
        None => Vec::new(),
    };
    checkpoint
        .checkpoint_mailbox(session.clone(), reason, consumed_mail_ids.clone())
        .await
        .map_err(|error| pl_protocol::PureError::MemoryError(error.to_string()))?;
    if let Some(mailbox) = &options.mailbox {
        mailbox.acknowledge(&consumed_mail_ids).await;
    }
    Ok(())
}

async fn persist_mailbox_checkpoint_if_needed(
    options: &TurnOptions,
    session: &AgentSession,
) -> Result<()> {
    let Some(mailbox) = &options.mailbox else {
        return Ok(());
    };
    if mailbox.pending_acknowledgements().await.is_empty() {
        return Ok(());
    }
    persist_checkpoint(
        options,
        session,
        crate::TurnCheckpointReason::MailboxInputConsumed,
    )
    .await
}

async fn drain_mailbox_inputs(
    options: &TurnOptions,
    session: &mut AgentSession,
    recorder: &mut TraceRecorder,
    turn_id: &str,
) -> Result<bool> {
    let Some(mailbox) = &options.mailbox else {
        return Ok(false);
    };
    let inputs = mailbox.drain().await;
    if inputs.is_empty() {
        return Ok(false);
    }
    for input in inputs {
        session.push_user_prompt(input.message.clone());
        recorder.user_text_item_with_id(
            turn_id,
            format!("{turn_id}-mail-{}", input.mail_id),
            input.message,
            Vec::new(),
        );
    }
    persist_checkpoint(
        options,
        session,
        crate::TurnCheckpointReason::MailboxInputConsumed,
    )
    .await?;
    Ok(true)
}

async fn finish_mailbox_window(
    options: &TurnOptions,
    session: &mut AgentSession,
    recorder: &mut TraceRecorder,
    turn_id: &str,
) -> Result<bool> {
    if drain_mailbox_inputs(options, session, recorder, turn_id).await? {
        return Ok(true);
    }
    persist_checkpoint(options, session, crate::TurnCheckpointReason::Terminal).await?;
    drain_mailbox_inputs(options, session, recorder, turn_id).await
}

fn tool_result_receipt(result: &super::tool_dispatch::ToolExecutionRecord) -> ToolResultReceipt {
    let artifacts = result
        .runtime_events
        .iter()
        .filter_map(|event| match event {
            crate::tool::ToolRuntimeEvent::OutputArtifacts { artifacts } => {
                Some(artifacts.as_slice())
            }
            crate::tool::ToolRuntimeEvent::SkillActivated { .. }
            | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
            | crate::tool::ToolRuntimeEvent::CacheHit { .. }
            | crate::tool::ToolRuntimeEvent::OutputMetrics { .. }
            | crate::tool::ToolRuntimeEvent::EndTurn => None,
        })
        .flatten()
        .map(compact_artifact_reference)
        .collect::<Vec<_>>();
    let cache_hit = result.runtime_events.iter().find_map(|event| match event {
        crate::tool::ToolRuntimeEvent::CacheHit {
            reused_from_call_id,
            result_hash,
            total_bytes,
        } => Some((reused_from_call_id, result_hash, *total_bytes)),
        crate::tool::ToolRuntimeEvent::SkillActivated { .. }
        | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
        | crate::tool::ToolRuntimeEvent::OutputArtifacts { .. }
        | crate::tool::ToolRuntimeEvent::OutputMetrics { .. }
        | crate::tool::ToolRuntimeEvent::EndTurn => None,
    });
    let metrics = result.runtime_events.iter().find_map(|event| match event {
        crate::tool::ToolRuntimeEvent::OutputMetrics {
            raw_bytes,
            model_visible_bytes,
            artifact_bytes: _,
            result_hash,
        } => Some((*raw_bytes, *model_visible_bytes, result_hash)),
        crate::tool::ToolRuntimeEvent::SkillActivated { .. }
        | crate::tool::ToolRuntimeEvent::ToolResultRevision { .. }
        | crate::tool::ToolRuntimeEvent::OutputArtifacts { .. }
        | crate::tool::ToolRuntimeEvent::CacheHit { .. }
        | crate::tool::ToolRuntimeEvent::EndTurn => None,
    });
    ToolResultReceipt {
        call_id: result.call_id.clone().unwrap_or_else(|| result.id.clone()),
        tool_name: result.name.clone(),
        arguments_hash: serde_json::from_str(&result.arguments).map_or_else(
            |_| canonical_content_hash(result.arguments.as_bytes()),
            |value| crate::canonical_json_hash(&value),
        ),
        result_hash: cache_hit.map_or_else(
            || {
                metrics.map_or_else(
                    || canonical_content_hash(result.result.as_bytes()),
                    |(_, _, hash)| hash.clone(),
                )
            },
            |(_, hash, _)| hash.clone(),
        ),
        total_bytes: cache_hit.map_or_else(
            || metrics.map_or(result.result.len() as u64, |(raw, _, _)| raw),
            |(_, _, bytes)| bytes,
        ),
        visible_bytes: metrics.map_or(result.result.len() as u64, |(_, visible, _)| visible),
        truncated: cache_hit.is_some()
            || metrics.is_some_and(|(raw, visible, _)| raw > visible)
            || result.result.len() >= crate::tool::MAX_MODEL_TOOL_OUTPUT_BYTES,
        artifacts,
        continuation: tool_result_continuation(&result.result),
        reused_from_call_id: cache_hit.map(|(call_id, _, _)| call_id.clone()),
    }
}

fn compact_artifact_reference(artifact: &serde_json::Value) -> serde_json::Value {
    const REFERENCE_FIELDS: [&str; 20] = [
        "artifactId",
        "artifact_id",
        "callId",
        "call_id",
        "contentHash",
        "content_hash",
        "id",
        "kind",
        "mediaType",
        "media_type",
        "mimeType",
        "mime_type",
        "name",
        "path",
        "sha256",
        "size",
        "sizeBytes",
        "size_bytes",
        "stream",
        "uri",
    ];
    let serialized = serde_json::to_vec(artifact).unwrap_or_default();
    let mut reference = serde_json::Map::new();
    if let Some(object) = artifact.as_object() {
        for field in REFERENCE_FIELDS {
            if let Some(value) = object.get(field)
                && matches!(
                    value,
                    serde_json::Value::Null
                        | serde_json::Value::Bool(_)
                        | serde_json::Value::Number(_)
                        | serde_json::Value::String(_)
                )
            {
                reference.insert(field.to_string(), value.clone());
            }
        }
    }
    reference.insert(
        "receiptHash".to_string(),
        serde_json::Value::String(canonical_content_hash(&serialized)),
    );
    reference.insert(
        "receiptBytes".to_string(),
        serde_json::Value::from(serialized.len() as u64),
    );
    serde_json::Value::Object(reference)
}

fn tool_result_continuation(output: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(output).ok()?;
    ["nextStartLine", "nextStartByte", "nextCursor", "nextOffset"]
        .into_iter()
        .find_map(|key| {
            value
                .get(key)
                .filter(|value| !value.is_null())
                .map(|value| serde_json::json!({ "field": key, "value": value }).to_string())
        })
}

#[cfg(test)]
mod receipt_tests {
    use pretty_assertions::assert_eq;

    use super::compact_artifact_reference;

    #[test]
    fn artifact_receipt_keeps_identity_but_not_large_payload() {
        let artifact = serde_json::json!({
            "kind": "webSearch",
            "id": "artifact-1",
            "results": "x".repeat(64 * 1024),
        });

        let reference = compact_artifact_reference(&artifact);

        assert_eq!(reference["kind"], "webSearch");
        assert_eq!(reference["id"], "artifact-1");
        assert_eq!(reference.get("results"), None);
        assert!(reference["receiptBytes"].as_u64().unwrap() > 64 * 1024);
        assert!(
            reference["receiptHash"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
    }
}
