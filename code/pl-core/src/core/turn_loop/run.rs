//! Turn 主循环：模型 step 状态机的实际执行体。
//!
//! 单一职责说明：整个文件是 `run_turn_with_trace` 一个编排函数——按模型 step
//! 循环推进（工具计划冻结 → 压缩 → 推理 → 工具执行 → 回写），循环各阶段的
//! 支撑逻辑已下沉到兄弟子模块，循环本身保持显式控制流以保证可审计性。

use pl_model::completion::CompletionRequest;
use pl_protocol::TurnCompletion;
use pl_protocol::{ErrorSeverity, Result};
use std::sync::Arc;

use super::attachments::prepare_context_items;
use super::compaction::CompactionStep;
use super::enabled_tools::record_enabled_tools;

use crate::context_assembler::{ContextAssembler, TurnContextSnapshot};
use crate::context_compaction::ensure_provider_can_consume_session;
use crate::runtime_usage::{InferenceBillingInput, inference_billing_record};
use crate::session::AgentSession;
use crate::tool::ModelStepToolContext;
use crate::trace::TraceRecorder;
use crate::turn::{BudgetLimit, BudgetTracker, TurnOptions, TurnRequest, TurnResult};
use crate::working_set::TurnWorkingSetChange;
use crate::{PromptCacheInput, prepare_prompt_context};

use super::super::engine::TurnEngine;
use super::super::permission::cancellation_reason;
use super::super::progress::{ProgressEmitter, ProgressVerbosity};
use super::super::tool_dispatch::{
    ToolExecutionContext, ToolExecutionError, execute_tool_call_batch,
};
use super::super::turn_result::{
    budget_limited_turn_result, failed_turn_result, failed_turn_result_with_abort_reason,
    interrupted_turn_result, is_cancelled, looks_like_unexecuted_tool_call_text,
    normalize_provider_error, should_request_parallel_tool_calls,
};
use crate::time::unix_seconds;

pub(in crate::core) async fn run_turn_with_trace(
    core: &TurnEngine,
    session: &mut AgentSession,
    request: TurnRequest,
    recorder: &mut TraceRecorder,
    options: TurnOptions,
) -> Result<TurnResult> {
    let mut billing = pl_protocol::TurnBillingRecord::new();
    let mut result = run_steps(core, session, request, recorder, options, &mut billing).await?;
    result.billing = billing;
    Ok(result)
}

