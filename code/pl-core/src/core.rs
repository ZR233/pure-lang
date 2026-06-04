use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::future::{BoxFuture, join_all};
use pl_model::{
    CompletionRequest, ModelProvider, ProviderCapabilities, ProviderInfo, ReasoningConfig,
    ReasoningSummary, SharedModelProvider, TokenUsage, ToolCallKind, create_provider,
    create_provider_with_models,
};
use pl_protocol::{
    AgentEvent, AgentEventSender, BudgetLimitKind, BudgetUsage, ErrorSeverity, Message,
    MessageContent, MessageRole, Result, TimelineItem, TimelineItemStatus, TokenUsageSnapshot,
};
use tokio::sync::RwLock;

use crate::config::{ModelRole, PureConfig, ReasoningEffort};
use crate::context_compaction::{
    CompactionOutcome, CompactionTrigger, ContextCompactionRequest, maybe_compact_session,
};
use crate::permission::{PermissionDecision, decide_tool_permission, parse_reviewer_decision};
use crate::provider_error::is_provider_429_error;
use crate::runtime_usage::{agent_runtime_delta, identity_for_subagent, token_usage_snapshot};
use crate::session::CoreSession;
use crate::tool::{
    ApplyPatchTool, BashTool, CloseAgentTool, CopyPathTool, CreateDirectoryTool, DeletePathTool,
    FollowupTaskTool, ListAgentsTool, ListFilesTool, MovePathTool, RECOVERABLE_SUBAGENT_429_MARKER,
    ReadFileTool, SearchFilesTool, SendMessageTool, SkillManageTool, SkillViewTool, SkillsListTool,
    SpawnAgentTool, StatPathTool, SubagentContext, SubagentTool, ToolContext, ToolInput,
    ToolOutput, ToolRegistry, WaitAgentTool, WorkspaceAccess, WriteFileTool,
};
use crate::trace::TraceRecorder;
use crate::turn::{
    AGENT_MAX_COUNT, AGENT_MAX_DEPTH, BudgetLimit, BudgetTracker, ToolApprovalDecision,
    ToolApprovalRequest, ToolExecutionMode, TurnOptions, TurnRequest, TurnResult, TurnResultStatus,
};

/// 生成唯一的会话 ID（毫秒时间戳 + 序列号）。
fn generate_session_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:x}")
}

const SUBAGENT_DISPATCH_CONSTRAINT: &str = "\n\n# 子代理调度约束\n用户明确要求使用 subagent/子代理分工时，必须先调度 `spawn_agent` 或 `subagent` 工具；不要只用 `bash` 或文件工具替代。若尚未知道 crate 列表，可以先用只读工具定位 workspace，再为每个 crate 创建 explorer agent，最后由父会话汇总。如果 `subagent`、`wait_agent` 或 `list_agents` 返回 `recoverableSubagentProvider429` 或 `recoverableFailures`，表示 provider 429 并发/容量上限导致子代理不可用；此时停止继续创建或重试子代理，由当前父 agent 自己完成剩余工作。";

const SUBAGENT_FORCE_DISPATCH_INSTRUCTION: &str = "# 当前轮强制要求\n前面已进行了必要定位但尚未创建 agent。本轮必须调用 `spawn_agent` 或 `subagent`，不要继续调用文件、shell 或搜索工具，也不要输出最终回答。若工具结果提示 `recoverableSubagentProvider429`，后续不再重试创建子代理，改由当前 agent 自己完成任务。";

/// Pure-Lang 核心逻辑层。
///
/// 负责组合会话状态、模型 provider、工具注册表和单轮编译请求。
/// 工具能力由调用方显式注册，并通过 `TurnOptions` 控制审批策略。
#[derive(Debug)]
pub struct PureCore {
    provider: SharedModelProvider,
    reasoning_effort: Option<ReasoningEffort>,
    config: Option<PureConfig>,
    workspace_root: Option<PathBuf>,
    workspace_instructions: Option<String>,
    active_subagent: Option<SubagentContext>,
    agent_control: crate::AgentControl,
    tools: ToolRegistry,
}

impl PureCore {
    pub fn new(provider: SharedModelProvider) -> Self {
        Self {
            provider,
            reasoning_effort: None,
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        }
    }

    pub fn with_reasoning_effort(
        provider: SharedModelProvider,
        reasoning_effort: ReasoningEffort,
    ) -> Self {
        Self {
            provider,
            reasoning_effort: Some(reasoning_effort),
            config: None,
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        }
    }

    pub fn from_provider_info(info: ProviderInfo) -> Result<Self> {
        Ok(Self::new(create_provider(info)?))
    }

    pub fn default_provider() -> Result<Self> {
        Self::from_provider_info(ProviderInfo::default_provider())
    }

    pub fn from_config(config: &PureConfig, role: ModelRole) -> Result<Self> {
        let resolved = config.resolve_role(role)?;
        let provider = create_provider_with_models(resolved.provider_info, resolved.models)?;
        Ok(Self {
            provider,
            reasoning_effort: Some(resolved.role_config.effort),
            config: Some(config.clone()),
            workspace_root: None,
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
            tools: ToolRegistry::new(),
        })
    }

    pub(crate) fn with_subagent_context(mut self, context: SubagentContext) -> Self {
        self.active_subagent = Some(context);
        self
    }

    pub(crate) fn with_agent_control(mut self, agent_control: crate::AgentControl) -> Self {
        self.agent_control = agent_control;
        self
    }

    /// 注册一个工具。
    pub fn register_tool(&mut self, tool: impl crate::tool::Tool + 'static) {
        self.tools.register(tool);
    }

