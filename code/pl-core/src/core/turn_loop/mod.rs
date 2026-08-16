use pl_model::{CompletionRequest, ModelProvider};
use pl_protocol::{
    ErrorSeverity, ModelContextItem, ResponsesContextItem, ResponsesContextItemKind, Result,
};
use pl_trace::TracePartStatus;
use std::collections::BTreeSet;
use std::sync::Arc;

mod attachments;
mod checkpoint;
mod completion;
pub(super) mod enabled_tools;
mod inference;
mod orchestration;
mod plan_exit;
mod prompt_cache;
mod tool_results;
mod turn_setup;

use attachments::materialize_context_items;
use enabled_tools::record_enabled_tools;
use plan_exit::record_plan_exit_items;

use crate::context_assembler::{ContextAssembler, TurnContextSnapshot};
use crate::context_compaction::{
    CompactionOutcome, ContextCompactionPhase, ContextCompactionRequest,
    ensure_provider_can_consume_session, maybe_compact_session,
};
use crate::runtime_usage::{InferenceBillingInput, inference_billing_record, token_usage_snapshot};
use crate::session::AgentSession;
use crate::tool::{ClientToolSearchResolution, ToolInventory, orchestrate_tool_inventory};
use crate::trace::TraceRecorder;
use crate::turn::{BudgetLimit, BudgetTracker, TurnOptions, TurnRequest, TurnResult};
use crate::working_set::{TurnWorkingSetChange, TurnWorkingSetHandle};
use crate::{PromptCacheInput, prepare_prompt_context};

use super::TurnEngine;
use super::permission::cancellation_reason;
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::tool_dispatch::{ToolExecutionContext, ToolExecutionError, execute_tool_call_batch};
use super::turn_result::{
    budget_limited_turn_result, failed_turn_result, failed_turn_result_with_abort_reason,
    interrupted_turn_result, is_cancelled, looks_like_unexecuted_tool_call_text,
    normalize_provider_error, should_request_parallel_tool_calls, unix_seconds,
};

