use pl_model::{CompletionRequest, ModelProvider, ReasoningConfig, ReasoningSummary};
use pl_protocol::TokenUsageSnapshot;
use pl_protocol::{ErrorSeverity, Result};
use pl_trace::{AgentEvent, TracePartStatus};
use std::sync::Arc;

mod attachments;
pub(super) mod enabled_tools;
mod plan_exit;

use attachments::materialize_messages;
use enabled_tools::record_enabled_tools;
use plan_exit::record_plan_exit_items;

use crate::context_compaction::{
    CompactionOutcome, ContextCompactionRequest, maybe_compact_session,
};
use crate::instruction::{InstructionAssembler, InstructionAssemblyRequest};
use crate::runtime_usage::{agent_runtime_delta, identity_for_subagent, token_usage_snapshot};
use crate::session::CoreSession;
use crate::trace::TraceRecorder;
use crate::turn::{
    AGENT_MAX_COUNT, AGENT_MAX_DEPTH, BudgetLimit, BudgetTracker, CompileMode, TurnOptions,
    TurnRequest, TurnResult, TurnResultStatus,
};

use super::PureCore;
use super::SUBAGENT_DISPATCH_CONSTRAINT;
use super::SUBAGENT_FORCE_DISPATCH_INSTRUCTION;
use super::permission::cancellation_reason;
use super::progress::{ProgressEmitter, ProgressVerbosity};
use super::tool_dispatch::records::tool_results_include_recoverable_subagent_capacity;
use super::tool_dispatch::{ToolExecutionContext, ToolExecutionError, execute_tool_calls};
use super::turn_result::{
    budget_limited_turn_result, default_workspace_root, failed_turn_result,
    failed_turn_result_with_abort_reason, interrupted_turn_result, is_cancelled,
    looks_like_unexecuted_tool_call_text, prompt_requires_subagent_dispatch,
    provider_error_severity, should_request_parallel_tool_calls, tool_allowed_in_mode,
    unix_seconds,
};

