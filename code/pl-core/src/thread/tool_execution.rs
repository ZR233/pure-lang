//! Frozen tool execution, control validation and atomic result commit.
use super::*;

#[derive(Clone)]
pub(super) struct ToolExecutionCompletion {
    pub(super) call: PendingCall,
    pub(super) output: Result<crate::tool::ToolOutput, Arc<crate::tool::opaque::ToolError>>,
    pub(super) cancellation: CancellationToken,
}

pub(super) type ToolExecutionFuture = futures::future::BoxFuture<'static, ToolExecutionCompletion>;

impl Owner {
    pub(super) async fn execute_tool(
        &mut self,
        id: String,
        cancellation: CancellationToken,
    ) -> Result<ToolDispatch, ThreadError> {
        let foreground = self
            .pending
            .get(&id)
            .ok_or(ThreadError::MissingCall)?
            .executor
            .requires_foreground_execution();
        let mut execution = self.begin_tool_execution(id.clone(), cancellation.clone())?;
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
        let previous = self.state.clone();
        let result = self.stage_tool_execution(completion.clone());
        let terminal =
            result.is_ok() || matches!(result, Err(ThreadError::Tool(_) | ThreadError::Cancelled));
        if terminal && self.state.deliveries.len() > previous.deliveries.len() {
            self.uncommitted_tools.remove(&id);
            self.permission_leases.remove(&format!("permission:{id}"));
            self.task_tokens.remove(&format!("task:{id}"));
            self.publish();
        } else {
            self.state = previous;
            self.uncommitted_tools.insert(id, completion);
            self.publish_snapshot();
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

    fn stage_tool_execution(
        &mut self,
        completion: ToolExecutionCompletion,
    ) -> Result<crate::tool::ToolOutput, ThreadError> {
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
            output.append_framework_context(ContextContent::Text {
                text: Arc::from(format!("Framework recorded tool failure: {error}")),
            });
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
