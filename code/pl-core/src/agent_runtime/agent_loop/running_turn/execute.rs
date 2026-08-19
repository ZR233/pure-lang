use pl_protocol::{BudgetLimitKind, ThreadNotification};
use pl_trace::{AgentEvent, TraceEvent, TracePartKind};
use tokio_util::sync::CancellationToken;

use crate::agent_runtime::host::AgentTurnFactory;
use crate::agent_runtime::state::unix_timestamp;
use crate::agent_runtime::*;
use crate::thread_event::{ObservedTurnEvent, observation_from_agent_event};
use crate::{
    AgentSession, ContextCompactionTrigger, ManualContextCompactionRequest, TraceRecorder,
    TurnResult,
};

use super::outcome::enforce_finalization;

pub(crate) struct TurnCompletion {
    pub(crate) turn_id: TurnId,
    pub(crate) identity: std::sync::Arc<()>,
    pub(crate) start_revision: u64,
    /// worker 正常返回时携带更新后的 session；任务被 abort 时必须保留 actor 中的原 session。
    pub(crate) session: Option<AgentSession>,
    pub(crate) result: std::result::Result<TurnResult, String>,
    pub(crate) finalized_with_tool: Option<String>,
    pub(crate) cancelled: bool,
    pub(crate) next_trace_sequence: u64,
}

