//! Frozen model preparation, admission, retry and output commit.
use super::*;

impl Owner {
    pub(super) async fn replace_model(
        &mut self,
        factory: crate::model::ModelFactory,
    ) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        if !self.uncommitted_tools.is_empty() {
            return Err(ThreadError::PendingToolCommit);
        }
        if let Some(mut previous) = self.model.take()
            && let Err(error) = self.await_with_mailbox(previous.close()).await
        {
            self.model = Some(previous);
            self.publish_snapshot();
            return Err(ThreadError::Model(Arc::new(error)));
        }
        self.retry_plan = None;
        self.state.private_context = None;
        self.publish();
        self.publish_snapshot();
        if self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        let opened = self.await_with_mailbox(factory.open_session()).await;
        self.model = Some(opened.map_err(|error| ThreadError::Model(Arc::new(error)))?);
        self.publish_snapshot();
        if self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        Ok(())
    }

    pub(super) async fn retry_attempt(
        &mut self,
        source: String,
        attempt: String,
        cancellation: CancellationToken,
    ) -> Result<ModelStepOutput, ThreadError> {
        let previous = self
            .state
            .attempts
            .last()
            .filter(|previous| previous.attempt_id == source)
            .ok_or(ThreadError::InvalidIdentity)?;
        if !matches!(
            previous.outcome,
            AttemptOutcome::Failed(_) | AttemptOutcome::Cancelled { .. }
        ) || previous.input != self.state.context
        {
            return Err(ThreadError::InvalidContext);
        }
        let (id, plan) = self
            .retry_plan
            .as_ref()
            .ok_or(ThreadError::InvalidIdentity)?;
        if id != &source || !plan.retry_compatible(&self.tools.freeze()) {
            return Err(ThreadError::InvalidContext);
        }
        let plan = plan.clone();
        let input = StepInput {
            turn_id: previous.turn_id.clone(),
            attempt_id: attempt,
            content: Vec::new(),
            cancellation,
        };
        self.step_with_plan(input, plan, Some(source)).await
    }

    pub(super) async fn step(&mut self, input: StepInput) -> Result<ModelStepOutput, ThreadError> {
        let plan = self.tools.freeze();
        self.step_with_plan(input, plan, None).await
    }

    pub(super) async fn correct_step(
        &mut self,
        input: StepInput,
        source: String,
    ) -> Result<ModelStepOutput, ThreadError> {
        let plan = self.tools.freeze();
        self.step_with_plan(input, plan, Some(source)).await
    }

    fn output_violation(
        &self,
        attempt_id: &str,
        revision: u64,
        plan: &crate::tool::opaque::ToolPlan,
        output: &ModelStepOutput,
    ) -> Option<ModelOutputViolation> {
        if output.attempt_id != attempt_id {
            return Some(ModelOutputViolation::AttemptIdentity {
                expected: attempt_id.into(),
                actual: output.attempt_id.clone(),
            });
        }
        if output.base_context_revision != revision {
            return Some(ModelOutputViolation::ContextRevision {
                expected: revision,
                actual: output.base_context_revision,
            });
        }
        let mut ids = std::collections::BTreeSet::new();
        for call in &output.tool_calls {
            if call.call_id.is_empty() {
                return Some(ModelOutputViolation::EmptyCallIdentity);
            }
            if plan.get(&call.tool_id).is_none() {
                return Some(ModelOutputViolation::UnknownTool {
                    tool_id: call.tool_id.clone(),
                    call_id: call.call_id.clone(),
                });
            }
            // Call identity is global to the Thread: a call already admitted by the current context,
            // by a still-resident attempt or by the live ledger must never be re-declared. The
            // current context is the bounded current fact set, not the whole history.
            if !ids.insert(&call.call_id)
                || self.state.live_calls.contains_key(&call.call_id)
                || self.state.context.records.iter().any(|record| {
                    record
                        .tool_calls
                        .iter()
                        .any(|old| old.call_id == call.call_id)
                })
                || self.state.attempts.iter().any(|attempt| {
                    matches!(
                        &attempt.outcome,
                        AttemptOutcome::Committed(previous)
                            if previous.tool_calls.iter().any(|old| old.call_id == call.call_id)
                    )
                })
            {
                return Some(ModelOutputViolation::DuplicateCallIdentity {
                    call_id: call.call_id.clone(),
                });
            }
        }
        let solo = output
            .tool_calls
            .iter()
            .filter(|call| {
                plan.get(&call.tool_id)
                    .is_some_and(|tool| tool.requires_solo_call())
            })
            .map(|call| call.tool_id.clone())
            .collect::<Vec<_>>();
        (output.tool_calls.len() > 1 && !solo.is_empty())
            .then_some(ModelOutputViolation::SoloBatch { tool_ids: solo })
    }

    async fn ensure_model_admission(
        &mut self,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<(), ThreadError> {
        if !self.uncommitted_tools.is_empty() {
            return Err(ThreadError::PendingToolCommit);
        }
        self.await_storage_admission(cancellation).await?;
        self.publish_snapshot();
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if !self
            .model
            .as_ref()
            .is_some_and(DynModelSession::is_available)
        {
            return Err(ThreadError::ModelUnavailable);
        }
        if self
            .state
            .interactions
            .values()
            .any(|record| record.state == interactions::InteractionState::Pending)
        {
            return Err(ThreadError::PendingInteraction);
        }
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        Ok(())
    }

    async fn step_with_plan(
        &mut self,
        input: StepInput,
        plan: crate::tool::opaque::ToolPlan,
        retry_of: Option<String>,
    ) -> Result<ModelStepOutput, ThreadError> {
        // The explicit execution phase describes live execution only: clear it on every exit path
        // (success, failure, cancellation and error) so a projection never reads a phase left over
        // from a finished step.
        let attempt_id = input.attempt_id.clone();
        let result = self.run_step(input, plan, retry_of).await;
        // The call is over on every exit path — success, failure, cancellation or an early error —
        // so the live quota is handed back here once: a call that never reserved (or already
        // released) is a no-op, and a refusal latched during the call becomes the typed fault that
        // stops new model/tool work. The hand-over check keeps the release from running ahead of the
        // fact the step published — the batch `run_step` committed is enrolled by `publish`/`admit`,
        // and only once that really completed does the ceiling come back. Doing it in the wrapper
        // covers the early returns between the reservation and the provider call that a call-site
        // release would miss.
        self.settle_operation_output(&attempt_id);
        if self.state.model_execution.is_some() {
            self.state.model_execution = None;
            self.publish_snapshot();
        }
        result
    }

    /// Publishes the explicit model execution phase at the exact boundary it describes.
    fn enter_model_execution(&mut self, phase: ModelExecutionPhase) {
        if self.state.model_execution == Some(phase) {
            return;
        }
        self.state.model_execution = Some(phase);
        self.publish_snapshot();
    }

    async fn run_step(
        &mut self,
        input: StepInput,
        plan: crate::tool::opaque::ToolPlan,
        retry_of: Option<String>,
    ) -> Result<ModelStepOutput, ThreadError> {
        // The first boundary is admission, not a provider call: the driver may wait for storage
        // admission before it even knows whether the context hook runs.
        self.enter_model_execution(ModelExecutionPhase::Admitting);
        self.ensure_model_admission(&input.cancellation).await?;
        let tools = plan.declarations();
        if input.turn_id.is_empty()
            || input.attempt_id.is_empty()
            || self.state.context.records.iter().any(|record| {
                record.id == format!("{}:input", input.attempt_id)
                    || record.id == format!("{}:output", input.attempt_id)
            })
            || self
                .state
                .attempts
                .iter()
                .any(|attempt| attempt.attempt_id == input.attempt_id)
        {
            return Err(ThreadError::InvalidIdentity);
        }
        if input.cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        let correcting = retry_of.as_ref().is_some_and(|source| {
            self.state.attempts.last().is_some_and(|previous| {
                previous.attempt_id == *source
                    && matches!(
                        previous.outcome,
                        AttemptOutcome::Rejected {
                            reason: ModelOutputViolation::SoloBatch { .. },
                            ..
                        }
                    )
            })
        });
        if retry_of.is_none() || correcting {
            // The context-preparation hook is the only step that may replace or compact the context;
            // it is the one boundary that can genuinely run a separate preparation model.
            self.enter_model_execution(ModelExecutionPhase::PreparingContext);
            self.apply_pending_runtime_facts()?;
            self.prepare_context(&input, tools.clone()).await?;
            // Admission after the hook is a storage/capacity wait again, not context preparation and
            // not a provider wait.
            self.enter_model_execution(ModelExecutionPhase::Admitting);
            self.ensure_model_admission(&input.cancellation).await?;
        }
        // Assembling the message/steering/tool records and the immutable request is request
        // construction, not context preparation.
        self.enter_model_execution(ModelExecutionPhase::BuildingRequest);
        let mut records = self.state.context.records.to_vec();
        let (messages, consumed_messages) = if retry_of.is_none() || correcting {
            self.message_context(&input.turn_id)
        } else {
            (Vec::new(), self.state.consumed_messages)
        };
        let (steering, steering_ids) = if retry_of.is_none() || correcting {
            self.steering_context(&input.turn_id)
        } else {
            (Vec::new(), Vec::new())
        };
        let changed = !messages.is_empty() || !steering.is_empty() || !input.content.is_empty();
        records.extend(messages);
        if !input.content.is_empty() {
            records.push(ContextRecord {
                tool_calls: Vec::new(),
                id: format!("{}:input", input.attempt_id),
                turn_id: Some(input.turn_id.clone()),
                source: if correcting {
                    ContextSource::Runtime {
                        source_id: "model-output-correction".into(),
                    }
                } else {
                    ContextSource::User
                },
                content: input.content,
            });
        }
        records.extend(steering);
        let input_revision = if changed {
            self.state
                .context
                .revision
                .checked_add(1)
                .ok_or(ThreadError::RevisionExhausted)?
        } else {
            self.state.context.revision
        };
        let output_revision = input_revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        let context = ContextSnapshot {
            revision: input_revision,
            records: records.into(),
        };
        context.validate_complete()?;
        let tool_context = context.clone();
        // Reserving the reliable output budget waits for capacity/storage admission, which is a wait
        // on the host, not on the model implementation.
        self.enter_model_execution(ModelExecutionPhase::Admitting);
        // The call may only start once the reliable budget really funds the output it will stream:
        // this is the admission reservation, and it waits at a storage safety point instead of
        // starting a call whose result could not be retained.
        let output_budget = self
            .reserve_operation_output(&input.attempt_id, &input.cancellation)
            .await?;
        let (progress, observations) = crate::model::ModelProgressSender::channel(
            self.cold.clone(),
            &self.id,
            &input.attempt_id,
            output_budget.clone(),
        );
        self.model_progress = Some((input.attempt_id.clone(), observations));
        let request = ModelRequest {
            tool_call_mode: plan.call_mode(),
            solo_tool_ids: plan.solo_tool_ids(),
            progress: Some(progress),
            thread_id: self.id.clone(),
            turn_id: input.turn_id.clone(),
            attempt_id: input.attempt_id.clone(),
            context: context.clone(),
            tools: tools.clone(),
            committed_private_context: self.state.private_context.clone(),
            resources: self.resources.clone(),
            cancellation: input.cancellation.clone(),
        };
        // Preparing the request runs inside the model implementation (freezing, encoding, estimating)
        // before any provider call exists, so this is still preparation, not a wait for output.
        self.enter_model_execution(ModelExecutionPhase::PreparingRequest);
        let mut model = self.model.take().ok_or(ThreadError::Closed)?;
        let prepared = self.await_with_mailbox(model.prepare(request)).await;
        self.model = Some(model);
        self.publish_snapshot();
        let prepared = match prepared {
            Err(error)
                if input.cancellation.is_cancelled()
                    && error.kind == crate::model::ModelFailureKind::Cancelled =>
            {
                return Err(ThreadError::Cancelled);
            }
            other => other.map_err(|error| ThreadError::Model(Arc::new(error)))?,
        };
        if input.cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        let input_estimate = prepared.input_estimate();
        let request_metadata = prepared.request_metadata().cloned();
        let tool_projection = prepared.tool_projection().cloned();
        // Capacity and storage admission happen before dispatch; they are waits on the host, not on
        // the model implementation.
        self.enter_model_execution(ModelExecutionPhase::Admitting);
        self.capacity.admit(input_estimate)?;
        self.ensure_model_admission(&input.cancellation).await?;
        if !plan.remains_authorized(&self.tools) {
            return Err(ThreadError::ToolPermissionRevoked);
        }
        let mut attempts = self.state.attempts.to_vec();
        attempts.push(RequestAttempt {
            request_metadata,
            tool_projection: tool_projection.clone(),
            turn_id: input.turn_id.clone(),
            attempt_id: input.attempt_id.clone(),
            input: context.clone(),
            tools,
            outcome: AttemptOutcome::Running,
            retry_of,
            input_estimate,
        });
        self.validate_steering(&steering_ids)?;
        self.consume_active_input(&input.turn_id, &input.attempt_id)?;
        self.consume_steering(&steering_ids, &input.turn_id, &input.attempt_id)?;
        self.retry_plan = Some((input.attempt_id.clone(), plan.clone()));
        self.state.context = context;
        self.state.consumed_messages = consumed_messages;
        self.state.attempts = attempts.clone().into();
        // The attempt is now running the prepared provider call; streaming output is observed
        // through the model progress sender.
        self.enter_model_execution(ModelExecutionPhase::Running);
        // The attempt's own "running" row is part of what this call's reliable output ceiling funds.
        self.claim_output(input.attempt_id.clone());
        self.publish();
        let result = self
            .await_with_mailbox(prepared.execute())
            .await
            .map_err(Arc::new);
        if input.cancellation.is_cancelled()
            && self.interrupted_turn.as_deref() == Some(&input.turn_id)
            && let Err(error) = &result
            && error.kind != crate::model::ModelFailureKind::Cancelled
        {
            self.input_driver
                .fail(Arc::new(ThreadError::Model(error.clone())));
            self.pause_inputs();
        }
        let (outcome, result) = if input.cancellation.is_cancelled() {
            (
                AttemptOutcome::Cancelled { result },
                Err(ThreadError::Cancelled),
            )
        } else {
            match result {
                Err(error) => (
                    AttemptOutcome::Failed(error.clone()),
                    Err(ThreadError::Model(error)),
                ),
                Ok(output)
                    if self
                        .output_violation(&input.attempt_id, input_revision, &plan, &output)
                        .is_some() =>
                {
                    let reason = self
                        .output_violation(&input.attempt_id, input_revision, &plan, &output)
                        .ok_or(ThreadError::InvalidOutput)?;
                    (
                        AttemptOutcome::Rejected {
                            output,
                            reason: reason.clone(),
                        },
                        Err(reason.into()),
                    )
                }
                Ok(output) => {
                    let admitted_call_ids = output
                        .tool_calls
                        .iter()
                        .map(|call| call.call_id.clone())
                        .collect::<Vec<_>>();
                    for call in &output.tool_calls {
                        if let Some(executor) = plan.get(&call.tool_id) {
                            self.pending.insert(
                                call.call_id.clone(),
                                PendingCall {
                                    context: tool_context.clone(),
                                    model_projection: tool_projection.clone(),
                                    turn_id: input.turn_id.clone(),
                                    call: call.clone(),
                                    executor: executor.clone(),
                                },
                            );
                        }
                    }
                    let mut records = self.state.context.records.to_vec();
                    records.push(ContextRecord {
                        tool_calls: output.tool_calls.clone(),
                        id: format!("{}:output", input.attempt_id),
                        // 提交记录与 effect/子引用共享同一个稳定 turn id：这里克隆而不是移动，
                        // `input.turn_id` 之后仍要写入 live_calls。
                        turn_id: Some(input.turn_id.clone()),
                        source: ContextSource::Assistant,
                        content: output.content.clone(),
                    });
                    self.state.context = ContextSnapshot {
                        revision: output_revision,
                        records: records.into(),
                    };
                    self.state.private_context = output.private_context.clone();
                    for call_id in admitted_call_ids {
                        self.state
                            .live_calls
                            .entry(call_id)
                            .or_insert_with(|| input.turn_id.clone());
                    }
                    (AttemptOutcome::Committed(output.clone()), Ok(output))
                }
            }
        };
        if let Some(attempt) = attempts.last_mut() {
            attempt.outcome = outcome;
        }
        self.state.attempts = attempts.into();
        // This commit carries the answer the attempt reserved its reliable output budget for, so the
        // store transfers that reservation onto the fact instead of charging the same output twice.
        self.pending_output_claim = Some(input.attempt_id.clone());
        self.publish();
        result
    }
}
