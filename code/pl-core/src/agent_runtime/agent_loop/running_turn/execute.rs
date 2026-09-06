use pl_protocol::{ThreadNotification, TurnOutcome, TurnRolloverOutcome};
use pl_trace::{AgentEvent, TraceEvent, TracePartKind};
use tokio_util::sync::CancellationToken;

use crate::agent_runtime::host::AgentTurnFactory;
use crate::agent_runtime::state::unix_timestamp;
use crate::agent_runtime::*;
use crate::thread_event::{ObservedTurnEvent, observation_from_agent_event};
use crate::{
    AgentSession, ContextCompactionTrigger, ManualContextCompactionRequest, TraceRecorder,
};

use super::outcome::{TurnWorkerOutcome, enforce_finalization};

pub(crate) struct TurnCompletion {
    pub(crate) turn_id: TurnId,
    pub(crate) identity: std::sync::Arc<()>,
    pub(crate) start_revision: u64,
    pub(crate) session: TurnSessionDisposition,
    pub(crate) worker_outcome: TurnWorkerOutcome,
    pub(crate) next_trace_sequence: u64,
}

pub(crate) enum TurnSessionDisposition {
    Preserve,
    Replace(AgentSession),
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
        context.session.push_user_prompt_with_presentation(
            input.payload.message.clone(),
            input.payload.presentation,
        );
    }
    let input_presentation = context.input.payload.presentation;
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
    let is_child = context.snapshot.identity.parent_id.is_some();
    let mut session = context.session.clone();
    let (result, session_commit) = match host.turn_factory().prepare_turn(context).await {
        Ok(mut prepared) => {
            prepared.request.user_presentation = input_presentation;
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
            if let Some(workflow) = prepared.initial_workflow.clone() {
                session.replace_workflow(Some(workflow));
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
                let event_task =
                    tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
                        loop {
                            match event_rx.recv().await {
                                Ok(event) => {
                                    if let Some(observation) = observation_from_agent_event(&event)
                                    {
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
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    continue;
                                }
                            }
                        }
                    }));
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
                let rollover_checkpoint = prepared.options.checkpoint.clone();
                let mut rollover_commit_error = None;
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
                    && !is_child
                    && matches!(
                        &turn_result.outcome,
                        TurnOutcome::BudgetLimited(outcome)
                            if outcome.limit().kind == pl_protocol::BudgetLimitKind::WallClock
                    )
                {
                    let before_rollover = session.clone();
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
                    let accounting = match &rollover {
                        Ok(Some(snapshot)) => Some(snapshot.accounting.clone()),
                        Ok(None) => None,
                        Err(failure) => Some((*failure.accounting).clone()),
                    };
                    match rollover {
                        Ok(snapshot) => {
                            if let TurnOutcome::BudgetLimited(outcome) = &mut turn_result.outcome {
                                outcome.replace_rollover(TurnRolloverOutcome::Succeeded);
                            }
                            turn_result.context_compactions.extend(snapshot);
                        }
                        Err(error) => {
                            if let TurnOutcome::BudgetLimited(outcome) = &mut turn_result.outcome {
                                outcome.replace_rollover(TurnRolloverOutcome::Failed {
                                    error: error.to_string(),
                                });
                            }
                        }
                    }
                    if let Some(accounting) = accounting {
                        turn_result.usage.merge(&accounting.usage.totals());
                        let runtime = prepared.engine.model_runtime();
                        let billing = pl_protocol::InferenceBillingRecord {
                            inference_id: format!("{turn_id}-rollover"),
                            provider_instance_id: runtime.provider_instance_id().to_owned(),
                            provider: runtime.endpoint().name.clone(),
                            model: runtime.model().slug.clone(),
                            context_window: runtime.model().resolved_context_window(),
                            accounting,
                            prompt_generation: None,
                            prompt_cache_policy: None,
                            prefix_changed_reason: None,
                            orchestration: Default::default(),
                            timing: None,
                            recorded_at: unix_timestamp(),
                        };
                        if let Err(error) = turn_result.billing.append(billing.clone()) {
                            rollover_commit_error = Some(error);
                        }
                        let runtime_delta = crate::runtime_usage::agent_runtime_delta(
                            crate::runtime_usage::identity_for_subagent(None),
                            &billing,
                        );
                        if let Some(checkpoint) = &rollover_checkpoint {
                            if let Err(error) = checkpoint
                                .commit_inference(
                                    before_rollover,
                                    AgentInferenceCommit {
                                        billing,
                                        runtime_delta,
                                    },
                                )
                                .await
                            {
                                rollover_commit_error = Some(error.to_string());
                            }
                        } else {
                            recorder.broadcast(AgentEvent::AgentRuntimeUpdated {
                                delta: runtime_delta,
                            });
                        }
                    }
                    turn_result.trace_events.extend(recorder.drain());
                }
                if let Some(error) = rollover_commit_error {
                    result = Err(error);
                }
                drop(recorder);
                let _ = event_task.await;
                break 'execute result;
            };
            enforce_finalization(&mut result, &policy);
            (result, session_commit)
        }
        Err(error) => (Err(error.to_string()), AgentSessionCommitPolicy::Persist),
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
            AgentSessionCommitPolicy::Persist => TurnSessionDisposition::Replace(session),
            AgentSessionCommitPolicy::DiscardTurn => TurnSessionDisposition::Preserve,
        },
        worker_outcome: result.into(),
        next_trace_sequence,
    }
}

fn activity_for_event(event: &AgentEvent) -> Option<AgentActivityUpdate> {
    match event {
        AgentEvent::TracePartStarted { item } => match item.kind() {
            TracePartKind::Tool | TracePartKind::Agent => Some(AgentActivityUpdate::WaitingTool),
            TracePartKind::Text
            | TracePartKind::Thinking
            | TracePartKind::Turn
            | TracePartKind::Inference => Some(AgentActivityUpdate::Running),
        },
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. } => Some(AgentActivityUpdate::Running),
        AgentEvent::InteractionChanged { event } => match event.interaction.status() {
            pl_protocol::InteractionStatus::Pending => {
                Some(AgentActivityUpdate::WaitingInteraction {
                    interaction_id: event.interaction.interaction_id.clone(),
                })
            }
            pl_protocol::InteractionStatus::Resolved
            | pl_protocol::InteractionStatus::Cancelled
            | pl_protocol::InteractionStatus::Expired => Some(AgentActivityUpdate::Running),
        },
        AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => None,
    }
}
