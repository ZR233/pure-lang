//! Private serial execution and atomic publication.
use super::*;

#[derive(Clone)]
pub(super) struct PendingCall {
    pub(super) context: ContextSnapshot,
    pub(super) model_projection: Option<OpaquePayload>,
    pub(super) turn_id: String,
    pub(super) call: crate::model::ModelToolCall,
    pub(super) executor: crate::tool::opaque::FrozenTool,
}

pub(super) struct Owner {
    pub(super) model_identity: Option<String>,
    pub(super) pending_model_update: Option<super::model_update::ModelUpdate>,
    pub(super) context_preparation: Option<context_preparation::ContextPreparer>,
    pub(super) model_progress: Option<(String, watch::Receiver<crate::model::ModelProgress>)>,
    pub(super) permission_leases:
        std::collections::BTreeMap<String, crate::tool::opaque::ExecutionAuthority>,
    pub(super) active_input: Option<String>,
    pub(super) input_driver: Option<input::InputDriverOptions>,
    pub(super) input_driver_error: Option<Arc<ThreadError>>,
    pub(super) uncommitted_tools:
        std::collections::BTreeMap<String, super::tool_execution::ToolExecutionCompletion>,
    pub(super) task_commands: mpsc::WeakSender<mailbox::MailboxCommand>,
    pub(super) background:
        futures::stream::FuturesUnordered<super::tool_execution::ToolExecutionFuture>,
    pub(super) task_tokens: std::collections::BTreeMap<String, CancellationToken>,
    pub(super) mailbox: mpsc::Receiver<super::mailbox::MailboxCommand>,
    pub(super) interrupt: cancellation::InterruptHandle,
    pub(super) id: String,
    pub(super) model: Option<DynModelSession>,
    pub(super) capacity: ContextCapacity,
    pub(super) journal: Vec<Arc<journal::ThreadCommit>>,
    pub(super) history: Arc<std::sync::RwLock<Vec<Arc<journal::ThreadCommit>>>>,
    pub(super) encoded_journal: Vec<Result<OpaquePayload, Arc<journal::JournalCodecError>>>,
    pub(super) cold: Option<cold::ColdStoreHandle>,
    pub(super) cold_error: Option<Arc<cold::ColdStoreError>>,
    pub(super) resources: Option<crate::context::ResourceAccess>,
    pub(super) retry_plan: Option<(String, crate::tool::opaque::ToolPlan)>,
    pub(super) published: ThreadSnapshot,
    pub(super) state: ThreadSnapshot,
    pub(super) publish: watch::Sender<ThreadSnapshot>,
    pub(super) tools: crate::tool::opaque::ToolManager,
    pub(super) pending: std::collections::BTreeMap<String, PendingCall>,
}