async fn run_steps(
    core: &TurnEngine,
    session: &mut AgentSession,
    request: TurnRequest,
    recorder: &mut TraceRecorder,
    options: TurnOptions,
    turn_billing: &mut pl_protocol::TurnBillingRecord,
) -> Result<TurnResult> {
    let runtime = core.runtime.clone();
    let model_info = runtime.model().clone();
    let model = model_info.slug.clone();
    ensure_provider_can_consume_session(model_info.binding.transport.protocol, session)?;
    let effort = core.effort.clone();
    let workspace = core.workspace.clone().unwrap_or_else(|| {
        crate::tool::AgentWorkspace::local(super::super::turn_result::default_workspace_root())
    });
    let workspace_root = workspace.root().to_path_buf();
    let active_subagent = core.active_subagent.clone();
    let cancellation_token = options.cancellation_token.clone();
    let mut budget_tracker = BudgetTracker::new(request.budget);
    let mut materialized_attachments = request.materialized_attachments.clone();
    let mut budget_limit: Option<BudgetLimit> = None;

    let turn_id = request
        .turn_id
        .clone()
        .unwrap_or_else(super::super::generate_turn_id);
    recorder.user_text_item_with_attachments(
        &turn_id,
        request.prompt.clone(),
        request.trace_attachments.clone(),
    );
    for activation in &request.skill_activations {
        recorder.record_trace_only(pl_trace::TraceEventKind::SkillActivated {
            activation: activation.clone(),
        });
    }
    if let Some(prompt_cache_key) = options.prompt_cache_key.clone() {
        session.set_prompt_cache_key(prompt_cache_key);
    }
    session.push_user_content_with_presentation(
        request.user_content.clone(),
        request.user_presentation,
    );
    core.tool_session_runtime.begin_turn(session)?;
    let working_set = core.tool_session_runtime.working_set();
    let tool_cache = crate::tool::cache::TurnToolCacheHandle::default();
    let turn_item = recorder.running_turn_item(&turn_id);
    recorder.start_item(turn_item.clone());
    let mut progress = ProgressEmitter::new(turn_id.clone(), ProgressVerbosity::from_env());
    progress.milestone(recorder, "已接收请求，正在准备上下文。");
    let mut last_content = String::new();
    let mut last_reasoning_content = None;
    let mut last_model = model.clone();
    let mut last_context_tokens = None;
    let mut context_compactions = Vec::new();
    let mut total_usage = pl_protocol::InferenceTokenUsage::default();
    let mut safe_message_count = session.len();
    let mut session_message_count = safe_message_count;
    let mut inference_count = 0_u64;

    let prompt_cache_policy = runtime
        .endpoint()
        .effective_prompt_cache_policy(&model_info);
    let instruction_snapshot =
        super::turn_setup::instruction_snapshot(core, &request, &model_info, &workspace_root)?;
    let turn_instruction_snapshot = instruction_snapshot.clone();
    let reasoning = super::turn_setup::reasoning(effort.as_ref());

    let mut provider_prompt_tokens_for_compaction = None;
    let mut last_compacted_state = None;
    let mut iteration = 0_u32;
    let mut terminal_checkpointed = false;
    let mut completion = TurnCompletion::Normal;
    super::checkpoint::persist_pending_mail(&options, session).await?;
    let mut turn_context =
        TurnContextSnapshot::capture(session.items(), working_set.model_context_snapshot(session));
    loop {
        if super::checkpoint::drain_mailbox(&options, session, recorder, &turn_id).await? {
            safe_message_count = session.len();
            session_message_count = safe_message_count;
        }
        options.apply_budget_refresh(&mut budget_tracker);
        if working_set.sync_session(session)? {
            super::checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        turn_context.refresh_working_context(working_set.model_context_snapshot(session));
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

        if let Some(hook) = &core.before_model_step {
            hook.refresh(ModelStepToolContext {
                agent_tools: core.agent_tools.clone(),
                session_id: recorder.session_id().to_string(),
                turn_id: turn_id.clone(),
                step: iteration,
            })
            .await?;
        }
        let unfiltered_tool_plan = core.acquire_tool_plan_for(session.tool_discovery());
        let normalized_discovery =
            unfiltered_tool_plan.normalized_discovery_state(session.tool_discovery());
        if session.replace_tool_discovery(normalized_discovery) {
            super::checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        let tool_plan = unfiltered_tool_plan.allowed_by(options.execution_policy.as_ref());
        let iteration_tools = tool_plan.specs().to_vec();
        record_enabled_tools(recorder, &turn_id, iteration, &tool_plan);
        let iteration_snapshot = turn_instruction_snapshot
            .clone()
            .with_tool_group_instructions(tool_plan.developer_instructions());
        let instruction_bundle = iteration_snapshot.to_bundle();
        let model_capabilities = runtime.effective_model_capabilities();
        let parallel_tool_calls = should_request_parallel_tool_calls(model_capabilities, &options);
        if prepare_prompt_context(
            session,
            PromptCacheInput {
                scope: &options.prompt_scope,
                provider: runtime.endpoint(),
                model: &model,
                instructions: &instruction_bundle.instructions,
                prelude_messages: &instruction_bundle.prelude_messages,
                working_context: Some(turn_context.model_context()),
                fixed_prefix_section_hashes: instruction_bundle.prefix_section_hashes.clone(),
                tools: &iteration_tools,
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
            super::checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
            safe_message_count = session.len();
            session_message_count = safe_message_count;
        }
        super::prompt_cache::sync(session, &options, prompt_cache_policy)?;
        let mut assembled_context = ContextAssembler::assemble_turn(
            &instruction_bundle.instructions,
            &instruction_bundle.prelude_messages,
            session.items(),
            &turn_context,
        )?;

        let compaction_step = CompactionStep {
            turn_billing,
            session,
            runtime: &runtime,
            config: &core.context_compaction,
            options: &options,
            recorder,
            progress: &mut progress,
            turn_id: &turn_id,
            model: &model,
            active_subagent: active_subagent.as_ref(),
            instruction_bundle: &instruction_bundle,
            assembled_context: &mut assembled_context,
            turn_context: &mut turn_context,
            working_set: &working_set,
            iteration_tools: &iteration_tools,
            parallel_tool_calls,
            reasoning: reasoning.as_ref(),
            prompt_cache_policy,
            iteration,
            last_content: &last_content,
            last_reasoning_content: &last_reasoning_content,
            last_model: &last_model,
            total_usage: &mut total_usage,
            inference_count: &mut inference_count,
            context_compactions: &mut context_compactions,
            safe_message_count: &mut safe_message_count,
            session_message_count: &mut session_message_count,
            last_compacted_state: &mut last_compacted_state,
            provider_prompt_tokens_for_compaction: &mut provider_prompt_tokens_for_compaction,
        };
        if let Some(result) = super::compaction::run(compaction_step).await? {
            return Ok(result);
        }
        budget_tracker.record_model_step();
        super::checkpoint::persist(
            &options,
            session,
            crate::TurnCheckpointReason::BeforeInference,
        )
        .await?;
        safe_message_count = session.len();
        session_message_count = safe_message_count;
        let (history_items, prepared_content) = prepare_context_items(
            &assembled_context.history,
            &mut materialized_attachments,
            core.attachment_runtime.as_ref(),
        )
        .await?;
        let mut input = assembled_context
            .prelude_messages
            .iter()
            .cloned()
            .map(pl_protocol::ModelContextItem::from)
            .collect::<Vec<_>>();
        input.extend(history_items.clone());
        if iteration == 0 {
            progress.milestone(recorder, "上下文已整理，准备调用模型。");
        } else {
            progress.tool_detail(recorder, "工具结果已写入上下文，准备继续调用模型。");
        }

        let inference_id = format!("{turn_id}-inf-{iteration}");
        let tool_schema_estimated_tokens =
            crate::tool::estimate_tool_schema_tokens(&iteration_tools);
        let mut inference_item = recorder.inference_item(&turn_id, &inference_id, &model);
        recorder.start_item(inference_item.clone());
        let completion_request = CompletionRequest::builder()
            .instructions(assembled_context.instructions.clone())
            .input(input.clone())
            .prepared_content(prepared_content)
            .tools(iteration_tools)
            .parallel_tool_calls(parallel_tool_calls)
            .reasoning(reasoning.clone())
            .build();
        let invocation = pl_model::runtime::ModelInvocationContext::new(session.model_session())
            .with_events(recorder.sender().clone())
            .with_prompt_cache_key(session.prompt_cache_key().map(ToString::to_string))
            .with_trace(
                pl_model::completion::CompletionTraceContext {
                    session_id: recorder.session_id().to_string(),
                    turn_id: turn_id.clone(),
                    inference_id: inference_id.clone(),
                },
                recorder
                    .trace_sink()
                    .expect("enabled turn tracing must provide a canonical sink"),
            )
            .with_cancellation(cancellation_token.clone());
        progress.heartbeat(recorder, "正在等待模型响应。");
        progress.debug(recorder, format!("模型 `{model}` 流式请求已发起。"));

        let response_result = runtime.complete(completion_request, invocation).await;
        if let Err(failure) = &response_result {
            total_usage.merge(&failure.accounting.usage.totals());
            let billing = inference_billing_record(InferenceBillingInput {
                inference_id: inference_id.clone(),
                provider_instance_id: runtime.provider_instance_id(),
                provider: &runtime.endpoint().name,
                model: &model,
                accounting: &failure.accounting,
                model_info: runtime.model(),
                prompt: super::prompt_cache::current(session, &options.prompt_scope),
                orchestration: Default::default(),
                timing: None,
                recorded_at: unix_seconds(),
            });
            super::inference::record(
                turn_billing,
                &options,
                session,
                recorder,
                super::inference::from_billing(active_subagent.as_ref(), billing),
            )
            .await?;
        }
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
                    normalize_provider_error(active_subagent.as_ref(), error.into());
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
        let response_usage = response.accounting.usage.totals();
        response.orchestration.tool_schema_estimated_tokens = tool_schema_estimated_tokens;
        let actual_model = if response.model.is_empty() {
            model.clone()
        } else {
            response.model.clone()
        };
        if let Err(error) = inference_item.apply(inference_item.command(
            unix_seconds(),
            pl_trace::TracePartAction::UpdateInferenceModel {
                model: actual_model.clone(),
            },
        )) {
            tracing::error!(%error, "failed to update inference model trace");
        }
        let usage_snapshot = response_usage.public_snapshot();
        recorder.complete_inference_item(inference_item, usage_snapshot.clone());
        let model_info = runtime.model().clone();
        let recorded_at = unix_seconds();
        let mut billing = inference_billing_record(InferenceBillingInput {
            inference_id,
            provider_instance_id: runtime.provider_instance_id(),
            provider: &runtime.endpoint().name,
            model: &actual_model,
            accounting: &response.accounting,
            model_info: &model_info,
            prompt: super::prompt_cache::current(session, &options.prompt_scope),
            orchestration: response.orchestration.clone(),
            timing: response.timing,
            recorded_at,
        });
        total_usage.merge(&response_usage);
        inference_count = inference_count.saturating_add(1);
        options.apply_budget_refresh(&mut budget_tracker);
        if let Err(limit) = budget_tracker.check_wall_clock() {
            super::inference::record(
                turn_billing,
                &options,
                session,
                recorder,
                super::inference::from_billing(active_subagent.as_ref(), billing),
            )
            .await?;
            budget_limit = Some(limit);
            break;
        }
        let response_prompt_tokens = response_usage.prompt_tokens;
        let response_total_tokens = response.accounting.usage.known_total_tokens();
        if response_total_tokens.is_some() {
            last_context_tokens = response_total_tokens;
        }
        let response_reached_auto_compact_limit = model_info
            .resolved_auto_compact_limit()
            .is_some_and(|limit| {
                response_prompt_tokens >= limit
                    || response_total_tokens.is_some_and(|tokens| tokens >= limit)
            });
        let content = response.content.unwrap_or_default();
        let reasoning_content = response.reasoning_content.clone();
        let tool_calls = response.tool_calls;
        session.push_responses_context_items(response.responses_context_items);

        last_model = actual_model;

        if tool_calls.is_empty() {
            progress.milestone(recorder, "模型已完成正文生成。");
            if looks_like_unexecuted_tool_call_text(&content) {
                super::inference::record(
                    turn_billing,
                    &options,
                    session,
                    recorder,
                    super::inference::from_billing(active_subagent.as_ref(), billing),
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
            super::inference::record(
                turn_billing,
                &options,
                session,
                recorder,
                super::inference::from_billing(active_subagent.as_ref(), billing),
            )
            .await?;
            if super::checkpoint::finish_mailbox_window(&options, session, recorder, &turn_id)
                .await?
            {
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
            provider_prompt_tokens_for_compaction = response_total_tokens;
        }
        let count = tool_calls.len();
        progress.tool_detail(recorder, format!("模型请求调用 {count} 个工具。"));

        let prepared_session_items = prepare_context_items(
            session.items(),
            &mut materialized_attachments,
            core.attachment_runtime.as_ref(),
        )
        .await?
        .0;
        core.tool_session_runtime
            .update_parent_session(Arc::new(AgentSession::from_items(prepared_session_items)));
        let tool_session_id = recorder.session_id().to_string();
        let tool_batch = match execute_tool_call_batch(
            &tool_calls,
            &mut budget_tracker,
            recorder,
            ToolExecutionContext {
                core,
                tool_plan: tool_plan.clone(),
                options: &options,
                session_id: &tool_session_id,
                turn_id: &turn_id,
                step: iteration,
                workspace: workspace.clone(),
                active_subagent: active_subagent.clone(),
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
                ));
            }
            Err(ToolExecutionError::RespondToModel(error)) => {
                session.truncate_messages(safe_message_count);
                super::inference::record(
                    turn_billing,
                    &options,
                    session,
                    recorder,
                    super::inference::from_billing(active_subagent.as_ref(), billing),
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
                ));
            }
        };
        billing.orchestration.merge(&tool_batch.orchestration);
        let mut tool_results = tool_batch.records;
        let mut discovery = session.tool_discovery().clone();
        for result in &tool_results {
            for event in &result.runtime_events {
                if let crate::tool::ToolDirective::RevealTools {
                    catalog_fingerprint,
                    tool_names,
                } = event
                    && tool_plan.catalog_fingerprint() == Some(catalog_fingerprint.as_str())
                {
                    discovery.catalog_fingerprint = Some(catalog_fingerprint.clone());
                    discovery
                        .revealed_tool_names
                        .extend(tool_names.iter().cloned());
                }
            }
        }
        if session.replace_tool_discovery(tool_plan.normalized_discovery_state(&discovery)) {
            super::checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        progress.tool_detail(recorder, "工具执行完成，准备回写结果。");
        let requested_interaction = tool_results.iter().any(|tool_result| {
            tool_result.runtime_events.iter().any(|event| {
                matches!(
                    event,
                    crate::tool::ToolDirective::InteractionRequested { .. }
                )
            })
        });
        let should_end_turn = tool_results.iter().any(|tool_result| {
            tool_result
                .runtime_events
                .iter()
                .any(|event| matches!(event, crate::tool::ToolDirective::EndTurn { .. }))
        });
        let end_turn_content = tool_results.iter().find_map(|tool_result| {
            tool_result
                .runtime_events
                .iter()
                .find_map(|event| match event {
                    crate::tool::ToolDirective::EndTurn {
                        final_content: Some(content),
                    } => Some(content.clone()),
                    crate::tool::ToolDirective::InteractionRequested { .. }
                    | crate::tool::ToolDirective::SkillActivated { .. }
                    | crate::tool::ToolDirective::ToolResultRevision { .. }
                    | crate::tool::ToolDirective::OutputArtifacts { .. }
                    | crate::tool::ToolDirective::RevealTools { .. }
                    | crate::tool::ToolDirective::AuditMetadata { .. }
                    | crate::tool::ToolDirective::ExecutionFailed
                    | crate::tool::ToolDirective::CacheHit { .. }
                    | crate::tool::ToolDirective::OutputMetrics { .. }
                    | crate::tool::ToolDirective::OutputBudget { .. }
                    | crate::tool::ToolDirective::EndTurn {
                        final_content: None,
                    } => None,
                })
        });
        let remaining_context_tokens = model_info
            .resolved_auto_compact_limit()
            .or_else(|| model_info.resolved_context_window())
            .zip(response_total_tokens)
            .map(|(limit, tokens)| limit.saturating_sub(tokens));
        super::tool_results::apply_batch_budget(&mut tool_results, remaining_context_tokens);
        super::tool_results::normalize_programmatic_results(&mut tool_results, &tool_calls);
        billing.orchestration.tool_result_estimated_tokens =
            crate::tool::estimate_tool_result_tokens(
                tool_results
                    .iter()
                    .map(|tool_result| tool_result.result.as_str()),
            );
        let tool_results = tool_results
            .into_iter()
            .map(|tool_result| {
                let receipt = super::tool_results::receipt(&tool_result);
                (tool_result, receipt)
            })
            .collect::<Vec<_>>();
        // 先补齐全部 canonical tool result，再更新辅助 evidence ledger。这样即使
        // pinned working context 的大小校验或持久化失败，也不会留下只有
        // assistant tool call、没有对应 output 的不可重放 session。
        for ((tool_result, receipt), _tool_call) in tool_results.iter().zip(&tool_calls) {
            session.push_tool_result_with_receipt(
                pl_protocol::ToolResultRecord {
                    item_id: tool_result.id.clone(),
                    call_id: tool_result.call_id.clone(),
                    name: tool_result.name.clone(),
                    kind: tool_result.kind,
                },
                tool_result.result.clone(),
                receipt.clone(),
            );
        }
        let tool_media = tool_results
            .iter()
            .flat_map(|(tool_result, _)| {
                tool_result
                    .model_attachments
                    .iter()
                    .cloned()
                    .map(|attachment| {
                        let label = attachment
                            .filename
                            .clone()
                            .unwrap_or_else(|| "image".to_string());
                        pl_protocol::ToolMediaContext {
                            call_id: tool_result.call_id.clone(),
                            label,
                            attachment,
                        }
                    })
            })
            .collect();
        session.push_tool_media(tool_media);
        if let Some(content) = end_turn_content {
            session.push_assistant_response(content.clone(), None);
            recorder.final_text_item(&turn_id, content.clone());
            last_content = content;
        }
        super::inference::record(
            turn_billing,
            &options,
            session,
            recorder,
            super::inference::from_billing(active_subagent.as_ref(), billing),
        )
        .await?;
        for (_, receipt) in tool_results {
            working_set.apply(TurnWorkingSetChange::AppendEvidence(receipt))?;
        }
        if working_set.sync_session(session)? {
            super::checkpoint::persist(
                &options,
                session,
                crate::TurnCheckpointReason::WorkingSetChanged,
            )
            .await?;
        }
        turn_context.refresh_working_context(working_set.model_context_snapshot(session));
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
            if super::checkpoint::finish_mailbox_window(&options, session, recorder, &turn_id)
                .await?
            {
                safe_message_count = session.len();
                session_message_count = safe_message_count;
                iteration = iteration.saturating_add(1);
                continue;
            }
            completion = if requested_interaction {
                TurnCompletion::InteractionRequested
            } else {
                TurnCompletion::Normal
            };
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
            super::super::turn_result::budget_limit_message(limit.kind, &limit.usage),
        ));
    }

    progress.milestone(recorder, "本轮已完成。");
    if !terminal_checkpointed {
        super::checkpoint::persist(&options, session, crate::TurnCheckpointReason::Terminal)
            .await?;
    }
    Ok(super::completion::finish(
        recorder,
        &turn_id,
        super::completion::CompletedTurn {
            content: last_content,
            reasoning_content: last_reasoning_content,
            model: last_model,
            usage: total_usage,
            last_context_tokens,
            context_compactions,
            session_message_count,
            completion,
        },
    ))
}
