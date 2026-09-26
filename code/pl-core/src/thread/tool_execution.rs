//! Frozen tool execution, control validation and atomic result commit.
use super::*;

#[derive(Clone)]
pub(super) struct ToolExecutionCompletion {
    pub(super) call: PendingCall,
    pub(super) output: Result<crate::tool::ToolOutput, Arc<crate::tool::opaque::ToolError>>,
    pub(super) cancellation: CancellationToken,
}

pub(super) type ToolExecutionFuture = futures::future::BoxFuture<'static, ToolExecutionCompletion>;

/// Reliable output reservation identity of one tool call.
///
/// It is exactly the caller key the tool's task is stored under, so the mailbox progress handler
/// finds the call's quota from the caller identity it already has instead of parsing it back out.
pub(super) fn tool_output_operation(call_id: &str) -> String {
    format!("task:{call_id}")
}

impl Owner {
    pub(super) async fn execute_tool(
        &mut self,
        id: String,
        cancellation: CancellationToken,
    ) -> Result<ToolDispatch, ThreadError> {
        self.await_storage_admission(&cancellation).await?;
        let foreground = self
            .pending
            .get(&id)
            .ok_or(ThreadError::MissingCall)?
            .executor
            .requires_foreground_execution();
        // The call may only start once the reliable budget really funds the output it will stream;
        // this waits at a storage safety point instead of starting a tool whose result could not be
        // retained. The quota is held until the call's result is committed.
        let operation = tool_output_operation(&id);
        self.reserve_operation_output(&operation, &cancellation)
            .await?;
        let mut execution = match self.begin_tool_execution(id.clone(), cancellation.clone()) {
            Ok(execution) => execution,
            Err(error) => {
                // The call never started, so hand its unused quota straight back; nothing was lost.
                self.finish_operation_output(&operation);
                return Err(error);
            }
        };
        if foreground {
            let completion = self.await_with_mailbox(execution).await;
            return self
                .commit_tool_execution(completion)
                .map(ToolDispatch::Completed);
        }
        let completed = self
            .await_with_mailbox(async {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => None,
                    completed = &mut execution => Some(completed),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => None,
                }
            })
            .await;
        if let Some(completed) = completed {
            if !cancellation.is_cancelled() {
                return self
                    .commit_tool_execution(completed)
                    .map(ToolDispatch::Completed);
            }
            execution = Box::pin(async move { completed });
        }
        self.background.push(execution);
        let task = self.acknowledge_task(&id)?;
        if cancellation.is_cancelled() {
            return Err(ThreadError::Cancelled);
        }
        Ok(ToolDispatch::Running(task))
    }

    pub(super) fn begin_tool_execution(
        &mut self,
        id: String,
        cancellation: CancellationToken,
    ) -> Result<ToolExecutionFuture, ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if cancellation.is_cancelled() || self.interrupt.is_closing() {
            return Err(ThreadError::Cancelled);
        }
        let call = self.pending.remove(&id).ok_or(ThreadError::MissingCall)?;
        if let Err(error) = self.start_task(&call) {
            self.pending.insert(id, call);
            return Err(error);
        }
        let cancellation = if call.executor.requires_foreground_execution() {
            cancellation.child_token()
        } else {
            CancellationToken::new()
        };
        self.task_tokens
            .insert(format!("task:{}", call.call.call_id), cancellation.clone());
        let authorized = call.executor.remains_authorized(&self.tools);
        let context = crate::tool::opaque::CallContext {
            grant: Default::default(),
            context: call.context.clone(),
            model_projection: call.model_projection.clone(),
            tasks: Some(TaskAccess::new(self, &id, &call.executor)),
            thread_id: self.id.clone(),
            turn_id: call.turn_id.clone(),
            call_id: id,
            cancellation: cancellation.clone(),
            extensions: Arc::new(self.state.extensions.clone()),
            extension_sequence: self.state.extension_sequence,
            catalog: self.tools.catalog(),
        };
        Ok(Box::pin(async move {
            let output = if authorized {
                call.executor
                    .execute(call.call.arguments.clone(), context)
                    .await
                    .map_err(Arc::new)
            } else {
                Err(Arc::new(crate::tool::opaque::ToolError::new(
                    ThreadError::ToolPermissionRevoked,
                )))
            };
            ToolExecutionCompletion {
                call,
                output,
                cancellation,
            }
        }))
    }

    pub(super) fn commit_tool_execution(
        &mut self,
        completion: ToolExecutionCompletion,
    ) -> Result<crate::tool::ToolOutput, ThreadError> {
        let id = completion.call.call.call_id.clone();
        let operation = tool_output_operation(&id);
        let previous = self.state.clone();
        let result = self.stage_tool_execution(completion.clone());
        let terminal =
            result.is_ok() || matches!(result, Err(ThreadError::Tool(_) | ThreadError::Cancelled));
        if terminal && self.state.deliveries.len() > previous.deliveries.len() {
            self.uncommitted_tools.remove(&id);
            self.permission_leases.remove(&format!("permission:{id}"));
            self.task_tokens.remove(&format!("task:{id}"));
            // The result being committed is exactly the output this call reserved budget for, so the
            // store transfers that reservation onto the fact instead of charging it a second time.
            self.pending_output_claim = Some(operation.clone());
            self.publish();
            // The call returned and its result is now enrolled by the same `publish`/`admit` the
            // ordinary path uses, so the transient ceiling can be given back — but only once that
            // hand-over really completed. Handing it back before the result was enrolled would let a
            // new operation take budget the retained bytes still need, so a backpressured admission
            // defers the release to the boundary the queue drains at.
            self.settle_operation_output(&operation);
        } else {
            self.state = previous;
            self.uncommitted_tools.insert(id, completion);
            self.publish_snapshot();
            // The completion stays resident here until a retry commit hands it over, so its
            // reservation stays charged: the retained bytes are real memory, and releasing the
            // ceiling now would make the Thread look drained while still holding them.
        }
        result
    }

    pub(super) fn retry_tool_commits(&mut self) -> Result<(), ThreadError> {
        let completed = self.uncommitted_tools.values().cloned().collect::<Vec<_>>();
        for completion in completed {
            let id = completion.call.call.call_id.clone();
            let result = self.commit_tool_execution(completion);
            if self.uncommitted_tools.contains_key(&id) {
                result?;
                return Err(ThreadError::PendingToolCommit);
            }
        }
        Ok(())
    }

    /// Accepts one increment of a running tool call's live output under its reliable output quota.
    ///
    /// The increment is charged against the call's reservation *before* it becomes resident: an
    /// increment the quota cannot hold is refused, the call is cancelled with the bytes it already
    /// accepted, and the earlier output is kept instead of being replaced by a larger one. The typed
    /// fault is latched right here, before the call can finish, so a Thread that truncated a tool
    /// never quietly starts more work while the tool is still wrapping up.
    ///
    /// The producer sends only what changed, so this path never re-reads or re-copies the accumulated
    /// output: the owner appends one shared chunk, or takes the producer's bounded window as a whole
    /// replacement, and publishes the newer snapshot by cloning `Arc`s.
    ///
    /// Two distinct ceilings can refuse an increment, and both cancel the call with the bytes already
    /// accepted and latch a typed pressure fault, but they bound different things:
    ///
    /// - The [`crate::model::MAX_TOOL_PROGRESS_BYTES`] live-window ceiling and the
    ///   [`crate::model::MAX_TOOL_PROGRESS_PARTS`] identity ceiling bound the *observed* window this
    ///   session holds. A producer that would exceed them is not rolling its window over as the port
    ///   requires, so it is cancelled rather than allowed to keep producing output the owner would
    ///   silently drop. This never charges or releases the durable reservation.
    /// - The call's reliable output reservation (`OutputBudget`) bounds the bytes the durable queue
    ///   may retain. Its accounting is owned by the writer, not by this path.
    pub(super) fn report_tool_progress(
        &mut self,
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        update: crate::model::ToolProgressUpdate,
    ) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if !self
            .state
            .tasks
            .get(&caller)
            .is_some_and(|task| task.status == task::TaskStatus::Running)
        {
            return Err(ThreadError::TaskAccessExpired);
        }
        if !executor.remains_authorized(&self.tools) {
            return Err(ThreadError::ToolPermissionRevoked);
        }
        let accepted = match self.state.tool_progress.get(&caller) {
            Some(current) => current.bytes_after(&update),
            None => crate::model::ToolProgress::default().bytes_after(&update),
        };
        let parts = match self.state.tool_progress.get(&caller) {
            Some(current) => current.parts_after(&update),
            None => 1,
        };
        if accepted > crate::model::MAX_TOOL_PROGRESS_BYTES
            || parts > crate::model::MAX_TOOL_PROGRESS_PARTS
        {
            // The live-output window this session can observe is exhausted: the producer is emitting
            // more than the owner can hold. This is a core semantic ceiling on the *observed* window,
            // distinct from the reliable output budget (a reservation this path never charges or
            // releases, and whose canonical/archive accounting belongs to the writer). A tool that
            // keeps producing output the owner can no longer accept must not be quietly ignored, so
            // cancel the call with the bytes already accepted — they stay resident — and latch the
            // same typed output-pressure fault the reliable-budget refusal uses, so the Thread fails
            // closed and pauses further admission instead of letting a runaway producer run on.
            if let Some(token) = self.task_tokens.get(&caller) {
                token.cancel();
            }
            let limit = if accepted > crate::model::MAX_TOOL_PROGRESS_BYTES {
                crate::model::MAX_TOOL_PROGRESS_BYTES
            } else {
                crate::model::MAX_TOOL_PROGRESS_PARTS as u64
            };
            let resident = self
                .state
                .tool_progress
                .get(&caller)
                .map_or(0, crate::model::ToolProgress::bytes);
            self.latch_output_budget_fault(&caller, resident, limit);
            return Err(ThreadError::InvalidOutput);
        }
        let refused = match self.operation_budgets.get(&caller) {
            Some(budget) => budget.charge(accepted).is_err(),
            // No reservation to charge: this backend has no reliable budget, and the preview is
            // still bounded by the producer's own process-side output cap.
            None => false,
        };
        if refused {
            // Cancel the operation with the bytes it already accepted; the earlier preview stays.
            if let Some(token) = self.task_tokens.get(&caller) {
                token.cancel();
            }
            // Publish the typed fault the moment the preview is refused. A tool that keeps wrapping
            // up after this must not hold the fault — and the admission block that goes with it —
            // invisible until it happens to commit; the owner is the single publisher, so this rides
            // the same latch the release path uses and repeating it there is a no-op.
            let refusal = self
                .operation_budgets
                .get(&caller)
                .and_then(|budget| budget.refusal());
            if let Some((accepted, limit)) = refusal {
                self.latch_output_budget_fault(&caller, accepted, limit);
            }
            return Err(ThreadError::InvalidOutput);
        }
        let next = match self.state.tool_progress.get(&caller) {
            Some(current) => current.applied(&update),
            None => crate::model::ToolProgress::default().applied(&update),
        };
        self.state.tool_progress.insert(caller, next);
        self.publish_snapshot();
        Ok(())
    }

    /// Latches a producer's typed reliable-output failure the moment the tool reports it.
    ///
    /// A capture write or archive that could not be stored is a fact about accepted output, not about
    /// the call's final result: reporting it through the running call's own reliable channel latches
    /// the fault — and blocks other model/tool admission — before the call returns, instead of waiting
    /// for `execute` to unwind. The obligation the fault carries is kept under this generation, so the
    /// pause stays until that same obligation is retried and its bytes really stored. The latch is
    /// idempotent, so a producer that reports the same fault again (or the return path that sees it a
    /// second time) is a no-op.
    pub(super) fn report_tool_output_storage_fault(
        &mut self,
        caller: String,
        executor: crate::tool::opaque::ExecutionAuthority,
        fault: Arc<cold::OutputStorageFault>,
    ) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if !self
            .state
            .tasks
            .get(&caller)
            .is_some_and(|task| task.status == task::TaskStatus::Running)
        {
            return Err(ThreadError::TaskAccessExpired);
        }
        if !executor.remains_authorized(&self.tools) {
            return Err(ThreadError::ToolPermissionRevoked);
        }
        self.latch_storage_fault(fault.kind, fault.source.clone(), fault.obligation.clone());
        Ok(())
    }

    fn stage_tool_execution(
        &mut self,
        completion: ToolExecutionCompletion,
    ) -> Result<crate::tool::ToolOutput, ThreadError> {
        if completion.cancellation.is_cancelled()
            && self.interrupted_turn.as_deref() == Some(completion.call.turn_id.as_str())
            && let Err(error) = &completion.output
            && !matches!(
                error.source.downcast_ref::<ThreadError>(),
                Some(ThreadError::Cancelled)
            )
        {
            self.input_driver
                .fail(Arc::new(ThreadError::Tool(error.clone())));
            self.pause_inputs();
        }
        let ToolExecutionCompletion {
            call,
            output,
            cancellation,
        } = completion;
        let id = call.call.call_id.clone();
        let (mut output, mut outcome) = match output {
            Ok(output) => {
                let outcome = match call.executor.validate_control(&output) {
                    Ok(()) => ToolOutcome::Succeeded,
                    Err(error) => ToolOutcome::Failed(Arc::new(error)),
                };
                (output, outcome)
            }
            Err(error) => {
                let output = error.observed_output().cloned().unwrap_or_else(|| {
                    let text = error.to_string();
                    crate::tool::ToolOutput::new(
                        OpaquePayload::text(text.clone()),
                        vec![ContextContent::Text {
                            text: Arc::from(text),
                        }],
                    )
                });
                (output, ToolOutcome::Failed(error))
            }
        };
        // A producer that could not store the output it was streaming reports its own typed storage
        // failure through the tool boundary. Latch the exact category it named before the failure is
        // delivered, so the Thread pauses further admission instead of continuing as if the capture
        // had succeeded; the accepted output still rides along with the failed result.
        if let ToolOutcome::Failed(error) = &outcome
            && let Some(fault) = error
                .source
                .downcast_ref::<crate::thread::cold::OutputStorageFault>()
        {
            self.latch_storage_fault(fault.kind, fault.source.clone(), fault.obligation.clone());
        }
        if cancellation.is_cancelled() {
            outcome = ToolOutcome::Cancelled;
        }
        let controls = output.control() != crate::tool::ToolControl::Continue
            || !output.extension_mutations().is_empty()
            || !output.revealed_tools().is_empty();
        if matches!(outcome, ToolOutcome::Succeeded)
            && controls
            && !call.executor.remains_authorized(&self.tools)
        {
            outcome = ToolOutcome::Failed(Arc::new(crate::tool::opaque::ToolError::new(
                ThreadError::ToolPermissionRevoked,
            )));
        }

        if matches!(outcome, ToolOutcome::Succeeded)
            && let Err(error) = self.tools.validate_reveal(output.revealed_tools())
        {
            outcome = ToolOutcome::Failed(Arc::new(crate::tool::opaque::ToolError::new(error)));
        }
        let interaction_id = crate::tool::opaque::tool_interaction_id(&id);
        if matches!(outcome, ToolOutcome::Succeeded)
            && output.interaction().is_some()
            && self.state.interactions.contains_key(&interaction_id)
        {
            outcome = ToolOutcome::Failed(Arc::new(crate::tool::opaque::ToolError::new(
                ThreadError::InvalidIdentity,
            )));
        }
        if matches!(outcome, ToolOutcome::Succeeded)
            && !output.extension_mutations().is_empty()
            && let Err(error) = super::extensions::stage_extensions(
                &mut self.state,
                output.extension_mutations().to_vec(),
            )
        {
            outcome = ToolOutcome::Failed(Arc::new(crate::tool::opaque::ToolError::new(error)));
        }
        if matches!(outcome, ToolOutcome::Succeeded)
            && let Some(payload) = output.interaction()
        {
            let now = crate::time::unix_seconds();
            let record = interactions::InteractionRecord {
                created_at: now,
                updated_at: now,
                continuation_id: None,
                request: interactions::InteractionRequest {
                    id: interaction_id.clone(),
                    turn_id: call.turn_id.clone(),
                    payload: payload.clone(),
                },
                revision: 1,
                state: interactions::InteractionState::Pending,
                extension_mutations: Vec::new(),
            };
            self.state
                .interactions
                .insert(interaction_id, record.clone());
            let mut changes = self.state.interaction_changes.to_vec();
            changes.push(record);
            self.state.interaction_changes = changes.into();
        }
        if let ToolOutcome::Failed(error) = &outcome {
            let diagnostic = error.to_string();
            if !output.context().iter().any(|content| matches!(content, ContextContent::Text { text } if text.as_ref() == diagnostic)) {
                output.append_framework_context(ContextContent::Text {
                    text: Arc::from(format!("Framework recorded tool failure: {diagnostic}")),
                });
            }
        }
        let delivered_context = if matches!(outcome, ToolOutcome::Cancelled) {
            vec![ContextContent::Text {
                text: Arc::from(
                    "Tool execution was cancelled. Any observed result is retained for inspection; its state changes and Turn control were not applied.",
                ),
            }]
        } else {
            output.context().to_vec()
        };
        let task = self
            .state
            .tasks
            .get(&format!("task:{id}"))
            .cloned()
            .ok_or(ThreadError::InvalidIdentity)?;
        let (target, delivered_context) = if task.acknowledgement.is_some() {
            let context = super::background::result_context(
                &task,
                task::TaskStatus::from_outcome(&outcome),
                delivered_context,
            );
            let message_id = super::background::append_result_message(
                &mut self.state,
                &task,
                &output,
                context.clone(),
            )?;
            (ToolDeliveryTarget::Inbox { message_id }, context)
        } else {
            let revision = self
                .state
                .context
                .revision
                .checked_add(1)
                .ok_or(ThreadError::RevisionExhausted)?;
            let mut records = self.state.context.records.to_vec();
            records.push(ContextRecord {
                tool_calls: Vec::new(),
                id: super::background::unique_id(
                    &format!("tool-result:{id}"),
                    self.state
                        .context
                        .records
                        .iter()
                        .map(|record| record.id.as_str()),
                )?,
                turn_id: Some(call.turn_id),
                source: ContextSource::ToolResult {
                    call_id: id.clone(),
                    tool_id: call.call.tool_id.clone(),
                },
                content: delivered_context.clone(),
            });
            self.state.context = ContextSnapshot {
                revision,
                records: records.into(),
            };
            (ToolDeliveryTarget::CallResult, delivered_context)
        };
        let mut deliveries = self.state.deliveries.to_vec();
        deliveries.push(ToolDelivery {
            target,
            call_id: id.clone(),
            tool_id: call.call.tool_id,
            output: output.clone(),
            delivered_context,
            outcome: outcome.clone(),
        });
        self.state.deliveries = deliveries.into();
        self.finish_task(&id, &outcome)?;
        self.settle_execution_permission(&id)?;
        if matches!(outcome, ToolOutcome::Succeeded) && !output.revealed_tools().is_empty() {
            self.tools.reveal(output.revealed_tools())?;
            self.state.discovered_tools = self.tools.discovery();
        }

        match outcome {
            ToolOutcome::Succeeded => Ok(output),
            ToolOutcome::Interrupted | ToolOutcome::Cancelled => Err(ThreadError::Cancelled),
            ToolOutcome::Failed(error) => Err(ThreadError::Tool(error)),
        }
    }
}