impl Owner {
    pub(super) async fn run(mut self, mut commands: mpsc::Receiver<Command>) {
        loop {
            tokio::task::consume_budget().await;
            if let Some(Some(completion)) =
                futures::FutureExt::now_or_never(futures::StreamExt::next(&mut self.background))
            {
                self.finish_background(completion);
            }
            let command = match commands.try_recv() {
                Ok(command) => command,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
                Err(mpsc::error::TryRecvError::Empty) => {
                    if self.drive_one_input().await {
                        continue;
                    }
                    tokio::select! {
                        Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => {
                            self.finish_background(completion);
                            continue;
                        },
                        command = commands.recv() => match command {
                            Some(command) => command,
                            None => break,
                        },
                        message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                            if let Some(message) = message {
                                self.process_mailbox(message);
                            }
                            continue;
                        }
                    }
                }
            };
            if self.interrupt.is_closing()
                && !matches!(command, Command::Close(_) | Command::Flush(_))
            {
                command.reject();
                continue;
            }
            match command {
                Command::ReplaceModel(factory, preparation, reply) => {
                    self.model_identity = None;
                    self.pending_model_update = None;
                    let result = self.replace_model(factory).await;
                    if result.is_ok() {
                        self.context_preparation = preparation;
                    }
                    let _ = reply.send(result);
                }
                Command::ContextPreparation(preparation, reply) => {
                    self.context_preparation = preparation;
                    let _ = reply.send(Ok(()));
                }
                Command::Reveal(ids, reply) => {
                    let result = self.tools.reveal(&ids).map_err(ThreadError::from);
                    if result.is_ok() {
                        self.state.discovered_tools = self.tools.discovery();
                        self.publish();
                    }
                    let _ = reply.send(result);
                }

                Command::RequestInteraction(request, reply) => {
                    let _ = reply.send(self.request_interaction(request));
                }
                Command::CancelInteraction(cancellation, reply) => {
                    let _ = reply.send(self.cancel_interaction(cancellation));
                }
                Command::ResolveInteraction(resolution, reply) => {
                    let _ = reply.send(self.resolve_interaction(resolution));
                }

                Command::Application(update, reply) => {
                    let _ = reply.send(self.update_application(update));
                }
                Command::Extensions(mutations, reply) => {
                    let _ = reply.send(self.mutate_extensions(mutations));
                }
                Command::Resources(resources, reply) => {
                    let result = if self.state.lifecycle == ThreadLifecycle::Open {
                        if !self
                            .resources
                            .as_ref()
                            .is_some_and(|current| current.same_service(&resources))
                        {
                            self.retry_plan = None;
                        }
                        self.resources = Some(resources);
                        Ok(())
                    } else {
                        Err(ThreadError::Closed)
                    };
                    let _ = reply.send(result);
                }

                Command::Retry {
                    source,
                    attempt,
                    cancellation,
                    reply,
                } => {
                    let active = self.interrupt.activate(&cancellation);
                    let result = self
                        .retry_attempt(source, attempt, active.token.clone())
                        .await;
                    let _ = reply.send(result);
                }
                Command::AttachCold(store, reply) => {
                    let result =
                        if self.cold.is_some() || self.state.lifecycle != ThreadLifecycle::Open {
                            Err(ThreadError::InvalidIdentity)
                        } else {
                            self.cold = Some(store);
                            self.admit_cold();
                            self.publish_snapshot();
                            Ok(())
                        };
                    let _ = reply.send(result);
                }
                Command::Flush(reply) => {
                    let result = self.flush_cold().await;
                    let _ = reply.send(result);
                }

                Command::QueuedTurn(request, reply) => {
                    let result = self.run_next_input(request).await;
                    let _ = reply.send(result);
                }
                Command::Turn(mut input, reply) => {
                    let active = self.interrupt.activate(&input.cancellation);
                    input.cancellation = active.token.clone();
                    let result = self.run_turn(input).await;
                    let _ = reply.send(result);
                }
                Command::Capacity(capacity, reply) => {
                    let result = if self.state.lifecycle == ThreadLifecycle::Open {
                        self.capacity = capacity;
                        Ok(())
                    } else {
                        Err(ThreadError::Closed)
                    };
                    let _ = reply.send(result);
                }

                Command::PatchRuntimeFacts(facts, reply) => {
                    let _ = reply.send(self.patch_runtime_facts(facts));
                }
                Command::UpdateFacts(facts, reply) => {
                    let _ = reply.send(self.update_facts(facts));
                }
                Command::ReplaceContext(replacement, reply) => {
                    let _ = reply.send(self.replace_context(replacement));
                }
                Command::Execute(id, cancellation, reply) => {
                    let active = self.interrupt.activate(&cancellation);
                    let result = self.execute_tool(id, active.token.clone()).await;
                    let _ = reply.send(result);
                }

                Command::Step(mut input, reply) => {
                    let active = self.interrupt.activate(&input.cancellation);
                    input.cancellation = active.token.clone();
                    let result = self.step(input).await;
                    let _ = reply.send(result);
                }
                Command::Close(reply) => {
                    if self.state.lifecycle == ThreadLifecycle::Open {
                        self.state.lifecycle = ThreadLifecycle::Closing;
                        self.publish();
                    }
                    let result = match self.close_resources().await {
                        Ok(()) => {
                            self.state.lifecycle = ThreadLifecycle::Closed;
                            self.publish();
                            self.flush_cold().await
                        }
                        Err(error) => Err(error),
                    };
                    let finished = result.is_ok();
                    let _ = reply.send(result);
                    if finished {
                        return;
                    }
                }
            }
        }
        // A last-handle drop seals admission and still settles owned work before releasing it.
        self.state.lifecycle = ThreadLifecycle::Closing;
        self.publish();
        match self.close_resources().await {
            Ok(()) => {
                self.state.lifecycle = ThreadLifecycle::Closed;
                self.publish();
                if let Err(error) = self.flush_cold().await {
                    tracing::error!(thread = self.id, %error, "abandoned Thread flush failed");
                }
            }
            Err(error) => {
                tracing::error!(thread = self.id, %error, "abandoned Thread resource close failed")
            }
        }
    }

    async fn close_resources(&mut self) -> Result<(), ThreadError> {
        self.retry_tool_commits()?;
        let tasks = self.task_tokens.keys().cloned().collect::<Vec<_>>();
        for id in tasks {
            self.cancel_task(&id)?;
        }
        self.drain_background().await;
        self.retry_tool_commits()?;
        self.cancel_pending_calls(None)?;
        let pending_interactions = self
            .state
            .interactions
            .iter()
            .filter(|(_, record)| record.state == interactions::InteractionState::Pending)
            .map(|(id, record)| interactions::InteractionCancellation {
                id: id.clone(),
                expected_revision: record.revision,
            })
            .collect::<Vec<_>>();
        for cancellation in pending_interactions {
            self.settle_interaction_cancellation(cancellation)?;
        }
        self.publish();
        if self
            .state
            .tasks
            .values()
            .any(|task| task.status == task::TaskStatus::Running)
        {
            return Err(ThreadError::InvalidOutput);
        }
        if let Some(mut model) = self.model.take()
            && let Err(error) = self.await_with_mailbox(model.close()).await
        {
            self.model = Some(model);
            return Err(ThreadError::Model(Arc::new(error)));
        }
        self.tools
            .close()
            .await
            .map_err(|error| ThreadError::Tool(Arc::new(error)))?;
        self.pending.clear();
        self.retry_plan = None;
        self.resources = None;
        Ok(())
    }

    pub(super) fn publish_snapshot(&self) {
        let mut snapshot = self.state.clone();
        snapshot.tool_progress.retain(|id, _| {
            snapshot.lifecycle == ThreadLifecycle::Open
                && snapshot
                    .tasks
                    .get(id)
                    .is_some_and(|task| task.status == task::TaskStatus::Running)
        });
        snapshot.model_progress = self
            .model_progress
            .as_ref()
            .filter(|(id, _)| {
                snapshot.attempts.last().is_some_and(|attempt| {
                    &attempt.attempt_id == id && matches!(attempt.outcome, AttemptOutcome::Running)
                })
            })
            .map(|(id, receiver)| crate::model::ActiveModelProgress {
                attempt_id: id.clone(),
                progress: receiver.borrow().clone(),
            });
        snapshot.input_execution = if let Some(input_id) = &self.active_input {
            input::InputExecution::Running {
                input_id: input_id.clone(),
            }
        } else if let Some(error) = &self.input_driver_error {
            input::InputExecution::Failed {
                error: error.clone(),
            }
        } else if self.input_driver.is_some()
            && self.state.lifecycle == ThreadLifecycle::Open
            && !self.interrupt.is_closing()
        {
            input::InputExecution::Ready
        } else {
            input::InputExecution::Paused
        };
        snapshot.model_available = self
            .model
            .as_ref()
            .is_some_and(DynModelSession::is_available);
        snapshot.pending_tool_commits = self.uncommitted_tools.keys().cloned().collect();
        if snapshot.lifecycle == ThreadLifecycle::Closed
            && snapshot.persistence.attached
            && snapshot.persistence.durable_sequence < snapshot.commit_sequence
        {
            snapshot.lifecycle = ThreadLifecycle::Closing;
        }
        self.publish.send_replace(snapshot);
    }

    pub(super) fn publish(&mut self) {
        self.state.tool_progress.retain(|id, _| {
            self.state.lifecycle == ThreadLifecycle::Open
                && self
                    .state
                    .tasks
                    .get(id)
                    .is_some_and(|task| task.status == task::TaskStatus::Running)
        });
        let sequence = self.journal.len() as u64 + 1;
        if let Some(commit) =
            journal::ThreadCommit::between(&self.id, &self.published, &self.state, sequence)
        {
            self.encoded_journal.push(commit.encode().map_err(Arc::new));
            let commit = Arc::new(commit);
            self.journal.push(commit.clone());
            // Publish immutable history before the watch snapshot advertising its watermark.
            // No IO, callbacks or await occurs under this append-only observation lock.
            self.history
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(commit);
            self.state.commit_sequence = sequence;
            self.published = self.state.clone();
            self.admit_cold();
            self.publish_snapshot();
        }
    }
}
