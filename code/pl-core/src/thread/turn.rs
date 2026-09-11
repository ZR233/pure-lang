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
        self.apply_model_update().await?;
        if input.cancellation.is_cancelled() || self.interrupt.is_closing() {
            return Err(ThreadError::Cancelled);
        }
        let started = tokio::time::Instant::now();
        let turn_id = input.turn_id.clone();
        let mut turns = self.state.turns.to_vec();
        turns.push(TurnRecord {
            elapsed_ms: None,
            input_id: self.active_input.clone(),
            turn_id: turn_id.clone(),
            state: TurnState::Running,
            model_steps: 0,
        });
        self.state.turns = turns.clone().into();
        self.publish();
        let mut result = self.execute_turn_loop(input).await;
        if matches!(result, Err(ThreadError::Closed)) && self.interrupt.is_closing() {
            result = Err(ThreadError::Cancelled);
        }
        if matches!(result, Err(ThreadError::Cancelled)) {
            self.cancel_pending_calls(Some(&turn_id))?;
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
                Err(ThreadError::Cancelled) => TurnState::Cancelled,
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
                    if corrections < 2 && step + 1 < input.max_model_steps.get() =>
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
                    step += 1;
                    continue;
                }
                other => other?,
            };
            if output.tool_calls.is_empty() {
                return Ok(TurnCompletion {
                    model_steps: step + 1,
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
                            model_steps: step + 1,
                            outcome: TurnOutcome::ToolCompleted,
                            last_output: output,
                        });
                    }
                    Ok(ToolDispatch::Completed(result))
                        if result.control() == crate::tool::ToolControl::AwaitInteraction =>
                    {
                        return Ok(TurnCompletion {
                            model_steps: step + 1,
                            outcome: TurnOutcome::WaitingInteraction,
                            last_output: output,
                        });
                    }
                    Err(ThreadError::Cancelled) if !input.cancellation.is_cancelled() => {}
                    Ok(_) | Err(ThreadError::Tool(_)) => {} // Failed tool facts were committed for model recovery.
                    Err(error) => return Err(error),
                }
            }
            if step + 1 == input.max_model_steps.get() {
                return Ok(TurnCompletion {
                    model_steps: step + 1,
                    outcome: TurnOutcome::StepLimit,
                    last_output: output,
                });
            }
            content = Vec::new();
            step += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelError, ModelRequest, ModelSession, ModelToolCall, PreparedModelCall};
    use crate::tool::opaque::{CallContext, Registration, Tool, ToolError};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MixedThenSolo {
        requests: usize,
        invalid_requests: usize,
        cancel_correction: bool,
        corrupt_identity: u8,
    }

    impl ModelSession for MixedThenSolo {
        async fn prepare(
            &mut self,
            request: ModelRequest,
        ) -> Result<PreparedModelCall, ModelError> {
            let index = self.requests;
            self.requests += 1;
            if index > 0 && self.cancel_correction {
                request.cancellation.cancel();
            }
            let mixed = index < self.invalid_requests;
            let corrupt_identity = self.corrupt_identity;
            if index > 0 {
                assert!(request.context.records.iter().flat_map(|r| &r.content).any(|c|
                    matches!(c, ContextContent::Text { text } if text.contains("No tools in this batch were executed"))));
                assert!(request.committed_private_context.is_none());
            }
            Ok(PreparedModelCall::new(async move {
                let tools = if mixed {
                    vec!["read", "finish"]
                } else {
                    vec!["finish"]
                };
                let mut output = ModelStepOutput {
                    attempt_id: request.attempt_id,
                    base_context_revision: request.context.revision,
                    content: Vec::new(),
                    tool_calls: tools
                        .into_iter()
                        .map(|id| ModelToolCall {
                            call_id: format!("{index}-{id}"),
                            tool_id: id.into(),
                            arguments: OpaquePayload::text("input"),
                        })
                        .collect(),
                    private_context: Some(OpaquePayload::text("uncommitted-provider-context")),
                    usage: Default::default(),
                };
                match corrupt_identity {
                    0 => {}
                    1 => output.attempt_id = "wrong-attempt".into(),
                    2 => output.base_context_revision += 1,
                    3 => output.tool_calls[1].call_id = output.tool_calls[0].call_id.clone(),
                    4 => output.tool_calls[0].call_id.clear(),
                    5 => output.tool_calls[0].tool_id = "unknown-tool".into(),
                    _ => unreachable!("test fixture corruption"),
                }
                Ok(output)
            }))
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct CountTool(Arc<AtomicUsize>);
    impl Tool for CountTool {
        async fn execute(
            &self,
            _: OpaquePayload,
            _: CallContext,
        ) -> Result<crate::tool::ToolOutput, ToolError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(crate::tool::ToolOutput::new(OpaquePayload::text("done"), Vec::new()).ending_turn())
        }
    }

    #[tokio::test]
    async fn solo_batch_is_corrected_without_executing_rejected_tools() {
        let count = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            "correction".into(),
            DynModelSession::new(MixedThenSolo {
                requests: 0,
                invalid_requests: 1,
                cancel_correction: false,
                corrupt_identity: 0,
            }),
        )
        .unwrap();
        thread
            .register_tools(vec![
                Registration::new(
                    "read".into(),
                    OpaquePayload::text("read"),
                    CountTool(count.clone()),
                )
                .unwrap(),
                Registration::new(
                    "finish".into(),
                    OpaquePayload::text("finish"),
                    CountTool(count.clone()),
                )
                .unwrap()
                .with_turn_completion(),
            ])
            .await
            .unwrap();
        let result = thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(8).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await;
        assert!(
            result.is_ok(),
            "Solo mixed batch must be corrected: {result:?}"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let snapshot = thread.snapshot();
        assert_eq!(snapshot.attempts.len(), 2);
        assert_eq!(snapshot.attempts[1].retry_of.as_deref(), Some("attempt:0"));
        assert!(matches!(
            snapshot.attempts[0].outcome,
            AttemptOutcome::Rejected { .. }
        ));
        let replayed = journal::replay(&thread.journal().await.unwrap()).unwrap();
        assert_eq!(replayed.attempts[1].retry_of.as_deref(), Some("attempt:0"));
        assert_eq!(replayed.deliveries.len(), 1);
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn repeated_solo_violations_stop_at_correction_or_step_budget_without_side_effects() {
        for (budget, expected_attempts) in [(8, 3), (1, 1), (2, 2)] {
            let count = Arc::new(AtomicUsize::new(0));
            let thread = ThreadHandle::start(
                "bounded".into(),
                DynModelSession::new(MixedThenSolo {
                    requests: 0,
                    invalid_requests: 10,
                    cancel_correction: false,
                    corrupt_identity: 0,
                }),
            )
            .unwrap();
            thread
                .register_tools(vec![
                    Registration::new(
                        "read".into(),
                        OpaquePayload::text("read"),
                        CountTool(count.clone()),
                    )
                    .unwrap(),
                    Registration::new(
                        "finish".into(),
                        OpaquePayload::text("finish"),
                        CountTool(count.clone()),
                    )
                    .unwrap()
                    .with_turn_completion(),
                ])
                .await
                .unwrap();
            let result = thread
                .run_turn(TurnInput {
                    turn_id: "turn".into(),
                    attempt_prefix: "attempt".into(),
                    content: Vec::new(),
                    max_model_steps: std::num::NonZeroU32::new(budget).unwrap(),
                    cancellation: CancellationToken::new(),
                })
                .await;
            assert!(matches!(
                result,
                Err(ThreadError::ModelOutput(
                    ModelOutputViolation::SoloBatch { .. }
                ))
            ));
            assert_eq!(thread.snapshot().attempts.len(), expected_attempts);
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert!(thread.snapshot().deliveries.is_empty());
            thread.close().await.unwrap();
        }
    }
    #[tokio::test]
    async fn cancelling_during_correction_never_executes_the_candidate_batch() {
        let count = Arc::new(AtomicUsize::new(0));
        let thread = ThreadHandle::start(
            "cancel".into(),
            DynModelSession::new(MixedThenSolo {
                requests: 0,
                invalid_requests: 1,
                cancel_correction: true,
                corrupt_identity: 0,
            }),
        )
        .unwrap();
        thread
            .register_tools(vec![
                Registration::new(
                    "read".into(),
                    OpaquePayload::text("read"),
                    CountTool(count.clone()),
                )
                .unwrap(),
                Registration::new(
                    "finish".into(),
                    OpaquePayload::text("finish"),
                    CountTool(count.clone()),
                )
                .unwrap()
                .with_turn_completion(),
            ])
            .await
            .unwrap();
        let result = thread
            .run_turn(TurnInput {
                turn_id: "turn".into(),
                attempt_prefix: "attempt".into(),
                content: Vec::new(),
                max_model_steps: std::num::NonZeroU32::new(8).unwrap(),
                cancellation: CancellationToken::new(),
            })
            .await;
        assert!(matches!(result, Err(ThreadError::Cancelled)));
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(thread.snapshot().deliveries.is_empty());
        thread.close().await.unwrap();
    }
    #[derive(Debug)]
    struct RejectAttemptStore;
    impl cold::ColdStore for RejectAttemptStore {
        fn admit(
            &self,
            _: &str,
            _: u64,
            payload: OpaquePayload,
        ) -> Result<(), cold::ColdStoreError> {
            let record: serde_json::Value = serde_json::from_str(payload.content()).unwrap();
            if record
                .pointer("/attempt/outcome/kind")
                .and_then(serde_json::Value::as_str)
                == Some("rejected")
            {
                return Err(cold::ColdStoreError {
                    source: Box::new(std::io::Error::other("rejected attempt persistence failed")),
                });
            }
            Ok(())
        }
        async fn flush(&self, _: &str, _: u64) -> Result<(), cold::ColdStoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn identity_violations_and_rejection_storage_failure_never_enter_correction() {
        for corruption in 0..=5 {
            let count = Arc::new(AtomicUsize::new(0));
            let thread = ThreadHandle::start(
                "fatal-admission".into(),
                DynModelSession::new(MixedThenSolo {
                    requests: 0,
                    invalid_requests: 1,
                    cancel_correction: false,
                    corrupt_identity: corruption,
                }),
            )
            .unwrap();
            thread
                .register_tools(vec![
                    Registration::new(
                        "read".into(),
                        OpaquePayload::text("read"),
                        CountTool(count.clone()),
                    )
                    .unwrap(),
                    Registration::new(
                        "finish".into(),
                        OpaquePayload::text("finish"),
                        CountTool(count.clone()),
                    )
                    .unwrap()
                    .with_turn_completion(),
                ])
                .await
                .unwrap();
            if corruption == 0 {
                thread
                    .attach_storage(cold::ColdStoreHandle::new(RejectAttemptStore))
                    .await
                    .unwrap();
            }
            let error = thread
                .run_turn(TurnInput {
                    turn_id: "turn".into(),
                    attempt_prefix: "attempt".into(),
                    content: Vec::new(),
                    max_model_steps: std::num::NonZeroU32::new(8).unwrap(),
                    cancellation: CancellationToken::new(),
                })
                .await
                .unwrap_err();
            assert!(
                matches!(
                    (corruption, &error),
                    (0, ThreadError::Storage(_))
                        | (
                            1,
                            ThreadError::ModelOutput(ModelOutputViolation::AttemptIdentity { .. })
                        )
                        | (
                            2,
                            ThreadError::ModelOutput(ModelOutputViolation::ContextRevision { .. })
                        )
                        | (
                            3,
                            ThreadError::ModelOutput(
                                ModelOutputViolation::DuplicateCallIdentity { .. }
                            )
                        )
                        | (
                            4,
                            ThreadError::ModelOutput(ModelOutputViolation::EmptyCallIdentity)
                        )
                        | (
                            5,
                            ThreadError::ModelOutput(ModelOutputViolation::UnknownTool { .. })
                        )
                ),
                "unexpected corruption {corruption} failure: {error:?}"
            );
            assert_eq!(thread.snapshot().attempts.len(), 1);
            assert_eq!(count.load(Ordering::SeqCst), 0);
            assert!(thread.snapshot().deliveries.is_empty());
            let closed = thread.close().await;
            if corruption != 0 {
                closed.unwrap();
            }
        }
    }
}
