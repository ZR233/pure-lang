//! One Turn's model/tool loop, executed entirely inside the Thread owner.
use super::*;

impl Owner {
    pub(super) async fn run_turn(
        &mut self,
        input: TurnInput,
    ) -> Result<TurnCompletion, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if input.turn_id.is_empty()
            || input.attempt_prefix.is_empty()
            || self
                .state
                .turns
                .iter()
                .any(|turn| turn.turn_id == input.turn_id)
        {
            return Err(ThreadError::InvalidIdentity);
        }
        if input.cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        let started = tokio::time::Instant::now();
        let turn_id = input.turn_id.clone();
        let mut turns = self.state.turns.to_vec();
        turns.push(TurnRecord {
            elapsed_ms: None,
            input_id: self.active_inputs.first().cloned(),
            turn_id: turn_id.clone(),
            state: TurnState::Running,
            model_steps: 0,
        });
        self.state.turns = turns.clone().into();
        self.publish();
        // Publish the generation before any asynchronous preparation: input received while a
        // model is being opened must redirect this Turn, not silently queue behind it.
        let mut result = async {
            self.apply_model_update().await?;
            if input.cancellation.is_cancelled() || self.interrupt.is_closing() {
                return Err(ThreadError::Cancelled);
            }
            self.execute_turn_loop(input).await
        }
        .await;
        if matches!(result, Err(ThreadError::Closed)) && self.interrupt.is_closing() {
            result = Err(ThreadError::Cancelled);
        }
        if matches!(result, Err(ThreadError::Cancelled)) {
            self.cancel_pending_calls(Some(&turn_id))?;
            if self.interrupted_turn.as_deref() == Some(&turn_id) {
                self.settle_turn_cancellation(&turn_id).await;
            }
        }
        if let Some(turn) = turns.last_mut() {
            turn.elapsed_ms = Some(
                started
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .map_err(|_| ThreadError::RevisionExhausted)?,
            );
            turn.model_steps = self
                .state
                .attempts
                .iter()
                .filter(|attempt| attempt.turn_id == turn_id)
                .count()
                .try_into()
                .map_err(|_| ThreadError::RevisionExhausted)?;
            turn.state = match &result {
                Ok(completed) => TurnState::Finished(completed.outcome),
                Err(ThreadError::Cancelled) => match self.input_driver.error() {
                    Some(error) => TurnState::Failed {
                        description: error.to_string(),
                    },
                    None if self.interrupted_turn.as_deref() == Some(&turn_id) => {
                        TurnState::Interrupted
                    }
                    None => TurnState::Cancelled,
                },
                Err(error) => TurnState::Failed {
                    description: error.to_string(),
                },
            };
        }
        self.state.turns = turns.into();
        self.publish();
        result
    }

    pub(super) fn cancel_pending_calls(
        &mut self,
        turn_id: Option<&str>,
    ) -> Result<(), ThreadError> {
        let selected = self
            .pending
            .iter()
            .filter(|(_, call)| turn_id.is_none_or(|turn| call.turn_id == turn))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Ok(());
        }
        let revision = self
            .state
            .context
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        let mut records = self.state.context.records.to_vec();
        let mut deliveries = self.state.deliveries.to_vec();
        let mut identities = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for call_id in selected {
            let Some(call) = self.pending.remove(&call_id) else {
                return Err(ThreadError::MissingCall);
            };
            let content = vec![ContextContent::Text {
                text: Arc::from(
                    "Tool call was cancelled before execution. No tool side effects were started.",
                ),
            }];
            let mut suffix = 0_u64;
            let id = loop {
                let id = format!("cancelled:{revision}:{call_id}:{suffix}");
                if identities.insert(id.clone()) {
                    break id;
                }
                suffix += 1;
            };
            records.push(ContextRecord {
                id,
                turn_id: Some(call.turn_id),
                source: ContextSource::ToolResult {
                    call_id: call_id.clone(),
                    tool_id: call.call.tool_id.clone(),
                },
                content: content.clone(),
                tool_calls: Vec::new(),
            });
            deliveries.push(ToolDelivery {
                target: ToolDeliveryTarget::CallResult,
                call_id,
                tool_id: call.call.tool_id,
                output: crate::tool::ToolOutput::new(
                    OpaquePayload::text("cancelled before execution"),
                    content.clone(),
                ),
                delivered_context: content,
                outcome: ToolOutcome::Cancelled,
            });
        }
        self.state.context = ContextSnapshot {
            revision,
            records: records.into(),
        };
        self.state.deliveries = deliveries.into();
        Ok(())
    }

    async fn execute_turn_loop(&mut self, input: TurnInput) -> Result<TurnCompletion, ThreadError> {
        if input.attempt_prefix.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        let mut content = input.content;
        let mut step = 0_u32;
        let mut corrections = 0_u8;
        let mut correction_source = None;
        loop {
            let completed = step.checked_add(1).ok_or(ThreadError::RevisionExhausted)?;
            let step_input = StepInput {
                turn_id: input.turn_id.clone(),
                attempt_id: format!("{}:{step}", input.attempt_prefix),
                content,
                cancellation: input.cancellation.clone(),
            };
            let result = match correction_source.take() {
                Some(source) => self.correct_step(step_input, source).await,
                None => self.step(step_input).await,
            };
            let output = match result {
                Err(ThreadError::ModelOutput(ModelOutputViolation::SoloBatch { ref tool_ids }))
                    if corrections < 2 && !input.max_model_steps.reached(completed) =>
                {
                    if input.cancellation.is_cancelled() || self.interrupt.is_closing() {
                        return Err(ThreadError::Cancelled);
                    }
                    content = vec![ContextContent::Text {
                        text: Arc::from(format!(
                            "No tools in this batch were executed. Tools {tool_ids:?} must each be called alone in a model response. Generate a corrected response using the committed results; do not repeat completed tools."
                        )),
                    }];
                    correction_source = Some(format!("{}:{step}", input.attempt_prefix));
                    corrections += 1;
                    step = step.checked_add(1).ok_or(ThreadError::RevisionExhausted)?;
                    continue;
                }
                other => other?,
            };
            if output.tool_calls.is_empty() {
                return Ok(TurnCompletion {
                    model_steps: completed,
                    outcome: TurnOutcome::Completed,
                    last_output: output,
                });
            }
            for call in &output.tool_calls {
                match self
                    .execute_tool(call.call_id.clone(), input.cancellation.clone())
                    .await
                {
                    Ok(ToolDispatch::Completed(result))
                        if result.control() == crate::tool::ToolControl::EndTurn =>
                    {
                        return Ok(TurnCompletion {
                            model_steps: completed,
                            outcome: TurnOutcome::Completed,
                            last_output: output,
                        });
                    }
                    Ok(ToolDispatch::Completed(result))
                        if result.control() == crate::tool::ToolControl::AwaitInteraction =>
                    {
                        return Ok(TurnCompletion {
                            model_steps: completed,
                            outcome: TurnOutcome::WaitingInteraction,
                            last_output: output,
                        });
                    }
                    Err(ThreadError::Cancelled) if !input.cancellation.is_cancelled() => {}
                    Ok(_) | Err(ThreadError::Tool(_)) => {} // Failed tool facts were committed for model recovery.
                    Err(error) => return Err(error),
                }
            }
            if input.max_model_steps.reached(completed) {
                return Ok(TurnCompletion {
                    model_steps: completed,
                    outcome: TurnOutcome::StepLimit,
                    last_output: output,
                });
            }
            content = Vec::new();
            step = step.checked_add(1).ok_or(ThreadError::RevisionExhausted)?;
        }
    }
}