    /// 注册默认工具集合。
    ///
    /// 当前包含 shell、subagent 和 workspace 文件工具。调用方应通过 `TurnOptions` 控制审批策略。
    pub fn register_default_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        let workspace_root = workspace_root.into();
        self.workspace_root = Some(workspace_root.clone());
        self.workspace_instructions = workspace_instructions.clone();
        self.register_skill_tools_for_workspace(
            workspace_root.clone(),
            workspace_instructions.clone(),
        );
        self.register_tool(BashTool::new(workspace_root));
        self.register_tool(ReadFileTool::new());
        self.register_tool(WriteFileTool);
        self.register_tool(ListFilesTool);
        self.register_tool(SearchFilesTool);
        self.register_tool(StatPathTool);
        self.register_tool(CreateDirectoryTool);
        self.register_tool(DeletePathTool);
        self.register_tool(CopyPathTool);
        self.register_tool(MovePathTool);
        self.register_tool(ApplyPatchTool);
        self.register_tool(SpawnAgentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(WaitAgentTool);
        self.register_tool(ListAgentsTool);
        self.register_tool(SendMessageTool);
        self.register_tool(FollowupTaskTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            workspace_instructions.clone(),
        ));
        self.register_tool(CloseAgentTool);
        self.register_tool(SubagentTool::new(
            self.provider.clone(),
            self.reasoning_effort.clone(),
            self.config.clone(),
            workspace_instructions,
        ));
    }

    pub(crate) fn register_skill_tools(
        &mut self,
        workspace_root: impl Into<std::path::PathBuf>,
        workspace_instructions: Option<String>,
    ) {
        self.register_skill_tools_for_workspace(workspace_root.into(), workspace_instructions);
    }

    fn register_skill_tools_for_workspace(
        &mut self,
        workspace_root: std::path::PathBuf,
        workspace_instructions: Option<String>,
    ) {
        self.workspace_root = Some(workspace_root);
        self.workspace_instructions = workspace_instructions;
        let Some(config) = self.config.as_ref().map(|config| config.skills.clone()) else {
            return;
        };
        if !config.enabled {
            return;
        }
        self.register_tool(SkillsListTool::new(config.clone()));
        self.register_tool(SkillViewTool::new(config.clone()));
        self.register_tool(SkillManageTool::new(config));
    }

    async fn review_tool_call_with_ai(
        &self,
        request: &ToolApprovalRequest,
        context: &ToolContext,
    ) -> ToolApprovalDecision {
        let (provider, reasoning_effort) = match &self.config {
            Some(config) => match PureCore::from_config(config, ModelRole::Reviewer) {
                Ok(core) => (core.provider, core.reasoning_effort),
                Err(error) => {
                    return ToolApprovalDecision::Denied {
                        reason: format!("AI reviewer is unavailable: {error}"),
                    };
                }
            },
            None => (self.provider.clone(), self.reasoning_effort.clone()),
        };
        let reasoning = reasoning_effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(ReasoningSummary::Enabled),
        });
        let payload = serde_json::json!({
            "toolName": &request.name,
            "arguments": &request.arguments,
            "workingDirectory": &request.working_directory,
            "parentAgentId": &request.parent_agent_id,
            "permissionMode": context.options.permission_mode.label(),
            "workspaceAccess": format!("{:?}", context.workspace_access),
            "compileMode": context.mode.label(),
            "workspaceRoot": context.workspace_root.display().to_string(),
            "riskSummary": permission_risk_summary(&request.name),
        });
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text(
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
            ),
            reasoning_content: None,
            metadata: HashMap::new(),
        };
        let completion_request = CompletionRequest {
            model: provider.default_model().to_string(),
            instructions: Some(include_str!("../prompts/permission_review.md").to_string()),
            messages: vec![message],
            tools: Vec::new(),
            tool_choice: "none".to_string(),
            parallel_tool_calls: false,
            temperature: Some(0.0),
            max_tokens: Some(512),
            reasoning,
            stream: false,
            timeline: None,
        };
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
        match provider.stream_complete(completion_request, event_tx).await {
            Ok(response) => {
                let content = response
                    .raw_content
                    .or(response.content)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if content.is_empty() {
                    return ToolApprovalDecision::Denied {
                        reason: "AI reviewer returned an empty decision".to_string(),
                    };
                }
                match parse_reviewer_decision(&content) {
                    Ok(decision) => decision,
                    Err(error) => ToolApprovalDecision::Denied { reason: error },
                }
            }
            Err(error) => ToolApprovalDecision::Denied {
                reason: format!("AI reviewer failed: {error}"),
            },
        }
    }

    pub fn run_turn<'a>(
        &'a self,
        session: &'a mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<TurnResult>> + Send + 'a {
        self.run_turn_with_options(session, request, event_tx, TurnOptions::default())
    }

    pub async fn run_turn_with_options(
        &self,
        session: &mut CoreSession,
        request: TurnRequest,
        event_tx: AgentEventSender,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        let mut recorder = TraceRecorder::disabled(event_tx);
        self.run_turn_with_trace(session, request, &mut recorder, options)
            .await
    }

    pub async fn run_turn_with_trace(
        &self,
        session: &mut CoreSession,
        request: TurnRequest,
        recorder: &mut TraceRecorder,
        options: TurnOptions,
    ) -> Result<TurnResult> {
        let provider = self.provider.clone();
        let reasoning_effort = self.reasoning_effort.clone();
        let workspace_root = self
            .workspace_root
            .clone()
            .unwrap_or_else(default_workspace_root);
        let workspace_instructions = self.workspace_instructions.clone();
        let active_subagent = self.active_subagent.clone();
        let agent_control = self.agent_control.clone();
        agent_control
            .configure_limits(AGENT_MAX_COUNT, AGENT_MAX_DEPTH)
            .await;
        let cancellation_token = options.cancellation_token.clone();
        let tool_schemas = self
            .tools
            .schemas()
            .into_iter()
            .filter(|schema| tool_allowed_in_mode(request.mode, schema.name()))
            .collect::<Vec<_>>();
        let mut budget_tracker = BudgetTracker::new(request.budget);
        let mut budget_limit: Option<BudgetLimit> = None;

        let session_id = generate_session_id();
        let turn_item = recorder.turn_item(&session_id, TimelineItemStatus::Running);
        recorder.start_item(turn_item.clone());
        let requires_subagent_dispatch =
            active_subagent.is_none() && prompt_requires_subagent_dispatch(&request.prompt);
        let initial_agent_count = if requires_subagent_dispatch {
            Some(agent_control.list_agents(None).await.len())
        } else {
            None
        };
        let mut subagent_dispatch_recovered = false;
        recorder.user_text_item(&session_id, request.prompt.clone());
        session.push_user_prompt(request.prompt);
        let model = provider.default_model().to_string();

        let mut last_content = String::new();
        let mut last_reasoning_content = None;
        let mut last_model = model.clone();
        let mut total_usage = pl_model::TokenUsage::default();
        let mut session_message_count = 0;

        let skills_instructions = self.config.as_ref().and_then(|config| {
            match crate::skill::build_skills_prompt(&workspace_root, &config.skills) {
                Ok(instructions) => instructions,
                Err(error) => {
                    eprintln!("[pl-core] failed to load skills prompt: {error}");
                    None
                }
            }
        });
        let mut instructions = format_instructions(
            request.mode.instructions(),
            skills_instructions.as_deref(),
            request.workspace_instructions.as_deref(),
        );
        if requires_subagent_dispatch {
            instructions.push_str(SUBAGENT_DISPATCH_CONSTRAINT);
        }
        let reasoning = reasoning_effort.as_ref().map(|effort| ReasoningConfig {
            effort: Some(effort.as_str().to_string()),
            summary: Some(if effort.is_none() {
                ReasoningSummary::Disabled
            } else {
                ReasoningSummary::Enabled
            }),
        });

        let mut messages;
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
                return Ok(interrupted_turn_result(
                    recorder,
                    &session_id,
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.messages().len(),
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
                    .filter(|schema| matches!(schema.name(), "spawn_agent" | "subagent"))
                    .cloned()
                    .collect()
            } else {
                tool_schemas.clone()
            };
            let iteration_instructions = if must_dispatch_agent_now {
                format!("{instructions}\n\n{SUBAGENT_FORCE_DISPATCH_INSTRUCTION}")
            } else {
                instructions.clone()
            };

            let compaction_trigger = provider_prompt_tokens_for_compaction
                .take()
                .map_or(CompactionTrigger::EstimatedTokens, |prompt_tokens| {
                    CompactionTrigger::ProviderPromptTokens(prompt_tokens)
                });
            let current_compaction_state = (session.revision(), session.len());
            if last_compacted_state != Some(current_compaction_state) {
                let compaction_result = maybe_compact_session(
                    session,
                    ContextCompactionRequest {
                        provider: &provider,
                        model: &model,
                        request_instructions: &iteration_instructions,
                        trigger: compaction_trigger,
                        event_tx: recorder.sender().clone(),
                    },
                )
                .await;
                match compaction_result {
                    Ok(CompactionOutcome::Skipped) => {}
                    Ok(CompactionOutcome::Compacted { usage }) => {
                        last_compacted_state = Some((session.revision(), session.len()));
                        total_usage.prompt_tokens += usage.prompt_tokens;
                        total_usage.completion_tokens += usage.completion_tokens;
                        total_usage.cached_prompt_tokens += usage.cached_prompt_tokens;
                        let usage_snapshot = token_usage_snapshot(&usage);
                        let model_info = provider.model_info(&model);
                        recorder.broadcast(AgentEvent::AgentRuntimeUpdated {
                            delta: agent_runtime_delta(
                                format!("{session_id}-compact-{iteration}"),
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
                            &session_id,
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
            messages = session.messages().to_vec();
            if let Err(limit) = budget_tracker.check_wall_clock() {
                budget_limit = Some(limit);
                break;
            }
            budget_tracker.record_model_step();

            let inference_id = format!("{session_id}-inf-{iteration}");
            let mut inference_item = recorder.inference_item(&session_id, &inference_id, &model);
            recorder.start_item(inference_item.clone());
            let parallel_tool_calls =
                should_request_parallel_tool_calls(provider.capabilities(), &options);

            let completion_request = CompletionRequest {
                model: model.clone(),
                instructions: Some(iteration_instructions),
                messages: messages.clone(),
                tools: iteration_tools,
                tool_choice: "auto".to_string(),
                parallel_tool_calls,
                temperature: None,
                max_tokens: None,
                reasoning: reasoning.clone(),
                stream: true,
                timeline: Some(pl_model::CompletionTimelineContext {
                    session_id: recorder.session_id().to_string(),
                    turn_id: session_id.clone(),
                    inference_id: inference_id.clone(),
                    starting_sequence: recorder.current_sequence(),
                    plan_mode: matches!(request.mode, crate::turn::CompileMode::Plan),
                }),
            };

            let response_result = match &cancellation_token {
                Some(token) => {
                    tokio::select! {
                        result = provider.stream_complete(completion_request, recorder.sender().clone()) => result,
                        _ = token.cancelled() => {
                            return Ok(interrupted_turn_result(
                                recorder,
                                &session_id,
                                request.mode,
                                last_content,
                                last_reasoning_content,
                                last_model,
                                total_usage,
                                session.messages().len(),
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
                    return Ok(interrupted_turn_result(
                        recorder,
                        &session_id,
                        request.mode,
                        last_content,
                        last_reasoning_content,
                        last_model,
                        total_usage,
                        session.messages().len(),
                        cancellation_reason(),
                    ));
                }
                Err(error) => {
                    let error = error.to_string();
                    let severity = provider_error_severity(active_subagent.as_ref(), &error);
                    return Ok(failed_turn_result(
                        recorder,
                        &session_id,
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

            recorder.record_events(response.timeline_events.clone());
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
            let raw_content = response.raw_content.unwrap_or_else(|| content.clone());
            let reasoning_content = response.reasoning_content.clone();
            let tool_calls = response.tool_calls;

            total_usage.prompt_tokens += response.usage.prompt_tokens;
            total_usage.completion_tokens += response.usage.completion_tokens;
            total_usage.cached_prompt_tokens += response.usage.cached_prompt_tokens;

            last_model = actual_model;

            if tool_calls.is_empty() {
                if looks_like_unexecuted_tool_call_text(&content) {
                    return Ok(failed_turn_result(
                        recorder,
                        &session_id,
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
                            &session_id,
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
                session.push_assistant_response(raw_content, reasoning_content.clone());
                last_content = content;
                last_reasoning_content = reasoning_content;
                session_message_count = session.messages().len();
                break;
            }

            session.push_assistant_tool_calls(
                if raw_content.is_empty() {
                    None
                } else {
                    Some(raw_content)
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

            let tool_results = execute_tool_calls(
                &tool_calls,
                &mut budget_tracker,
                recorder,
                ToolExecutionContext {
                    core: self,
                    options: &options,
                    mode: request.mode,
                    session_id: &session_id,
                    workspace_root: &workspace_root,
                    workspace_instructions: workspace_instructions.clone(),
                    active_subagent: active_subagent.clone(),
                    agent_control: agent_control.clone(),
                },
            )
            .await;
            if tool_results_include_recoverable_subagent_capacity(&tool_results) {
                subagent_dispatch_recovered = true;
            }
            if is_cancelled(&options) {
                return Ok(interrupted_turn_result(
                    recorder,
                    &session_id,
                    request.mode,
                    last_content,
                    last_reasoning_content,
                    last_model,
                    total_usage,
                    session.messages().len(),
                    cancellation_reason(),
                ));
            }
            for tool_result in tool_results {
                session.push_tool_result(
                    tool_result.call_id,
                    tool_result.name,
                    tool_result.kind,
                    tool_result.result,
                    tool_result.arguments,
                );
            }

            session_message_count = session.messages().len();
            if budget_limit.is_some() {
                break;
            }
            iteration += 1;
        }

        total_usage.total_tokens = total_usage.prompt_tokens + total_usage.completion_tokens;
        if is_cancelled(&options) {
            return Ok(interrupted_turn_result(
                recorder,
                &session_id,
                request.mode,
                last_content,
                last_reasoning_content,
                last_model,
                total_usage,
                session_message_count,
                cancellation_reason(),
            ));
        }

        if let Some(limit) = budget_limit {
            return Ok(budget_limited_turn_result(
                recorder,
                &session_id,
                request.mode,
                last_content,
                last_reasoning_content,
                last_model,
                total_usage,
                session_message_count,
                limit.kind,
                limit.usage,
                budget_limit_message(limit.kind, &limit.usage),
            ));
        }

        recorder.ensure_assistant_text_item(&session_id, &last_content);
        let mut completed_turn_item =
            recorder.turn_item(&session_id, TimelineItemStatus::Completed);
        completed_turn_item.content = last_content.clone();
        completed_turn_item.usage = Some(TokenUsageSnapshot {
            prompt_tokens: total_usage.prompt_tokens,
            completion_tokens: total_usage.completion_tokens,
            cached_prompt_tokens: total_usage.cached_prompt_tokens,
            total_tokens: total_usage.total_tokens,
        });
        recorder.complete_item(completed_turn_item);
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
            timeline_events: recorder.drain(),
        })
    }
}

struct ToolExecutionRecord {
    call_id: String,
    name: String,
    kind: ToolCallKind,
    result: String,
    arguments: String,
    status: TimelineItemStatus,
    exit_code: Option<i32>,
    timed_out: bool,
}

struct ScheduledToolExecution<'a> {
    tool_call: pl_model::ToolCall,
    item: TimelineItem,
    future: BoxFuture<'a, ToolExecutionRecord>,
}

struct ToolExecutionContext<'a> {
    core: &'a PureCore,
    options: &'a TurnOptions,
    mode: crate::turn::CompileMode,
    session_id: &'a str,
    workspace_root: &'a Path,
    workspace_instructions: Option<String>,
    active_subagent: Option<SubagentContext>,
    agent_control: crate::AgentControl,
}

async fn execute_tool_calls(
    tool_calls: &[pl_model::ToolCall],
    budget_tracker: &mut BudgetTracker,
    recorder: &mut TraceRecorder,
    context: ToolExecutionContext<'_>,
) -> Vec<ToolExecutionRecord> {
    let mut scheduled = Vec::new();
    let mut initial_items = HashMap::new();
    let runtime_lock = Arc::new(RwLock::new(()));

    for tool_call in tool_calls {
        if is_cancelled(context.options) {
            break;
        }
        let tool_call_id = tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone());
        let timeline_item_id = namespaced_tool_timeline_item_id(context.session_id, &tool_call_id);
        let mut item = recorder.tool_item(
            context.session_id,
            &timeline_item_id,
            tool_call.name.clone(),
            tool_call.payload_text(),
            tool_call.call_id.clone(),
            Some(tool_call.id.clone()),
        );
        initial_items.insert(timeline_item_id.clone(), item.clone());
        recorder.start_item(item.clone());
        budget_tracker.record_tool_call(&tool_call.name);

        if !tool_allowed_in_mode(context.mode, &tool_call.name) {
            let mode = context.mode.label();
            let name = &tool_call.name;
            let message = format!("Tool disabled in {mode} mode: {name}");
            item.status = TimelineItemStatus::Denied;
            item.updated_at = unix_seconds();
            if let Some(tool) = &mut item.tool {
                tool.denial_reason = Some(message.clone());
                tool.result = Some(message.clone());
            }
            recorder.complete_item(item);
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item: initial_items[&timeline_item_id].clone(),
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    message,
                    TimelineItemStatus::Denied,
                    None,
                    false,
                )),
            });
            continue;
        }

        let Some(tool) = context.core.tools.get(&tool_call.name) else {
            let available: Vec<&str> = context.core.tools.names();
            eprintln!(
                "[pl-core] Unknown tool: {:?}, available: {:?}",
                tool_call.name, available
            );
            item.status = TimelineItemStatus::Failed;
            item.updated_at = unix_seconds();
            if let Some(tool) = &mut item.tool {
                tool.result = Some(format!("Unknown tool: {}", tool_call.name));
            }
            recorder.fail_item(item, format!("Unknown tool: {}", tool_call.name));
            scheduled.push(ScheduledToolExecution {
                tool_call: tool_call.clone(),
                item: initial_items[&timeline_item_id].clone(),
                future: Box::pin(ready_tool_execution_record(
                    tool_call.clone(),
                    format!("Unknown tool: {}", tool_call.name),
                    TimelineItemStatus::Failed,
                    None,
                    false,
                )),
            });
            continue;
        };

        let supports_parallel = tool.supports_parallel_tool_calls()
            && matches!(
                context.options.tool_execution_mode,
                ToolExecutionMode::ModelDefault | ToolExecutionMode::Parallel
            );
        let tool_context = ToolContext {
            event_tx: recorder.sender().clone(),
            options: context.options.clone(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: context.mode,
            workspace_root: context.workspace_root.to_path_buf(),
            workspace_instructions: context.workspace_instructions.clone(),
            active_subagent: context.active_subagent.clone(),
            agent_control: context.agent_control.clone(),
        };
        let approval_request = approval_request(tool_call, &tool_context);
        let requested_access = requested_workspace_access(tool_call, context.workspace_root);
        let mut execution_workspace_access = WorkspaceAccess::WorkspaceOnly;
        let decision = match decide_tool_permission(
            context.options,
            context.mode,
            &approval_request,
            requested_access,
        ) {
            PermissionDecision::Approved { workspace_access } => {
                execution_workspace_access = workspace_access;
                ToolApprovalDecision::Approved
            }
            PermissionDecision::NeedsUserApproval { workspace_access } => {
                let decision = request_user_approval(
                    context.options,
                    &approval_request,
                    recorder.sender().clone(),
                )
                .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                decision
            }
            PermissionDecision::NeedsAiReview { workspace_access } => {
                let mut review_context = tool_context.clone();
                review_context.workspace_access = workspace_access;
                let decision = context
                    .core
                    .review_tool_call_with_ai(&approval_request, &review_context)
                    .await;
                if matches!(decision, ToolApprovalDecision::Approved) {
                    execution_workspace_access = workspace_access;
                }
                decision
            }
            PermissionDecision::Denied { reason } => ToolApprovalDecision::Denied { reason },
        };
        if is_cancelled(context.options) {
            return Vec::new();
        }

        match decision {
            ToolApprovalDecision::Approved => {
                let mut tool_context = tool_context;
                tool_context.workspace_access = execution_workspace_access;
                item.status = TimelineItemStatus::Approved;
                item.updated_at = unix_seconds();
                recorder.complete_item(item.clone());
                let tool_input = ToolInput {
                    arguments: tool_call.arguments_for_tool(),
                    session_id: context.session_id.to_string(),
                    tool_id: tool_call_id.clone(),
                };
                let lock = runtime_lock.clone();
                let tool_name = tool_call.name.clone();
                let tool_call_for_task = tool_call.clone();
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item,
                    future: Box::pin(async move {
                        let result = if supports_parallel {
                            let _guard = lock.read().await;
                            tool.execute(tool_input, tool_context).await
                        } else {
                            let _guard = lock.write().await;
                            tool.execute(tool_input, tool_context).await
                        };
                        tool_execution_record(tool_call_for_task, tool_name, result)
                    }),
                });
            }
            ToolApprovalDecision::Denied { reason } => {
                item.status = TimelineItemStatus::Denied;
                item.updated_at = unix_seconds();
                if let Some(tool) = &mut item.tool {
                    tool.denial_reason = Some(reason.clone());
                    tool.result = Some(format!("Tool execution denied: {reason}"));
                }
                recorder.complete_item(item);
                scheduled.push(ScheduledToolExecution {
                    tool_call: tool_call.clone(),
                    item: initial_items[&timeline_item_id].clone(),
                    future: Box::pin(ready_tool_execution_record(
                        tool_call.clone(),
                        format!("Tool execution denied: {reason}"),
                        TimelineItemStatus::Denied,
                        None,
                        false,
                    )),
                });
            }
        }
    }

    let mut records = Vec::new();
    let futures = scheduled
        .into_iter()
        .map(|scheduled| async move {
            let record = scheduled.future.await;
            (scheduled.tool_call, scheduled.item, record)
        })
        .collect::<Vec<_>>();
    for (_tool_call, mut item, record) in join_all(futures).await {
        item.status = record.status;
        item.updated_at = unix_seconds();
        if let Some(tool) = &mut item.tool {
            tool.result = Some(record.result.clone());
            tool.exit_code = record.exit_code;
            tool.timed_out = record.timed_out;
        }
        if item.status == TimelineItemStatus::Failed {
            recorder.fail_item(item, record.result.clone());
        } else {
            recorder.complete_item(item);
        }
        records.push(record);
    }
    records
}

fn namespaced_tool_timeline_item_id(turn_id: &str, tool_call_id: &str) -> String {
    if tool_call_id.starts_with(turn_id) {
        return tool_call_id.to_string();
    }
    format!("{turn_id}-{tool_call_id}")
}

async fn ready_tool_execution_record(
    tool_call: pl_model::ToolCall,
    result: String,
    status: TimelineItemStatus,
    exit_code: Option<i32>,
    timed_out: bool,
) -> ToolExecutionRecord {
    ToolExecutionRecord {
        call_id: tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone()),
        name: tool_call.name.clone(),
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result,
        status,
        exit_code,
        timed_out,
    }
}

fn tool_execution_record(
    tool_call: pl_model::ToolCall,
    tool_name: String,
    result: std::result::Result<ToolOutput, pl_protocol::PureError>,
) -> ToolExecutionRecord {
    let (result, status, exit_code, timed_out) = match result {
        Ok(output) => (
            output.description,
            TimelineItemStatus::Completed,
            output.exit_code,
            output.timed_out,
        ),
        Err(error) => (
            format!("Tool execution error: {error}"),
            TimelineItemStatus::Failed,
            None,
            false,
        ),
    };
    ToolExecutionRecord {
        call_id: tool_call
            .call_id
            .clone()
            .unwrap_or_else(|| tool_call.id.clone()),
        name: tool_name,
        kind: tool_call.kind(),
        arguments: serde_json::to_string(&tool_call.arguments_for_display()).unwrap_or_default(),
        result,
        status,
        exit_code,
        timed_out,
    }
}

fn tool_results_include_recoverable_subagent_capacity(
    tool_results: &[ToolExecutionRecord],
) -> bool {
    tool_results.iter().any(|tool_result| {
        tool_result.status == TimelineItemStatus::Completed
            && matches!(
                tool_result.name.as_str(),
                "subagent" | "spawn_agent" | "wait_agent" | "list_agents"
            )
            && tool_result.result.contains(RECOVERABLE_SUBAGENT_429_MARKER)
    })
}

fn provider_error_severity(
    active_subagent: Option<&SubagentContext>,
    error: &str,
) -> ErrorSeverity {
    if active_subagent.is_none() && is_provider_429_error(error) {
        ErrorSeverity::Transient
    } else {
        ErrorSeverity::Recoverable
    }
}

fn should_request_parallel_tool_calls(
    capabilities: ProviderCapabilities,
    options: &TurnOptions,
) -> bool {
    match options.tool_execution_mode {
        ToolExecutionMode::Sequential => false,
        ToolExecutionMode::Parallel => true,
        ToolExecutionMode::ModelDefault => capabilities.supports_parallel_tool_calls(),
    }
}

fn tool_allowed_in_mode(mode: crate::turn::CompileMode, name: &str) -> bool {
    match mode {
        crate::turn::CompileMode::Auto => true,
        crate::turn::CompileMode::Plan => matches!(
            name,
            "bash"
                | "read_file"
                | "list_files"
                | "search_files"
                | "stat_path"
                | "skills_list"
                | "skill_view"
                | "spawn_agent"
                | "wait_agent"
                | "list_agents"
                | "send_message"
                | "followup_task"
                | "close_agent"
                | "subagent"
        ),
    }
}

fn is_cancelled(options: &TurnOptions) -> bool {
    options
        .cancellation_token
        .as_ref()
        .is_some_and(|token| token.is_cancelled())
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn cancellation_reason() -> String {
    "interrupted by user".to_string()
}

fn budget_limit_message(kind: BudgetLimitKind, usage: &BudgetUsage) -> String {
    format!(
        "budget limited by {} budget (modelSteps={}, toolCalls={}, waitCalls={}, elapsedMs={})",
        kind.as_str(),
        usage.model_steps,
        usage.tool_calls,
        usage.wait_calls,
        usage.elapsed_ms
    )
}

#[allow(clippy::too_many_arguments)]
fn interrupted_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: crate::turn::CompileMode,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    mut usage: TokenUsage,
    session_message_count: usize,
    reason: String,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TimelineItemStatus::Interrupted);
    item.content = content.clone();
    recorder.fail_item(item, reason.clone());
    recorder.broadcast(AgentEvent::TurnInterrupted { reason });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        mode,
        session_message_count,
        status: TurnResultStatus::Aborted,
        abort_reason: Some(crate::turn::TurnAbortReason::Interrupted),
        error: None,
        budget_limit_kind: None,
        budget_usage: None,
        timeline_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
fn failed_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: crate::turn::CompileMode,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    mut usage: TokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TimelineItemStatus::Failed);
    item.content = content.clone();
    recorder.fail_item(item, error.clone());
    recorder.broadcast(AgentEvent::Error {
        message: error.clone(),
        severity,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        mode,
        session_message_count,
        status: TurnResultStatus::Errored,
        abort_reason: Some(crate::turn::TurnAbortReason::ProviderError),
        error: Some(error),
        budget_limit_kind: None,
        budget_usage: None,
        timeline_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
fn budget_limited_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: crate::turn::CompileMode,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    mut usage: TokenUsage,
    session_message_count: usize,
    limit_kind: BudgetLimitKind,
    budget_usage: BudgetUsage,
    reason: String,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TimelineItemStatus::BudgetLimited);
    item.content = content.clone();
    item.usage = Some(TokenUsageSnapshot {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        cached_prompt_tokens: usage.cached_prompt_tokens,
        total_tokens: usage.total_tokens,
    });
    recorder.fail_item(item, reason.clone());
    recorder.broadcast(AgentEvent::TurnBudgetLimited {
        reason,
        limit_kind,
        usage: budget_usage,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        mode,
        session_message_count,
        status: TurnResultStatus::Aborted,
        abort_reason: Some(crate::turn::TurnAbortReason::BudgetLimited),
        error: None,
        budget_limit_kind: Some(limit_kind),
        budget_usage: Some(budget_usage),
        timeline_events: recorder.drain(),
    }
}

fn approval_request(tool_call: &pl_model::ToolCall, context: &ToolContext) -> ToolApprovalRequest {
    let arguments = tool_call.arguments_for_display();
    let tool_arguments = tool_call.arguments_for_tool();
    let working_directory =
        get_working_directory(&tool_arguments).or_else(|| get_working_directory(&arguments));
    ToolApprovalRequest {
        id: tool_call.id.clone(),
        name: tool_call.name.clone(),
        arguments,
        working_directory,
        parent_agent_id: context
            .active_subagent
            .as_ref()
            .map(|subagent| subagent.id.clone()),
    }
}

fn get_working_directory(arguments: &serde_json::Value) -> Option<String> {
    arguments
        .get("workingDirectory")
        .or_else(|| arguments.get("working_directory"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn requested_workspace_access(
    tool_call: &pl_model::ToolCall,
    workspace_root: &Path,
) -> WorkspaceAccess {
    let arguments = tool_call.arguments_for_tool();
    let paths = requested_paths_for_tool(&tool_call.name, &arguments);
    let Some(root) = canonical_workspace_root(workspace_root) else {
        return WorkspaceAccess::ExternalAllowed;
    };
    if paths
        .iter()
        .any(|path| path_requires_external_access(path, &root))
    {
        WorkspaceAccess::ExternalAllowed
    } else {
        WorkspaceAccess::WorkspaceOnly
    }
}

fn requested_paths_for_tool(name: &str, arguments: &serde_json::Value) -> Vec<String> {
    match name {
        "bash" => get_working_directory(arguments).into_iter().collect(),
        "read_file" | "write_file" | "stat_path" | "create_directory" | "delete_path" => {
            argument_path(arguments, "path").into_iter().collect()
        }
        "list_files" | "search_files" => argument_path(arguments, "path")
            .into_iter()
            .filter(|path| !path.trim().is_empty())
            .collect(),
        "copy_path" | "move_path" => ["from", "to"]
            .into_iter()
            .filter_map(|key| argument_path(arguments, key))
            .collect(),
        "apply_patch" => arguments
            .get("patch")
            .or_else(|| arguments.get("input"))
            .and_then(serde_json::Value::as_str)
            .map(paths_from_patch_text)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn argument_path(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn paths_from_patch_text(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        let line = line.trim_start();
        for prefix in [
            "*** Add File:",
            "*** Delete File:",
            "*** Update File:",
            "*** Move to:",
        ] {
            if let Some(path) = line.strip_prefix(prefix) {
                paths.push(path.trim().to_string());
            }
        }
    }
    paths
}

fn canonical_workspace_root(workspace_root: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(workspace_root).ok()
}

fn path_requires_external_access(path: &str, workspace_root: &Path) -> bool {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return false;
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return true;
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let resolved = canonicalize_existing_or_parent(&candidate);
    !resolved.starts_with(workspace_root)
}

fn canonicalize_existing_or_parent(candidate: &Path) -> PathBuf {
    let mut current = candidate.to_path_buf();
    loop {
        if current.exists()
            && let Ok(canonical) = std::fs::canonicalize(&current)
        {
            return canonical;
        }
        let Some(parent) = current.parent() else {
            return candidate.to_path_buf();
        };
        if parent == current {
            return candidate.to_path_buf();
        }
        current = parent.to_path_buf();
    }
}

fn permission_risk_summary(tool_name: &str) -> &'static str {
    match tool_name {
        "bash" => "shell command; may execute arbitrary process actions",
        "write_file" => "file write; may create, overwrite, or append content",
        "create_directory" => "filesystem write; creates directories",
        "delete_path" => "destructive filesystem operation",
        "copy_path" => "filesystem write; copies files or directories",
        "move_path" => "filesystem write; moves or renames paths",
        "apply_patch" => "batch filesystem edit",
        "skill_manage" => "project skill write or management",
        _ => "read-only or coordination tool",
    }
}

#[cfg(test)]
async fn approve_tool_call(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
    context: &ToolContext,
) -> ToolApprovalDecision {
    match decide_tool_permission(options, context.mode, request, context.workspace_access) {
        PermissionDecision::Approved { .. } => ToolApprovalDecision::Approved,
        PermissionDecision::NeedsUserApproval { .. } => {
            request_user_approval(options, request, event_tx).await
        }
        PermissionDecision::NeedsAiReview { .. } => ToolApprovalDecision::Denied {
            reason: "AI reviewer approval requires the core execution path".to_string(),
        },
        PermissionDecision::Denied { reason } => ToolApprovalDecision::Denied { reason },
    }
}

async fn request_user_approval(
    options: &TurnOptions,
    request: &ToolApprovalRequest,
    event_tx: AgentEventSender,
) -> ToolApprovalDecision {
    let _ = event_tx.send(AgentEvent::ToolApprovalRequested {
        id: request.id.clone(),
        name: request.name.clone(),
        arguments: serde_json::to_string(&request.arguments).unwrap_or_default(),
        working_directory: request.working_directory.clone(),
    });
    match &options.tool_approval_callback {
        Some(callback) => match &options.cancellation_token {
            Some(token) => {
                tokio::select! {
                    decision = callback(request.clone()) => decision,
                    _ = token.cancelled() => ToolApprovalDecision::Denied {
                        reason: cancellation_reason(),
                    },
                }
            }
            None => callback(request.clone()).await,
        },
        None => ToolApprovalDecision::Denied {
            reason: "manual approval required but no approver is configured".to_string(),
        },
    }
}

pub(crate) fn compact_text(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let trimmed = text.trim();
    let mut result = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= MAX_CHARS {
            result.push_str("...");
            return result;
        }
        result.push(ch);
    }
    result
}

fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

fn format_instructions(base: &str, skills: Option<&str>, workspace: Option<&str>) -> String {
    let mut instructions = base.to_string();
    if let Some(content) = skills
        && !content.trim().is_empty()
    {
        instructions.push_str("\n\n");
        instructions.push_str(content);
    }
    if let Some(content) = workspace
        && !content.trim().is_empty()
    {
        instructions.push_str("\n\n# 项目记忆\n");
        instructions.push_str(content);
    }
    instructions
}

fn prompt_requires_subagent_dispatch(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let lower_without_file_mentions = lower
        .replace("subagent.rs", "")
        .replace("subagent.md", "")
        .replace("subagent.toml", "");
    let mentions_subagent = lower_without_file_mentions.contains("subagent")
        || prompt.contains("子代理")
        || prompt.contains("分代理");
    let requests_partition =
        lower.contains("crate") || prompt.contains("每个") || prompt.contains("分别");
    mentions_subagent && requests_partition
}

fn looks_like_unexecuted_tool_call_text(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();

    trimmed.contains("<｜｜DSML｜｜tool_calls>")
        || trimmed.contains("<｜｜DSML｜｜invoke name=")
        || lower.contains("<tool_call>")
        || lower.contains("<tool_calls>")
        || looks_like_json_tool_calls_text(trimmed)
}

fn looks_like_json_tool_calls_text(trimmed: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };
    has_tool_calls_shape(&value)
}

fn has_tool_calls_shape(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map
            .get("tool_calls")
            .or_else(|| map.get("toolCalls"))
            .is_some_and(|tool_calls| {
                tool_calls
                    .as_array()
                    .is_some_and(|items| has_tool_call_entries(items))
            }),
        serde_json::Value::Array(items) => has_tool_call_entries(items),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => false,
    }
}

fn has_tool_call_entries(items: &[serde_json::Value]) -> bool {
    !items.is_empty()
        && items.iter().all(|item| {
            item.as_object().is_some_and(|entry| {
                entry.contains_key("name")
                    || entry.contains_key("function")
                    || entry.contains_key("arguments")
                    || entry.contains_key("input")
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{PermissionMode, ToolApprovalPolicy};
    use crate::{ConfigStore, ModelRole};
    use pl_model::ToolCall;
    use pl_protocol::TimelineTextRole;
    use pretty_assertions::assert_eq;

    fn test_tool_context(event_tx: AgentEventSender) -> ToolContext {
        ToolContext {
            event_tx,
            options: TurnOptions::default(),
            workspace_access: WorkspaceAccess::WorkspaceOnly,
            mode: crate::turn::CompileMode::Auto,
            workspace_root: std::env::temp_dir(),
            workspace_instructions: None,
            active_subagent: None,
            agent_control: crate::AgentControl::default(),
        }
    }

    #[test]
    fn config_core_uses_planner_role_model_and_effort() {
        let config = ConfigStore::new(crate::ConfigPaths::from_home("unused"))
            .load_or_default()
            .unwrap();
        let core = PureCore::from_config(&config, ModelRole::Planner).unwrap();

        assert_eq!(core.provider.default_model(), "deepseek-v4-flash");
        assert_eq!(core.reasoning_effort.unwrap().as_str(), "high");
    }

    #[test]
    fn format_instructions_without_workspace() {
        assert_eq!(format_instructions("base", None, None), "base");
    }

    #[test]
    fn format_instructions_with_workspace() {
        assert_eq!(
            format_instructions("base", None, Some("project rules")),
            "base\n\n# 项目记忆\nproject rules"
        );
    }

    #[test]
    fn format_instructions_injects_skills_before_workspace() {
        assert_eq!(
            format_instructions("base", Some("# Skills\n- rust"), Some("project rules")),
            "base\n\n# Skills\n- rust\n\n# 项目记忆\nproject rules"
        );
    }

    #[test]
    fn format_instructions_ignores_empty_workspace() {
        assert_eq!(format_instructions("base", None, Some("")), "base");
        assert_eq!(format_instructions("base", None, Some("   ")), "base");
    }

    #[test]
    fn detects_explicit_subagent_partition_requests() {
        assert!(prompt_requires_subagent_dispatch(
            "每个 crate 分一个 subagent 探索，然后介绍整个项目"
        ));
        assert!(prompt_requires_subagent_dispatch(
            "请分别用子代理探索前端和后端"
        ));
        assert!(!prompt_requires_subagent_dispatch("介绍整个项目"));
        assert!(!prompt_requires_subagent_dispatch(
            "用 bash 看一下每个 crate"
        ));
        assert!(!prompt_requires_subagent_dispatch(
            "读取 src/tool/subagent.rs，并总结每个模块的职责"
        ));
    }

    #[test]
    fn subagent_dispatch_instructions_describe_recoverable_429() {
        assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("429"));
        assert!(SUBAGENT_DISPATCH_CONSTRAINT.contains("recoverableSubagentProvider429"));
        assert!(SUBAGENT_FORCE_DISPATCH_INSTRUCTION.contains("429"));
    }

    #[test]
    fn detects_recoverable_subagent_tool_result_marker() {
        let records = vec![
            ToolExecutionRecord {
                call_id: "call-1".to_string(),
                name: "subagent".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: retry locally".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
            },
            ToolExecutionRecord {
                call_id: "call-2".to_string(),
                name: "bash".to_string(),
                kind: ToolCallKind::Function,
                arguments: "{}".to_string(),
                result: "recoverableSubagentProvider429: unrelated text".to_string(),
                status: TimelineItemStatus::Completed,
                exit_code: None,
                timed_out: false,
            },
        ];

        assert!(tool_results_include_recoverable_subagent_capacity(&records));
        assert!(!tool_results_include_recoverable_subagent_capacity(
            &records[1..]
        ));
    }

    #[test]
    fn plan_mode_tool_allowlist_exposes_only_read_and_agent_tools() {
        let auto = crate::turn::CompileMode::Auto;
        let plan = crate::turn::CompileMode::Plan;

        assert!(tool_allowed_in_mode(auto, "write_file"));
        assert!(tool_allowed_in_mode(plan, "read_file"));
        assert!(tool_allowed_in_mode(plan, "search_files"));
        assert!(tool_allowed_in_mode(plan, "skills_list"));
        assert!(tool_allowed_in_mode(plan, "skill_view"));
        assert!(tool_allowed_in_mode(plan, "spawn_agent"));
        assert!(tool_allowed_in_mode(plan, "followup_task"));
        assert!(tool_allowed_in_mode(plan, "subagent"));
        assert!(tool_allowed_in_mode(plan, "bash"));
        assert!(!tool_allowed_in_mode(plan, "write_file"));
        assert!(!tool_allowed_in_mode(plan, "apply_patch"));
        assert!(!tool_allowed_in_mode(plan, "delete_path"));
        assert!(!tool_allowed_in_mode(plan, "skill_manage"));
    }

    #[test]
    fn tool_timeline_item_ids_are_scoped_to_turn() {
        assert_eq!(
            namespaced_tool_timeline_item_id("turn-1", "call_0"),
            "turn-1-call_0"
        );
        assert_eq!(
            namespaced_tool_timeline_item_id("turn-1", "turn-1-call_0"),
            "turn-1-call_0"
        );
    }

    #[test]
    fn root_provider_429_is_transient_but_subagent_provider_429_stays_recoverable() {
        assert!(matches!(
            provider_error_severity(None, "API error 429 Too Many Requests"),
            ErrorSeverity::Transient
        ));

        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: None,
            agent_path: Some("/root/worker".to_string()),
            role: "executor".to_string(),
            task: "inspect worker".to_string(),
            depth: 1,
        };
        assert!(matches!(
            provider_error_severity(Some(&subagent), "API error 429 Too Many Requests"),
            ErrorSeverity::Recoverable
        ));
        assert!(matches!(
            provider_error_severity(None, "API error 500"),
            ErrorSeverity::Recoverable
        ));
    }

    #[test]
    fn detects_unexecuted_tool_call_text() {
        assert!(looks_like_unexecuted_tool_call_text(
            "<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"spawn_agent\">"
        ));
        assert!(looks_like_unexecuted_tool_call_text(
            r#"{"tool_calls":[{"name":"spawn_agent"}]}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "源码中有 tool_calls 字段、name 字段和 subagent.rs 文件。"
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            r#"{"summary":"tool_calls and name are discussed in docs"}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "已完成探索，没有工具调用文本。"
        ));
    }

    #[test]
    fn failed_turn_result_preserves_error_message() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);

        let result = failed_turn_result(
            &mut recorder,
            "turn-1",
            crate::turn::CompileMode::Auto,
            "partial summary".to_string(),
            None,
            "model-a".to_string(),
            TokenUsage::default(),
            3,
            "provider rejected request".to_string(),
            ErrorSeverity::Transient,
        );

        assert_eq!(result.status, TurnResultStatus::Errored);
        assert_eq!(
            result.abort_reason,
            Some(crate::turn::TurnAbortReason::ProviderError),
        );
        assert_eq!(result.content, "partial summary");
        assert_eq!(result.error.as_deref(), Some("provider rejected request"));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { item }
                if item.item_id == "turn-1-assistant"
                    && item.role == Some(TimelineTextRole::Assistant)
                    && item.content == "partial summary"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemCompleted { item, .. }
                if item.item_id == "turn-1-assistant"
                    && item.role == Some(TimelineTextRole::Assistant)
                    && item.content == "partial summary"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemFailed { item, .. } if item.item_id == "turn-1-turn"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::Error {
                severity: ErrorSeverity::Transient,
                ..
            }
        ));
        assert!(matches!(event_rx.try_recv().unwrap(), AgentEvent::Done));
    }

    #[test]
    fn default_turn_options_auto_allow_tools() {
        let options = TurnOptions::default();

        assert_eq!(options.tool_approval_policy, ToolApprovalPolicy::AutoAllow);
        assert_eq!(options.permission_mode, PermissionMode::RequestApproval);
        assert!(options.tool_approval_callback.is_none());
    }

    #[tokio::test]
    async fn manual_tool_approval_can_approve() {
        let options = TurnOptions::manual(std::sync::Arc::new(|request| {
            Box::pin(async move {
                assert_eq!(request.name, "bash");
                ToolApprovalDecision::Approved
            })
        }));
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;
        let event = event_rx.recv().await.unwrap();

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(matches!(
            event,
            AgentEvent::ToolApprovalRequested {
                id,
                name,
                ..
            } if id == "call-1" && name == "bash"
        ));
    }

    #[tokio::test]
    async fn plan_mode_bash_requires_manual_approval_even_when_auto_allowed() {
        let options = TurnOptions::default();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "pwd"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx.clone());
        context.mode = crate::turn::CompileMode::Plan;

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "manual approval required but no approver is configured".to_string()
            }
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::ToolApprovalRequested {
                id,
                name,
                ..
            } if id == "call-1" && name == "bash"
        ));
    }

    #[tokio::test]
    async fn plan_mode_read_tool_still_uses_auto_allow() {
        let options = TurnOptions::default();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "Cargo.toml"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx.clone());
        context.mode = crate::turn::CompileMode::Plan;

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(decision, ToolApprovalDecision::Approved);
        assert!(event_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn plan_mode_denies_disallowed_tool_before_execution_even_with_full_access() {
        let core = PureCore::default_provider().unwrap();
        let tool_call = ToolCall::function(
            "call-1",
            "write_file",
            serde_json::json!({"path": "a.txt", "content": "oops"}),
            None,
        );
        let options = TurnOptions::default().with_permission_mode(PermissionMode::FullAccess);
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));
        let workspace_root = std::env::temp_dir();

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &options,
                mode: crate::turn::CompileMode::Plan,
                session_id: "turn-1",
                workspace_root: &workspace_root,
                workspace_instructions: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
            },
        )
        .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Denied);
        assert_eq!(records[0].name, "write_file");
        assert_eq!(records[0].result, "Tool disabled in plan mode: write_file");
    }

    #[tokio::test]
    async fn request_approval_allows_external_path_after_user_approval() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let workspace_root =
            std::env::temp_dir().join(format!("pure-permission-workspace-{unique}"));
        let outside_root = std::env::temp_dir().join(format!("pure-permission-outside-{unique}"));
        tokio::fs::create_dir_all(&workspace_root).await.unwrap();
        tokio::fs::create_dir_all(&outside_root).await.unwrap();
        let outside_file = outside_root.join("note.txt");
        tokio::fs::write(&outside_file, "external ok")
            .await
            .unwrap();
        let mut core = PureCore::default_provider().unwrap();
        core.register_tool(ReadFileTool::new());
        let tool_call = ToolCall::function(
            "call-1",
            "read_file",
            serde_json::json!({"path": outside_file.to_string_lossy()}),
            None,
        );
        let mut options = TurnOptions::default();
        options.tool_approval_callback = Some(std::sync::Arc::new(|request| {
            Box::pin(async move {
                assert_eq!(request.name, "read_file");
                ToolApprovalDecision::Approved
            })
        }));
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(16);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let mut budget = BudgetTracker::new(crate::turn::TurnBudget::new(60_000));

        let records = execute_tool_calls(
            &[tool_call],
            &mut budget,
            &mut recorder,
            ToolExecutionContext {
                core: &core,
                options: &options,
                mode: crate::turn::CompileMode::Auto,
                session_id: "turn-1",
                workspace_root: &workspace_root,
                workspace_instructions: None,
                active_subagent: None,
                agent_control: crate::AgentControl::default(),
            },
        )
        .await;

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, TimelineItemStatus::Completed);
        assert!(records[0].result.contains("external ok"));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TimelineItemStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::ToolApprovalRequested { name, .. } if name == "read_file"
        ));
        let _ = tokio::fs::remove_dir_all(workspace_root).await;
        let _ = tokio::fs::remove_dir_all(outside_root).await;
    }

    #[tokio::test]
    async fn deny_all_tool_approval_denies_without_request_event() {
        let options = TurnOptions::deny_all();
        let request = ToolApprovalRequest {
            id: "call-1".to_string(),
            name: "bash".to_string(),
            arguments: serde_json::json!({"command": "echo hi"}),
            working_directory: None,
            parent_agent_id: None,
        };
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let context = test_tool_context(event_tx.clone());

        let decision = approve_tool_call(&options, &request, event_tx, &context).await;

        assert_eq!(
            decision,
            ToolApprovalDecision::Denied {
                reason: "tool execution denied by policy".to_string()
            }
        );
        assert!(event_rx.try_recv().is_err());
    }

    #[test]
    fn approval_request_extracts_working_directory() {
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({
                "command": "pwd",
                "workingDirectory": "C:/work"
            }),
            None,
        );

        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let request = approval_request(&call, &test_tool_context(event_tx));

        assert_eq!(request.working_directory.as_deref(), Some("C:/work"));
    }

    #[test]
    fn approval_request_marks_parent_agent() {
        let call = ToolCall::function(
            "call-1",
            "bash",
            serde_json::json!({"command": "pwd"}),
            None,
        );
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut context = test_tool_context(event_tx);
        context.active_subagent = Some(SubagentContext {
            id: "subagent-1".to_string(),
            parent_id: None,
            agent_path: None,
            role: "executor".to_string(),
            task: "inspect".to_string(),
            depth: 1,
        });

        let request = approval_request(&call, &context);

        assert_eq!(request.parent_agent_id.as_deref(), Some("subagent-1"));
    }

    #[test]
    fn default_tools_register_bash_and_subagent() {
        let mut core = PureCore::default_provider().unwrap();

        core.register_default_tools(std::env::temp_dir(), Some("rules".to_string()));

        assert!(core.tools.get("bash").is_some());
        assert!(core.tools.get("subagent").is_some());
        assert!(core.tools.get("read_file").is_some());
        assert!(core.tools.get("apply_patch").is_some());
    }
}