pub(super) async fn run_turn_with_trace(
    core: &TurnEngine,
    session: &mut AgentSession,
    request: TurnRequest,
    recorder: &mut TraceRecorder,
    options: TurnOptions,
) -> Result<TurnResult> {
    let provider = core.provider.clone();
    let model = provider.default_model().to_string();
    let model_info = provider.model_info(&model);
    ensure_provider_can_consume_session(model_info.transport.protocol, session)?;
    let effort = core.effort.clone();
    let workspace = core.workspace.clone().unwrap_or_else(|| {
        crate::tool::AgentWorkspace::local(super::turn_result::default_workspace_root())
    });
    let workspace_root = workspace.root().to_path_buf();
    let workspace_instructions = core.workspace_instructions.clone();
    let active_subagent = core.active_subagent.clone();
    let cancellation_token = options.cancellation_token.clone();
    let model_capabilities = provider.effective_model_capabilities(&model);
    let orchestration_options =
        orchestration::options(provider.info(), &model_info, &model_capabilities);
    let lease = core.acquire_tool_lease()?;
    let tool_inventory = orchestrate_tool_inventory(
        lease.entries(),
        options.execution_policy.as_ref(),
        orchestration_options,
    );
    let tool_schemas = tool_inventory.request_schemas().to_vec();
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
    let tool_cache = crate::tool::cache::TurnToolCacheHandle::default();
    record_enabled_tools(recorder, &turn_id, &tool_schemas);
    let turn_item = recorder.turn_item(&turn_id, TracePartStatus::Running);
    recorder.start_item(turn_item.clone());
    let mut progress = ProgressEmitter::new(
        recorder.sender().clone(),
        turn_id.clone(),
        ProgressVerbosity::from_env(),
    );
    progress.milestone("已接收请求，正在准备上下文。");
    let mut last_content = String::new();
    let mut last_reasoning_content = None;
    let mut last_model = model.clone();
    let mut last_context_tokens = None;
    let mut context_compactions = Vec::new();
    let mut total_usage = pl_model::TokenUsage::default();
    let mut safe_message_count = session.len();
    let mut session_message_count = safe_message_count;
    let mut inference_count = 0_u64;

    let prompt_cache_policy = provider.info().effective_prompt_cache_policy(&model_info);
    let instruction_snapshot =
        turn_setup::instruction_snapshot(core, &request, &model_info, &workspace_root)?;
    let turn_instruction_snapshot = instruction_snapshot.clone();
    let reasoning = turn_setup::reasoning(effort.as_ref());

    let mut provider_prompt_tokens_for_compaction = None;
    let mut last_compacted_state = None;
    let mut iteration = 0_u32;
    let mut terminal_checkpointed = false;
    let mut ended_for_interaction = false;
    checkpoint::persist_pending_mail(&options, session).await?;
    let mut turn_context =
        TurnContextSnapshot::capture(session.items(), session.working_context_snapshot());
    loop {
        if checkpoint::drain_mailbox(&options, session, recorder, &turn_id).await? {
            safe_message_count = session.len();
            session_message_count = safe_message_count;
        }
        if working_set.sync_session(session)? {
            checkpoint::persist(
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
        let model_capabilities = provider.effective_model_capabilities(&model);
        let parallel_tool_calls = should_request_parallel_tool_calls(model_capabilities, &options);
        if prepare_prompt_context(
            session,
            PromptCacheInput {
                scope: &options.prompt_scope,
                provider: provider.info(),
                model: &model,
                instructions: &instruction_bundle.instructions,
                prelude_messages: &instruction_bundle.prelude_messages,
                working_context: Some(turn_context.model_context()),
                fixed_prefix_section_hashes: instruction_bundle.prefix_section_hashes.clone(),
                tools: &iteration_tools,
                tool_catalog_hash: tool_inventory.catalog_fingerprint(),
                registry_revision: Some(lease.revision().0),
                tool_choice: "auto",
                parallel_tool_calls,
                reasoning: reasoning.as_ref(),
                output_schema: None,
                service_tier: None,
                compacted: false,
                prompt_cache_policy,
                updated_at: unix_seconds(),
            },
        )?
        .is_some()
        {
            checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
            safe_message_count = session.len();
            session_message_count = safe_message_count;
        }
        prompt_cache::sync(session, &options, prompt_cache_policy)?;
        let mut assembled_context = ContextAssembler::assemble_turn(
            &instruction_bundle.instructions,
            &instruction_bundle.prelude_messages,
            session.items(),
            &turn_context,
        )?;

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
                    working_context_tail: assembled_context.working_context_tail.clone(),
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
                    let compaction_inference = usage.map(|usage| {
                        total_usage.prompt_tokens += usage.prompt_tokens;
                        total_usage.completion_tokens += usage.completion_tokens;
                        total_usage.cached_prompt_tokens += usage.cached_prompt_tokens;
                        total_usage.cache_write_tokens += usage.cache_write_tokens;
                        total_usage.reasoning_tokens += usage.reasoning_tokens;
                        total_usage.total_tokens += usage
                            .total_tokens
                            .max(usage.prompt_tokens.saturating_add(usage.completion_tokens));
                        inference_count = inference_count.saturating_add(1);
                        let model_info = provider.model_info(&model);
                        let inference_id = format!("{turn_id}-compact-{iteration}");
                        let recorded_at = unix_seconds();
                        let billing = inference_billing_record(InferenceBillingInput {
                            inference_id,
                            provider: &provider.info().name,
                            model: &model,
                            usage: &usage,
                            model_info: &model_info,
                            prompt_cache_policy,
                            prompt: prompt_cache::current(session, &options.prompt_scope),
                            orchestration: Default::default(),
                            recorded_at,
                        });
                        inference::from_billing(active_subagent.as_ref(), billing)
                    });
                    context_compactions.push(snapshot);
                    working_set.sync_session(session)?;
                    prepare_prompt_context(
                        session,
                        PromptCacheInput {
                            scope: &options.prompt_scope,
                            provider: provider.info(),
                            model: &model,
                            instructions: &instruction_bundle.instructions,
                            prelude_messages: &instruction_bundle.prelude_messages,
                            working_context: Some(turn_context.model_context()),
                            fixed_prefix_section_hashes: instruction_bundle
                                .prefix_section_hashes
                                .clone(),
                            tools: &iteration_tools,
                            tool_catalog_hash: tool_inventory.catalog_fingerprint(),
                            registry_revision: Some(lease.revision().0),
                            tool_choice: "auto",
                            parallel_tool_calls,
                            reasoning: reasoning.as_ref(),
                            output_schema: None,
                            service_tier: None,
                            compacted: true,
                            prompt_cache_policy,
                            updated_at: unix_seconds(),
                        },
                    )?;
                    prompt_cache::sync(session, &options, prompt_cache_policy)?;
                    turn_context.rebase(session.items());
                    assembled_context = ContextAssembler::assemble_turn(
                        &instruction_bundle.instructions,
                        &instruction_bundle.prelude_messages,
                        session.items(),
                        &turn_context,
                    )?;
                    if let Some(inference) = compaction_inference {
                        inference::record(&options, session, recorder, inference).await?;
                    } else {
                        checkpoint::persist(
                            &options,
                            session,
                            crate::TurnCheckpointReason::ContextCompacted,
                        )
                        .await?;
                    }
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
        checkpoint::persist(
            &options,
            session,
            crate::TurnCheckpointReason::BeforeInference,
        )
        .await?;
        safe_message_count = session.len();
        session_message_count = safe_message_count;
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
        let tool_schema_estimated_tokens =
            crate::tool::estimate_tool_schema_tokens(&iteration_tools);
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
        let mut response = match response_result {
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
        response.orchestration.tool_schema_estimated_tokens = tool_schema_estimated_tokens;
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
        let recorded_at = unix_seconds();
        let mut billing = inference_billing_record(InferenceBillingInput {
            inference_id,
            provider: &provider.info().name,
            model: &actual_model,
            usage: &response.usage,
            model_info: &model_info,
            prompt_cache_policy,
            prompt: prompt_cache::current(session, &options.prompt_scope),
            orchestration: response.orchestration.clone(),
            recorded_at,
        });
        let response_prompt_tokens = response.usage.prompt_tokens;
        let response_total_tokens = response
            .usage
            .total_tokens
            .max(response.usage.prompt_tokens + response.usage.completion_tokens);
        last_context_tokens = Some(response_total_tokens);
        let response_reached_auto_compact_limit = model_info
            .resolved_auto_compact_limit()
            .is_some_and(|limit| response_prompt_tokens >= limit || response_total_tokens >= limit);
        let content = response.content.unwrap_or_default();
        let reasoning_content = response.reasoning_content.clone();
        let mut tool_calls = response.tool_calls;
        let client_search = collect_client_tool_search(
            &mut tool_calls,
            &response.responses_context_items,
            &tool_inventory,
            session,
        )?;
        session.push_responses_context_items(response.responses_context_items);
        if !client_search.resolution.outputs.is_empty() {
            if !tool_calls.is_empty() {
                return Err(pl_protocol::PureError::LlmError(
                    "provider response protocol error: client tool_search cannot be mixed with ordinary tool calls"
                        .to_string(),
                ));
            }
            apply_client_tool_search(
                session,
                &client_search,
                &mut budget_tracker,
                &mut billing.orchestration,
            );
            if !content.is_empty() {
                session.push_assistant_response(content.clone(), reasoning_content.clone());
                last_content = content;
                last_reasoning_content = reasoning_content;
            }
            if response_reached_auto_compact_limit {
                provider_prompt_tokens_for_compaction = Some(response_total_tokens);
            }
            let loaded = client_search.resolution.loaded_tool_count;
            progress.tool_detail(format!(
                "工具搜索已加载 {loaded} 个候选 schema，准备继续调用模型。"
            ));
            session_message_count = session.len();
            safe_message_count = session_message_count;
            inference::record(
                &options,
                session,
                recorder,
                inference::from_billing(active_subagent.as_ref(), billing),
            )
            .await?;
            iteration = iteration.saturating_add(1);
            continue;
        }

        total_usage.prompt_tokens += response.usage.prompt_tokens;
        total_usage.completion_tokens += response.usage.completion_tokens;
        total_usage.cached_prompt_tokens += response.usage.cached_prompt_tokens;
        total_usage.cache_write_tokens += response.usage.cache_write_tokens;
        total_usage.reasoning_tokens += response.usage.reasoning_tokens;
        total_usage.total_tokens += response.usage.total_tokens.max(
            response
                .usage
                .prompt_tokens
                .saturating_add(response.usage.completion_tokens),
        );
        inference_count = inference_count.saturating_add(1);

        last_model = actual_model;

        if tool_calls.is_empty() {
            progress.milestone("模型已完成正文生成。");
            if looks_like_unexecuted_tool_call_text(&content) {
                inference::record(
                    &options,
                    session,
                    recorder,
                    inference::from_billing(active_subagent.as_ref(), billing),
                )
                .await?;
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
            inference::record(
                &options,
                session,
                recorder,
                inference::from_billing(active_subagent.as_ref(), billing),
            )
            .await?;
            if checkpoint::finish_mailbox_window(&options, session, recorder, &turn_id).await? {
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

        let tool_batch = match execute_tool_call_batch(
            &tool_calls,
            &mut budget_tracker,
            recorder,
            ToolExecutionContext {
                core,
                lease: lease.clone(),
                options: &options,
                session_id: &turn_id,
                workspace: workspace.clone(),
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
            Ok(tool_batch) => tool_batch,
            Err(ToolExecutionError::Fatal(error)) => {
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
                    ErrorSeverity::Fatal,
                    pl_protocol::TurnFailure::permanent(
                        pl_protocol::TurnFailureCategory::Tool,
                        "fatal tool runtime failure",
                    ),
                    crate::turn::TurnAbortReason::ToolError,
                ));
            }
            Err(ToolExecutionError::RespondToModel(error)) => {
                session.truncate_messages(safe_message_count);
                inference::record(
                    &options,
                    session,
                    recorder,
                    inference::from_billing(active_subagent.as_ref(), billing),
                )
                .await?;
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
                        pl_protocol::TurnFailureCategory::Validation,
                        "tool execution failed and can be corrected",
                    ),
                    crate::turn::TurnAbortReason::ToolError,
                ));
            }
        };
        billing.orchestration.merge(&tool_batch.orchestration);
        let mut tool_results = tool_batch.records;
        progress.tool_detail("工具执行完成，准备回写结果。");
        record_plan_exit_items(recorder, &turn_id, &tool_results);
        let requested_interaction = tool_results.iter().any(|tool_result| {
            tool_result.runtime_events.iter().any(|event| {
                matches!(
                    event,
                    crate::tool::ToolRuntimeEvent::InteractionRequested { .. }
                )
            })
        });
        let should_end_turn = tool_results.iter().any(|tool_result| {
            tool_result
                .runtime_events
                .iter()
                .any(|event| matches!(event, crate::tool::ToolRuntimeEvent::EndTurn))
        });
        let remaining_context_tokens = model_info
            .resolved_auto_compact_limit()
            .or_else(|| model_info.resolved_context_window())
            .map(|limit| limit.saturating_sub(response_total_tokens));
        tool_results::apply_batch_budget(&mut tool_results, remaining_context_tokens);
        tool_results::normalize_programmatic_results(&mut tool_results, &tool_calls);
        billing.orchestration.tool_result_estimated_tokens =
            crate::tool::estimate_tool_result_tokens(
                tool_results
                    .iter()
                    .map(|tool_result| tool_result.result.as_str()),
            );
        let tool_results = tool_results
            .into_iter()
            .map(|tool_result| {
                let receipt = tool_results::receipt(&tool_result);
                (tool_result, receipt)
            })
            .collect::<Vec<_>>();
        // 先补齐全部 canonical tool result，再更新辅助 evidence ledger。这样即使
        // pinned working context 的大小校验或持久化失败，也不会留下只有
        // assistant tool call、没有对应 output 的不可重放 session。
        for ((tool_result, receipt), tool_call) in tool_results.iter().zip(&tool_calls) {
            session.push_tool_result_with_receipt_and_caller(
                tool_result.id.clone(),
                Some(tool_result.call_id.clone()),
                tool_result.name.clone(),
                tool_result.kind,
                tool_result.result.clone(),
                tool_result.arguments.clone(),
                receipt.clone(),
                tool_call.caller.clone(),
            );
        }
        inference::record(
            &options,
            session,
            recorder,
            inference::from_billing(active_subagent.as_ref(), billing),
        )
        .await?;
        for (_, receipt) in tool_results {
            working_set.apply(TurnWorkingSetChange::AppendEvidence(receipt))?;
        }
        if working_set.sync_session(session)? {
            checkpoint::persist(
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
            if checkpoint::finish_mailbox_window(&options, session, recorder, &turn_id).await? {
                safe_message_count = session.len();
                session_message_count = safe_message_count;
                iteration = iteration.saturating_add(1);
                continue;
            }
            ended_for_interaction = requested_interaction;
            terminal_checkpointed = true;
            break;
        }
        if budget_limit.is_some() {
            break;
        }
        iteration += 1;
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

    progress.milestone("本轮已完成。");
    if !terminal_checkpointed {
        checkpoint::persist(&options, session, crate::TurnCheckpointReason::Terminal).await?;
    }
    Ok(completion::finish(
        recorder,
        &turn_id,
        completion::CompletedTurn {
            content: last_content,
            reasoning_content: last_reasoning_content,
            model: last_model,
            usage: total_usage,
            last_context_tokens,
            context_compactions,
            session_message_count,
            ended_for_interaction,
            inference_count,
        },
    ))
}

/// 本轮 provider 响应中的 client tool search 调用集合。
struct ClientToolSearchBatch {
    /// 由 function call 合成的有序 `tool_search_call` 上下文项。
    call_items: Vec<ResponsesContextItem>,
    resolution: ClientToolSearchResolution,
}

/// 汇总客户端 tool search 调用。
///
/// `tool_search` 以普通 function 工具形式发送，模型调用先于普通 dispatch 被拦截，
/// 在冻结 catalog 上检索并产出配对的 `tool_search_output`；provider 原生返回的
/// `tool_search_call`（execution=client）项同样处理。session 中已有配对 output 的
/// call_id 不重复解析，保证 HTTP/WS/恢复回放幂等。
fn collect_client_tool_search(
    tool_calls: &mut Vec<pl_model::ToolCall>,
    response_items: &[ResponsesContextItem],
    inventory: &ToolInventory,
    session: &AgentSession,
) -> Result<ClientToolSearchBatch> {
    let paired = paired_tool_search_call_ids(session);
    let mut calls = Vec::new();
    let mut call_items = Vec::new();
    for item in response_items {
        if item.kind != ResponsesContextItemKind::ToolSearchCall {
            continue;
        }
        if item
            .value
            .get("execution")
            .and_then(serde_json::Value::as_str)
            != Some("client")
        {
            continue;
        }
        let call_id = item
            .value
            .get("call_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !call_id.is_empty() && paired.contains(&call_id) {
            continue;
        }
        calls.push(item.clone());
    }
    if inventory.catalog().is_some() {
        tool_calls.retain(|tool_call| {
            if tool_call.name != "tool_search" {
                return true;
            }
            let call_id = tool_call.call_id.clone();
            if call_id.is_empty() || paired.contains(&call_id) {
                return false;
            }
            let arguments = match &tool_call.payload {
                pl_model::ToolCallPayload::Function { arguments } => arguments.clone(),
                pl_model::ToolCallPayload::Custom { input } => {
                    serde_json::json!({ "input": input })
                }
            };
            let call_item = ResponsesContextItem {
                kind: ResponsesContextItemKind::ToolSearchCall,
                value: serde_json::json!({
                    "type": "tool_search_call",
                    "call_id": call_id,
                    "execution": "client",
                    "arguments": arguments,
                }),
            };
            calls.push(call_item.clone());
            call_items.push(call_item);
            false
        });
    }
    let resolution = if calls.is_empty() {
        ClientToolSearchResolution::default()
    } else {
        inventory.resolve_client_search_calls(&calls)?
    };
    Ok(ClientToolSearchBatch {
        call_items,
        resolution,
    })
}

/// session 中已存在 `tool_search_output` 的 call_id 集合。
fn paired_tool_search_call_ids(session: &AgentSession) -> BTreeSet<String> {
    session
        .items()
        .iter()
        .filter_map(|item| match item {
            ModelContextItem::Responses {
                item:
                    ResponsesContextItem {
                        kind: ResponsesContextItemKind::ToolSearchOutput,
                        value,
                    },
            } => value
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            _ => None,
        })
        .collect()
}

/// 把 client tool search 的 call/output 项写入 canonical context 并记录指标。
fn apply_client_tool_search(
    session: &mut AgentSession,
    batch: &ClientToolSearchBatch,
    budget_tracker: &mut BudgetTracker,
    orchestration: &mut pl_protocol::InferenceOrchestrationMetrics,
) {
    let output_count = batch.resolution.outputs.len();
    for _ in 0..output_count {
        budget_tracker.record_tool_call("tool_search");
    }
    let output_texts = batch
        .resolution
        .outputs
        .iter()
        .map(|item| item.value.to_string())
        .collect::<Vec<_>>();
    let estimated_tokens =
        crate::tool::estimate_tool_result_tokens(output_texts.iter().map(String::as_str));
    session.push_responses_context_items(batch.call_items.clone());
    session.push_responses_context_items(batch.resolution.outputs.clone());
    orchestration.tool_search_calls = orchestration
        .tool_search_calls
        .saturating_add(output_count as u64);
    orchestration.tool_search_loaded_tools = orchestration
        .tool_search_loaded_tools
        .saturating_add(batch.resolution.loaded_tool_count);
    orchestration.tool_result_estimated_tokens = estimated_tokens;
}