pub(super) async fn run_turn_with_trace(
    core: &PureCore,
    session: &mut CoreSession,
    request: TurnRequest,
    recorder: &mut TraceRecorder,
    options: TurnOptions,
) -> Result<TurnResult> {
    let provider = core.provider.clone();
    let reasoning_effort = core.reasoning_effort.clone();
    let workspace_root = core
        .workspace_root
        .clone()
        .unwrap_or_else(default_workspace_root);
    let workspace_instructions = core.workspace_instructions.clone();
    let active_subagent = core.active_subagent.clone();
    let agent_control = core.agent_control.clone();
    agent_control
        .configure_limits(AGENT_MAX_COUNT, AGENT_MAX_DEPTH)
        .await;
    let cancellation_token = options.cancellation_token.clone();
    let tool_schemas = core
        .tools
        .schemas()
        .into_iter()
        .filter(|schema| tool_allowed_in_mode(request.mode, schema.name()))
        .collect::<Vec<_>>();
    let mut budget_tracker = BudgetTracker::new(request.budget);
    let mut budget_limit: Option<BudgetLimit> = None;

    let turn_id = request
        .turn_id
        .clone()
        .unwrap_or_else(super::generate_turn_id);
    let requires_subagent_dispatch =
        active_subagent.is_none() && prompt_requires_subagent_dispatch(&request.prompt);
    let initial_agent_count = if requires_subagent_dispatch {
        Some(agent_control.list_agents(None).await.len())
    } else {
        None
    };
    let mut subagent_dispatch_recovered = false;
    recorder.user_text_item_with_attachments(
        &turn_id,
        request.prompt.clone(),
        request.trace_attachments.clone(),
    );
    session.push_user_content(request.user_content.clone());
    record_enabled_tools(recorder, &turn_id, request.mode, &tool_schemas);
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
    let mut total_usage = pl_model::TokenUsage::default();
    let mut safe_message_count = session.len();
    let mut session_message_count = safe_message_count;

    let model_info = provider.model_info(&model);
    let instruction_snapshot = match request.instruction_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => InstructionAssembler::assemble(InstructionAssemblyRequest {
            config: core.config.as_ref(),
            model: &model_info,
            mode: request.mode,
            workspace_root: &workspace_root,
            current_dir: &workspace_root,
            workspace_instructions: request.workspace_instructions.as_deref(),
            subagent_constraint: None,
        })?,
    };
    let turn_instruction_snapshot = if requires_subagent_dispatch {
        instruction_snapshot
            .clone()
            .with_subagent_constraint(SUBAGENT_DISPATCH_CONSTRAINT)
    } else {
        instruction_snapshot.clone()
    };
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
    loop {
        let must_dispatch_agent_now = if let Some(initial_count) = initial_agent_count {
            iteration >= 2
                && !subagent_dispatch_recovered
                && agent_control.list_agents(None).await.len() <= initial_count
        } else {
            false
        };
        if is_cancelled(&options) {
            session.truncate_messages(safe_message_count);
            return Ok(interrupted_turn_result(
                recorder,
                &turn_id,
                request.mode,
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

        let iteration_tools = if must_dispatch_agent_now {
            tool_schemas
                .iter()
                .filter(|schema| schema.name() == "spawn_agent")
                .cloned()
                .collect()
        } else {
            tool_schemas.clone()
        };
        let iteration_snapshot = if must_dispatch_agent_now {
            turn_instruction_snapshot
                .clone()
                .with_subagent_force(SUBAGENT_FORCE_DISPATCH_INSTRUCTION)
        } else {
            turn_instruction_snapshot.clone()
        };
        let instruction_bundle = iteration_snapshot.to_bundle();

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
                    request_instructions: &instruction_bundle.instructions,
                    request_messages: &instruction_bundle.prelude_messages,
                    trigger: compaction_trigger,
                    event_tx: recorder.sender().clone(),
                    progress: Some(&mut progress),
                },
            )
            .await;
            match compaction_result {
                Ok(CompactionOutcome::Skipped) => {}
                Ok(CompactionOutcome::Compacted { usage }) => {
                    last_compacted_state = Some((session.revision(), session.len()));
                    safe_message_count = session.len();
                    session_message_count = safe_message_count;
                    total_usage.prompt_tokens += usage.prompt_tokens;
                    total_usage.completion_tokens += usage.completion_tokens;
                    total_usage.cached_prompt_tokens += usage.cached_prompt_tokens;
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
                Err(error) => {
                    let error = error.to_string();
                    let severity = provider_error_severity(active_subagent.as_ref(), &error);
                    return Ok(failed_turn_result(
                        recorder,
                        &turn_id,
                        request.mode,
                        last_content,
                        last_reasoning_content,
                        last_model,
                        total_usage,
                        session.messages().len(),
                        error,
                        severity,
                    ));
                }
            }
        }
        if let Err(limit) = budget_tracker.check_wall_clock() {
            budget_limit = Some(limit);
            break;
        }
        budget_tracker.record_model_step();
        let history_messages =
            materialize_messages(session.messages(), &request.materialized_attachments)?;
        let mut messages = instruction_bundle.prelude_messages.clone();
        messages.extend(history_messages.clone());
        if iteration == 0 {
            progress.milestone("上下文已整理，准备调用模型。");
        } else {
            progress.tool_detail("工具结果已写入上下文，准备继续调用模型。");
        }

        let inference_id = format!("{turn_id}-inf-{iteration}");
        let mut inference_item = recorder.inference_item(&turn_id, &inference_id, &model);
        recorder.start_item(inference_item.clone());
        let model_capabilities = provider.effective_model_capabilities(&model);
        let parallel_tool_calls = should_request_parallel_tool_calls(model_capabilities, &options);

        let completion_request = CompletionRequest {
            model: model.clone(),
            instructions: Some(instruction_bundle.instructions.clone()),
            messages: messages.clone(),
            tools: iteration_tools,
            tool_choice: "auto".to_string(),
            parallel_tool_calls,
            temperature: None,
            max_tokens: None,
            reasoning: reasoning.clone(),
            stream: true,
            trace: Some(pl_model::CompletionTraceContext {
                session_id: recorder.session_id().to_string(),
                turn_id: turn_id.clone(),
                inference_id: inference_id.clone(),
                plan_mode: matches!(request.mode, CompileMode::Plan),
                trace_sequence_base: recorder.current_sequence(),
            }),
        };
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
                            request.mode,
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
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    safe_message_count,
                    cancellation_reason(),
                ));
            }
            Err(error) => {
                let error = error.to_string();
                let severity = provider_error_severity(active_subagent.as_ref(), &error);
                return Ok(failed_turn_result(
                    recorder,
                    &turn_id,
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.messages().len(),
                    error,
                    severity,
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
        let response_reached_auto_compact_limit = model_info
            .resolved_auto_compact_limit()
            .is_some_and(|limit| response_prompt_tokens >= limit);
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

        last_model = actual_model;

        if tool_calls.is_empty() {
            progress.milestone("模型已完成正文生成。");
            if looks_like_unexecuted_tool_call_text(&content) {
                return Ok(failed_turn_result(
                    recorder,
                    &turn_id,
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.messages().len(),
                    "模型返回了未执行的工具调用文本，未产生可执行 tool call。".to_string(),
                    ErrorSeverity::Recoverable,
                ));
            }
            if let Some(initial_count) = initial_agent_count {
                let current_count = agent_control.list_agents(None).await.len();
                if current_count <= initial_count && !subagent_dispatch_recovered {
                    return Ok(failed_turn_result(
                        recorder,
                        &turn_id,
                        request.mode,
                        last_content,
                        last_reasoning_content,
                        last_model,
                        total_usage,
                        session.messages().len(),
                        "用户明确要求子代理分工，但本轮没有实际创建任何 agent。".to_string(),
                        ErrorSeverity::Recoverable,
                    ));
                }
            }
            session.push_assistant_response(content.clone(), reasoning_content.clone());
            last_content = content;
            last_reasoning_content = reasoning_content;
            session_message_count = session.messages().len();
            safe_message_count = session_message_count;
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
            provider_prompt_tokens_for_compaction = Some(response_prompt_tokens);
        }
        progress.tool_detail(format!("模型请求调用 {} 个工具。", tool_calls.len()));

        let tool_results = match execute_tool_calls(
            &tool_calls,
            &mut budget_tracker,
            recorder,
            ToolExecutionContext {
                core,
                options: &options,
                mode: request.mode,
                session_id: &turn_id,
                workspace_root: &workspace_root,
                workspace_instructions: workspace_instructions.clone(),
                active_subagent: active_subagent.clone(),
                agent_control: agent_control.clone(),
                instruction_snapshot: Some(instruction_snapshot.clone()),
                parent_session: Arc::new(CoreSession::from_messages(history_messages)),
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
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    safe_message_count,
                    error,
                    ErrorSeverity::Recoverable,
                    crate::turn::TurnAbortReason::ToolError,
                ));
            }
        };
        if tool_results_include_recoverable_subagent_capacity(&tool_results) {
            subagent_dispatch_recovered = true;
        }
        progress.tool_detail("工具执行完成，准备回写结果。");
        if request.mode == CompileMode::Plan {
            record_plan_exit_items(recorder, &turn_id, &tool_results);
        }
        if is_cancelled(&options) {
            session.truncate_messages(safe_message_count);
            return Ok(interrupted_turn_result(
                recorder,
                &turn_id,
                request.mode,
                last_content,
                last_reasoning_content,
                last_model,
                total_usage,
                safe_message_count,
                cancellation_reason(),
            ));
        }
        for tool_result in tool_results {
            session.push_tool_result(
                tool_result.id,
                tool_result.call_id,
                tool_result.name,
                tool_result.kind,
                tool_result.result,
                tool_result.arguments,
            );
        }

        session_message_count = session.messages().len();
        safe_message_count = session_message_count;
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
            request.mode,
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
            request.mode,
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
    recorder.broadcast(AgentEvent::Done);

    Ok(TurnResult {
        content: last_content,
        reasoning_content: last_reasoning_content,
        model: last_model,
        usage: total_usage,
        mode: request.mode,
        session_message_count,
        status: TurnResultStatus::Completed,
        abort_reason: None,
        error: None,
        budget_limit_kind: None,
        budget_usage: None,
        trace_events: recorder.drain(),
    })
}
