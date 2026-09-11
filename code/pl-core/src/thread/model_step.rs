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
            if !ids.insert(&call.call_id) || self.state.context.records.iter().any(|record| record.tool_calls.iter().any(|old| old.call_id == call.call_id)) || self.state.attempts.iter().any(|attempt| matches!(&attempt.outcome, AttemptOutcome::Committed(previous) if previous.tool_calls.iter().any(|old| old.call_id == call.call_id))) {
                return Some(ModelOutputViolation::DuplicateCallIdentity { call_id: call.call_id.clone() });
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

    fn ensure_model_admission(&mut self) -> Result<(), ThreadError> {
        if !self.uncommitted_tools.is_empty() {
            return Err(ThreadError::PendingToolCommit);
        }
        self.refresh_storage_pressure();
        self.publish_snapshot();
        if self.state.persistence.pressure_paused {
            return Err(ThreadError::StoragePressure);
        }
        if let Some(error) = &self.cold_error {
            return Err(ThreadError::Storage(error.clone()));
        }
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
        self.ensure_model_admission()?;
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
            self.prepare_context(&input, tools.clone()).await?;
            self.ensure_model_admission()?;
        }
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
        let (progress, observations) = crate::model::ModelProgressSender::channel();
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
        let mut model = self.model.take().ok_or(ThreadError::Closed)?;
        let prepared = self.await_with_mailbox(model.prepare(request)).await;
        self.model = Some(model);
        self.publish_snapshot();
        let prepared = prepared.map_err(|error| ThreadError::Model(Arc::new(error)))?;
        if input.cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        let input_estimate = prepared.input_estimate();
        let request_metadata = prepared.request_metadata().cloned();
        let tool_projection = prepared.tool_projection().cloned();
        self.capacity.admit(input_estimate)?;
        self.ensure_model_admission()?;
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
        self.publish();
        let result = self.await_with_mailbox(prepared.execute()).await;
        let (outcome, result) = if input.cancellation.is_cancelled() {
            (
                AttemptOutcome::Cancelled {
                    result: result.map_err(Arc::new),
                },
                Err(ThreadError::Cancelled),
            )
        } else {
            match result {
                Err(error) => {
                    let error = Arc::new(error);
                    (
                        AttemptOutcome::Failed(error.clone()),
                        Err(ThreadError::Model(error)),
                    )
                }
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
                        turn_id: Some(input.turn_id),
                        source: ContextSource::Assistant,
                        content: output.content.clone(),
                    });
                    self.state.context = ContextSnapshot {
                        revision: output_revision,
                        records: records.into(),
                    };
                    self.state.private_context = output.private_context.clone();
                    (AttemptOutcome::Committed(output.clone()), Ok(output))
                }
            }
        };
        if let Some(attempt) = attempts.last_mut() {
            attempt.outcome = outcome;
        }
        self.state.attempts = attempts.into();
        self.publish();
        result
    }
}