pub(crate) async fn execute_turn<H>(
    host: H,
    mut context: AgentTurnPreparationContext,
    identity: std::sync::Arc<()>,
    cancellation: CancellationToken,
    durable_trace_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
    observation_tx: tokio::sync::mpsc::UnboundedSender<ObservedTurnEvent>,
) -> TurnCompletion
where
    H: AgentRuntimeHost,
{
    let leading_inputs = context.leading_inputs.clone();
    for input in &leading_inputs {
        context
            .session
            .push_user_prompt(input.payload.message.clone());
    }
    let turn_id = context.turn_id.clone();
    let start_revision = context.snapshot.revision;
    let framework_thread_id = context.thread_id.clone();
    let trace_thread_id = context.thread_id.to_string();
    let initial_trace_sequence = context.trace_sequence;
    let activity_runtime = context.runtime.clone();
    let activity_agent_id = context.snapshot.identity.id.clone();
    let checkpoint = AgentTurnCheckpointHandle::new(
        context.runtime.clone(),
        context.snapshot.identity.id.clone(),
        context.turn_id.clone(),
        context.thread_id.clone(),
    );
    let mailbox = context.mailbox.clone();
    let budget_refresh = context.budget_refresh.clone();
    let mut session = context.session.clone();
    let (result, session_commit, finalized_with_tool) = match host
        .turn_factory()
        .prepare_turn(context)
        .await
    {
        Ok(prepared) => {
            let prepared = prepared.with_runtime_context(
                &turn_id,
                cancellation.clone(),
                checkpoint,
                mailbox,
                budget_refresh,
            );
            for section in &prepared.pinned_context {
                session.upsert_pinned_context(section.clone());
            }
            let policy = prepared.policy.clone();
            let session_commit = prepared.session_commit;
            let context_compaction_control = prepared.options.context_compaction_control();
            let session_runtime_result = if let Some(runtime) = &prepared.session_runtime {
                match activity_runtime.thread_snapshot(&framework_thread_id) {
                    Ok(current) => {
                        let updated_at = unix_timestamp();
                        let snapshot =
                            runtime.merge_with(&framework_thread_id, &current, updated_at);
                        activity_runtime
                            .record_thread_facts(
                                activity_agent_id.clone(),
                                framework_thread_id.clone(),
                                vec![crate::ThreadNotificationFact::durable(
                                    updated_at,
                                    ThreadNotification::ThreadRuntimeUpdated {
                                        runtime: Box::new(snapshot),
                                    },
                                )],
                            )
                            .await
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            let mut result = 'execute: {
                if let Err(error) = session_runtime_result {
                    break 'execute Err(error.to_string());
                }
                let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(128);
                let activity_turn_id = turn_id.clone();
                let observation_turn_id = turn_id.to_string();
                let observation_thread_id = trace_thread_id.clone();
                let event_task = tokio::spawn(async move {
                    loop {
                        match event_rx.recv().await {
                            Ok(event) => {
                                if let Some(observation) = observation_from_agent_event(&event) {
                                    let _ = observation_tx.send(ObservedTurnEvent {
                                        turn_id: observation_turn_id.clone(),
                                        thread_id: observation_thread_id.clone(),
                                        observation,
                                    });
                                }
                                if let Some(activity) = activity_for_event(&event) {
                                    let _ = activity_runtime
                                        .set_activity(
                                            activity_agent_id.clone(),
                                            activity_turn_id.clone(),
                                            activity,
                                        )
                                        .await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
                let mut recorder = TraceRecorder::streaming(
                    trace_thread_id,
                    event_tx,
                    initial_trace_sequence,
                    durable_trace_tx,
                );
                for input in &leading_inputs {
                    recorder.user_text_item_with_id(
                        turn_id.as_str(),
                        format!("{turn_id}-mail-{}", input.mail_id),
                        input.payload.message.clone(),
                        Vec::new(),
                    );
                }
                let mut result = prepared
                    .engine
                    .run_turn_with_trace(
                        &mut session,
                        prepared.request,
                        &mut recorder,
                        prepared.options,
                    )
                    .await
                    .map_err(|error| error.to_string());
                if let Ok(turn_result) = &mut result
                    && turn_result.budget_limit_kind == Some(BudgetLimitKind::WallClock)
                {
                    let rollover = prepared
                        .engine
                        .compact_session_with_trace_control(
                            &mut session,
                            ManualContextCompactionRequest {
                                turn_id: Some(turn_id.to_string()),
                                execution_policy: Some(policy.clone()),
                                trigger: ContextCompactionTrigger::WallClockRollover,
                                ..ManualContextCompactionRequest::default()
                            },
                            &mut recorder,
                            context_compaction_control,
                        )
                        .await;
                    match rollover {
                        Ok(snapshot) => {
                            turn_result.rollover_compacted = true;
                            turn_result.context_compactions.extend(snapshot);
                        }
                        Err(error) => {
                            turn_result.rollover_compaction_error = Some(error.to_string());
                        }
                    }
                    turn_result.trace_events.extend(recorder.drain());
                }
                drop(recorder);
                let _ = event_task.await;
                break 'execute result;
            };
            let finalized_with_tool = enforce_finalization(&mut result, &policy);
            (result, session_commit, finalized_with_tool)
        }
        Err(error) => (
            Err(error.to_string()),
            AgentSessionCommitPolicy::Persist,
            None,
        ),
    };
    let next_trace_sequence = result
        .as_ref()
        .ok()
        .and_then(|result| result.trace_events.iter().map(|event| event.sequence).max())
        .map(|sequence| sequence.saturating_add(1))
        .unwrap_or(initial_trace_sequence);
    TurnCompletion {
        turn_id,
        identity,
        start_revision,
        session: match session_commit {
            AgentSessionCommitPolicy::Persist => Some(session),
            AgentSessionCommitPolicy::DiscardTurn => None,
        },
        result,
        finalized_with_tool,
        cancelled: cancellation.is_cancelled(),
        next_trace_sequence,
    }
}

fn activity_for_event(event: &AgentEvent) -> Option<ActiveKind> {
    match event {
        AgentEvent::TracePartStarted { item } => match item.kind {
            TracePartKind::Tool | TracePartKind::Agent => Some(ActiveKind::WaitingTool),
            TracePartKind::Text
            | TracePartKind::Thinking
            | TracePartKind::Turn
            | TracePartKind::Inference
            | TracePartKind::Plan => Some(ActiveKind::Running),
        },
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. } => Some(ActiveKind::Running),
        AgentEvent::InteractionChanged { event } => match event.interaction.status {
            pl_protocol::InteractionStatus::Pending => Some(ActiveKind::WaitingInteraction),
            pl_protocol::InteractionStatus::Resolved
            | pl_protocol::InteractionStatus::Cancelled
            | pl_protocol::InteractionStatus::Expired => Some(ActiveKind::Running),
        },
        AgentEvent::AgentStateChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::SubAgentActivity { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => None,
    }
}
