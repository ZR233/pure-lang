use pl_protocol::AgentStatus;

use super::events::emit_agent_record;
use super::types::AgentRunConfig;
use crate::agent::AgentStatusUpdate;
use crate::config::ModelRole;
use crate::core::compact_text;
use crate::session::CoreSession;
use crate::turn::{TurnAbortReason, TurnResultStatus};
use crate::{PureCore, SubagentContext};

pub(super) async fn mark_agent_failed(config: &AgentRunConfig, error: String) {
    if let Some(record) = config
        .agent_control
        .update_status_with(
            &config.agent_id,
            AgentStatusUpdate {
                status: AgentStatus::Errored,
                summary: None,
                error: Some(error),
                reason: Some("errored".to_string()),
                budget_limit_kind: None,
                budget_usage: None,
            },
        )
        .await
    {
        emit_agent_record(&config.event_tx, &record);
    }
    for record in config
        .agent_control
        .shutdown_descendants(&config.agent_id, "errored")
        .await
    {
        emit_agent_record(&config.event_tx, &record);
    }
}

pub(crate) async fn run_agent_turn(config: AgentRunConfig) {
    let Some(record) = config
        .agent_control
        .update_status(&config.agent_id, AgentStatus::Running, None, None)
        .await
    else {
        return;
    };
    emit_agent_record(&config.event_tx, &record);

    let role = ModelRole::from_key(&config.role).unwrap_or(ModelRole::Executor);
    let core_result = match &config.config {
        Some(pure_config) => PureCore::from_config(pure_config, role),
        None => Ok(match &config.reasoning_effort {
            Some(effort) => {
                PureCore::with_reasoning_effort(config.provider.clone(), effort.clone())
            }
            None => PureCore::new(config.provider.clone()),
        }),
    };
    let mut core = match core_result {
        Ok(core) => {
            let mut core = core
                .with_mcp_runtime(config.mcp_runtime.clone().unwrap_or_default())
                .with_agent_control(config.agent_control.clone())
                .with_subagent_context(SubagentContext {
                    id: config.agent_id.clone(),
                    parent_id: record.parent_path.clone(),
                    agent_path: Some(config.agent_path.clone()),
                    role: config.role.clone(),
                    task: compact_text(&config.message),
                    depth: record.depth,
                });
            if let Some(lsp_runtime) = config.lsp_runtime.clone() {
                core = core.with_lsp_runtime(lsp_runtime);
            }
            core
        }
        Err(error) => {
            mark_agent_failed(&config, error.to_string()).await;
            return;
        }
    };
    core.register_default_tools(
        config.workspace_root.clone(),
        config.workspace_instructions.clone(),
    );
    if let Err(error) = core.register_configured_mcp_tools().await {
        mark_agent_failed(&config, error.to_string()).await;
        return;
    }

    let mut session = config
        .agent_control
        .load_session(&config.agent_id)
        .await
        .unwrap_or_else(CoreSession::new);
    let mut request = crate::turn::TurnRequest::new(config.message.clone(), config.mode)
        .with_budget(config.budget);
    if let Some(instructions) = config.workspace_instructions.clone() {
        request = request.with_workspace_instructions(instructions);
    }
    if let Some(snapshot) = config.instruction_snapshot.clone() {
        request = request.with_instruction_snapshot(snapshot);
    }
    let (agent_event_tx, agent_event_rx) = tokio::sync::broadcast::channel(256);
    let forward_task = tokio::spawn(super::events::forward_agent_lifecycle_events(
        agent_event_rx,
        config.event_tx.clone(),
    ));
    let result = core
        .run_turn_with_options(
            &mut session,
            request,
            agent_event_tx.clone(),
            config.options.clone(),
        )
        .await;
    drop(agent_event_tx);
    let _ = forward_task.await;
    config
        .agent_control
        .store_session(&config.agent_id, session)
        .await;

    match result {
        Ok(result) => {
            let status = match result.status {
                TurnResultStatus::Completed => AgentStatus::Completed,
                TurnResultStatus::Aborted => AgentStatus::Interrupted,
                TurnResultStatus::Errored => AgentStatus::Errored,
            };
            let summary = result.content.trim().to_string();
            let reason = result
                .abort_reason
                .map(|reason| reason.as_str().to_string());
            let error = match result.status {
                TurnResultStatus::Aborted
                    if matches!(result.abort_reason, Some(TurnAbortReason::BudgetLimited)) =>
                {
                    result
                        .error
                        .clone()
                        .or_else(|| Some("subagent budget limited".to_string()))
                }
                TurnResultStatus::Errored => result
                    .error
                    .clone()
                    .or_else(|| Some("subagent errored".to_string())),
                TurnResultStatus::Completed | TurnResultStatus::Aborted => result.error.clone(),
            };
            if let Some(record) = config
                .agent_control
                .update_status_with(
                    &config.agent_id,
                    AgentStatusUpdate {
                        status,
                        summary: (!summary.is_empty()).then_some(summary.clone()),
                        error,
                        reason,
                        budget_limit_kind: result.budget_limit_kind,
                        budget_usage: result.budget_usage,
                    },
                )
                .await
            {
                emit_agent_record(&config.event_tx, &record);
            }
            if !matches!(result.status, TurnResultStatus::Completed) {
                let reason = result
                    .abort_reason
                    .map(|reason| reason.as_str())
                    .unwrap_or("errored");
                for record in config
                    .agent_control
                    .shutdown_descendants(&config.agent_id, reason)
                    .await
                {
                    emit_agent_record(&config.event_tx, &record);
                }
            }
        }
        Err(error) => {
            mark_agent_failed(&config, error.to_string()).await;
        }
    }
}
