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

pub(super) struct PendingEffect {
    pub(super) write: cold::ThreadWrite,
    /// Encoded byte length of this effect, measured at most once while it stays pending.
    ///
    /// Pressure admission must not re-serialize the whole still-pending queue on every model step:
    /// that made a long Turn quadratic in its commit count. The measurement is memoized here, so an
    /// effect is serialized once no matter how many admission checks observe it, and a Thread with
    /// an attached store (whose queue drains on every publish) never serializes it here at all.
    pub(super) encoded_bytes: Option<u64>,
}

pub(super) struct Owner {
    pub(super) model_identity: Option<String>,
    pub(super) pending_model_update: Option<super::model_update::DeferredModelUpdate>,
    pub(super) pending_runtime_facts: std::collections::BTreeMap<String, RuntimeFact>,
    pub(super) context_preparation: Option<context_preparation::ContextPreparer>,
    pub(super) model_progress: Option<(String, watch::Receiver<crate::model::ModelProgress>)>,
    pub(super) permission_leases:
        std::collections::BTreeMap<String, crate::tool::opaque::ExecutionAuthority>,
    pub(super) active_inputs: Vec<String>,
    pub(super) input_batch_through: Option<u64>,
    pub(super) interrupted_turn: Option<String>,
    pub(super) input_driver: input::InputDriver,
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
    /// Availability while prepare temporarily borrows the session out of `model`.
    /// Cleared before publishing its result, so poison/close still comes from the session.
    pub(super) preparing_model_available: bool,
    pub(super) capacity: ContextCapacity,
    pub(super) effect_window: Arc<EffectWindow>,
    pub(super) pending_effects: std::collections::VecDeque<PendingEffect>,
    pub(super) cold: Option<cold::ColdStoreHandle>,
    pub(super) cold_error: Option<Arc<cold::ColdStoreError>>,
    /// `(fault_generation, durability fence)` a latched storage fault must be recovered to, fixed
    /// when that generation's fault is first observed. `None` means this owner owes no recovery.
    /// It is captured once so a later admission while paused cannot move the target that "recovery
    /// succeeded" is verified against.
    pub(super) fault_fence: Option<(u64, u64)>,
    /// Fault generation the attached backend itself reported, if the current latch came from it.
    ///
    /// A backend-named generation is only recovered once that backend verifies the generation's own
    /// recovery; a generation this owner latched by itself (its own reliable-output truncation) has
    /// no backend retry to wait for, so its own fixed fence is the proof. Tracking which generation
    /// the backend owns is what keeps an older, already-verified verdict from releasing a newer
    /// fault.
    pub(super) store_fault_generation: Option<u64>,
    /// Newest generation the backend reported as verified-recovered, as a typed value.
    ///
    /// It is the backend's own receipt ([`crate::thread::cold::StoragePressure::recovered_generation`]),
    /// not a second fact source: the owner is the only place that compares it with the fault it is
    /// holding.
    pub(super) verified_recovery_generation: Option<u64>,
    /// Typed storage fault this owner latched itself, independent of the backend's report.
    ///
    /// An in-flight model/tool result that exceeded the reliable retention budget is a fact about
    /// this Thread's own work, not about the backend: a healthy backend report must not clear it
    /// before an explicit resume names the generation it releases.
    pub(super) local_fault: Option<cold::StorageFaultKind>,
    /// Accepted producer output that still has to be stored, keyed by its stable identity.
    ///
    /// One entry per failed capture/archive operation, so two parallel tools that both failed keep
    /// their own obligation instead of one overwriting the other; the pause stays until *every*
    /// entry of the generation is retried and its bytes really stored, so a healthy history write can
    /// never stand in for an archive that failed. Empty for a pure reliable-budget truncation, whose
    /// accepted bytes the reliable writer saves with the fence. Each value carries the fault
    /// generation it was latched under, so a stale retry cannot clear a newer obligation. Live
    /// execution state, never persisted.
    pub(super) output_obligations:
        std::collections::BTreeMap<String, (u64, Arc<dyn cold::OutputRetryObligation>)>,
    /// Repaired resource references whose committed result is not resident yet, keyed by call id.
    ///
    /// A producer can report its failed archive the moment it happens, so the user may retry before
    /// the operation has finished and committed its result. The reference must not be dropped then:
    /// it is kept here until the result it supplements is committed, and a repeated repair of the
    /// same call and reference never duplicates. It is released by the commit that applies it, so it
    /// is bounded by the operations still finishing, never by history.
    pub(super) pending_output_repairs:
        std::collections::BTreeMap<String, Vec<crate::context::ResourceReference>>,
    /// Stable identities of obligations this generation already re-stored.
    ///
    /// A producer reports the same failure twice — once through its live channel and again on the
    /// return path — and the return path can arrive *after* the retry already stored the bytes.
    /// Remembering which identities were satisfied keeps that late repeat from re-arming an
    /// obligation the Thread already discharged, so the pause cannot come back for a fact that is
    /// already durable. Bounded by the operations that failed in this generation and cleared when
    /// the generation is released, exactly like the obligations themselves.
    pub(super) satisfied_output_obligations: std::collections::BTreeSet<String>,
    /// Whether the reliable budget could not fund the next operation's live output.
    ///
    /// Nothing was lost, so this is backpressure: admission waits at the storage safety point and
    /// resumes by itself once the budget can hold the quota again.
    pub(super) output_backpressure: bool,
    /// Hysteresis latch of the byte-threshold pressure, so the backpressure above stays a separate
    /// fact instead of being folded into the latch the thresholds own.
    pub(super) threshold_paused: bool,
    /// Live-output quotas held for operations that are still producing a result.
    ///
    /// One entry per in-flight model call or tool call, removed at the same boundary that hands the
    /// result over, so the map is bounded by the number of concurrent operations and never grows
    /// with history.
    pub(super) operation_budgets:
        std::collections::BTreeMap<String, std::sync::Arc<crate::model::OutputBudget>>,
    /// Owner-owned wakeup every in-flight operation's budget reports its first refusal through.
    ///
    /// A producer charges its output off the owner's stack, so this is how a truncation reaches the
    /// owner the moment it happens: the owner latches the typed fault from `output_refusals` instead
    /// of waiting for the call to return. It is the same fault the release path latches, published by
    /// the same owner, so it is not a second fact source.
    pub(super) output_refusals: tokio::sync::watch::Sender<Option<crate::model::OutputRefusal>>,
    pub(super) output_refusal_rx: tokio::sync::watch::Receiver<Option<crate::model::OutputRefusal>>,
    /// Operations whose transient ceiling is held back until their fact is handed to the queue.
    ///
    /// Bounded by the number of concurrent operations: an entry is added only when a call ended
    /// while the reliable queue still retained its fact, and it is drained at the exact boundary the
    /// queue empties.
    pub(super) deferred_output_releases: std::collections::BTreeSet<String>,
    /// Operation whose reserved output ceiling funds the next effect this owner publishes.
    ///
    /// Set by the call site that is about to commit one of an in-flight operation's own facts, so the
    /// store can transfer exactly that operation's reservation onto the fact instead of charging the
    /// same output twice or spending an unrelated operation's reserved headroom. Consumed by
    /// [`Owner::publish`]; a publish that commits nothing drops it, and the operation's own boundary
    /// then releases its still-untransferred ceiling.
    pub(super) pending_output_claim: Option<String>,
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
        // The backend's own storage watch, subscribed once a store is attached. An idle owner reads
        // the same typed report a paused Turn's safety point reads, so a save that recovered — or a
        // fault that just latched — reaches this Thread without waiting for its next command. It is
        // a wake-up only: the latch and the readiness are still computed here, by this one owner.
        let mut pressure_updates = self.subscribe_pressure();
        loop {
            tokio::task::consume_budget().await;
            if pressure_updates.is_none() {
                pressure_updates = self.subscribe_pressure();
            }
            if let Some(Some(completion)) =
                futures::FutureExt::now_or_never(futures::StreamExt::next(&mut self.background))
            {
                self.finish_background(completion);
            }
            // Service the admitted mailbox batch before another paid request. Bound the batch
            // so a continuous notification producer cannot starve close or other owner commands.
            for _ in 0..self.mailbox.len() {
                if let Ok(message) = self.mailbox.try_recv() {
                    self.dispatch_mailbox(message).await;
                }
            }
            let command = match commands.try_recv() {
                Ok(command) => command,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
                Err(mpsc::error::TryRecvError::Empty) => {
                    if self.drive_one_input().await {
                        continue;
                    }
                    let awaiting_storage = self.state.persistence.resume_required;
                    tokio::select! {
                        Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => {
                            self.finish_background(completion);
                            continue;
                        },
                        update = async {
                            match pressure_updates.as_mut() {
                                Some(update) => update.changed().await.is_ok(),
                                None => std::future::pending::<bool>().await,
                            }
                        }, if awaiting_storage => {
                            if !update {
                                // A dropped backend never wakes this owner again; the fixed fence
                                // below stays the fact that decides readiness.
                                pressure_updates = None;
                            }
                            // A storage change is a wake-up, never a second fact source: this owner
                            // re-reads the backend's typed report, re-presents what it never handed
                            // over, and republishes the readiness it owns — so a Thread parked
                            // between Turns learns about the recovery instead of waiting for work.
                            self.admit_cold();
                            if self.cold_error.is_some() {
                                // A failed save is not pressure: new model/tool admission stays held
                                // until an explicit continue releases this generation.
                                self.state.persistence.resume_required = true;
                            }
                            self.refresh_storage_pressure();
                            self.publish_snapshot();
                            continue;
                        },
                        command = commands.recv() => match command {
                            Some(command) => command,
                            None => break,
                        },
                        message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                            if let Some(message) = message {
                                self.dispatch_mailbox(message).await;
                            }
                            continue;
                        }
                    }
                }
            };
            if self.interrupt.is_closing() && !matches!(command, Command::Close(_)) {
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

    /// Subscribes to the attached backend's storage-change notification.
    ///
    /// `None` while no store is attached: there is no storage fact to wait on then, so the idle loop
    /// arms nothing instead of polling.
    fn subscribe_pressure(&self) -> Option<watch::Receiver<()>> {
        self.cold
            .as_ref()
            .and_then(|store| store.subscribe_pressure(&self.id))
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
        snapshot.input_execution = if let Some(error) = self.input_driver.error() {
            input::InputExecution::Failed {
                error: error.clone(),
            }
        } else if let Some(turn_id) = &self.interrupted_turn {
            input::InputExecution::Interrupting {
                turn_id: turn_id.clone(),
            }
        } else if let Some(input_id) = self.active_inputs.first() {
            input::InputExecution::Running {
                input_id: input_id.clone(),
            }
        } else if self.input_driver.options().is_some()
            && self.state.lifecycle == ThreadLifecycle::Open
            && !self.interrupt.is_closing()
        {
            input::InputExecution::Ready
        } else {
            input::InputExecution::Paused
        };
        snapshot.model_available = self.model.as_ref().map_or(
            self.preparing_model_available,
            DynModelSession::is_available,
        );
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
        self.publish_once();
        // A pending output repair can only supplement a result that is already committed: the journal
        // validates a delivered context record against its delivery, so the supplement always lands in
        // its own commit right after the result. Applying it here — right after the commit that may
        // have carried that result — keeps the reference off the snapshot that produced the delivery
        // while still becoming durable before any consumer can observe the pause released.
        if self.flush_pending_output_repairs() {
            self.publish_once();
        }
    }

    /// Commits the resident facts as exactly one effect batch.
    fn publish_once(&mut self) {
        self.state.tool_progress.retain(|id, _| {
            self.state.lifecycle == ThreadLifecycle::Open
                && self
                    .state
                    .tasks
                    .get(id)
                    .is_some_and(|task| task.status == task::TaskStatus::Running)
        });
        let sequence = self.state.commit_sequence.saturating_add(1);
        let Some(effect) =
            ThreadEffectBatch::between(&self.id, &self.published, &self.state, sequence)
        else {
            // Nothing was committed, so no fact can take a reservation over: the claim is dropped
            // and the operation's own boundary releases its ceiling instead.
            self.pending_output_claim = None;
            return;
        };
        let effect = Arc::new(effect);
        self.state.commit_sequence = sequence;
        // The effect above is the only copy of this commit's facts: the write below persists a
        // transfer state that still carries them so the writer projects the effect without reading
        // back a pruned checkpoint; the resident state drops the same facts right after.
        let checkpoint =
            ThreadCheckpoint::capture_transfer(self.id.clone(), sequence, self.state.clone());
        self.state.retain_live_facts();
        self.published = self.state.clone();
        // The operation about to see its result committed names the reservation that funds it; a
        // publish that commits no effect (handled above) drops the claim, so the operation's own
        // boundary releases its still-untransferred ceiling instead.
        let output_claim = self.pending_output_claim.take();
        self.pending_effects.push_back(PendingEffect {
            write: cold::ThreadWrite {
                effect: effect.clone(),
                checkpoint,
                output_claim,
            },
            encoded_bytes: None,
        });
        self.effect_window.push(effect);
        self.admit_cold();
        self.publish_snapshot();
    }

    /// Names the reservation that funds the facts the next [`Owner::publish`] commits.
    ///
    /// An in-flight model/tool call holds a reliable output ceiling (see
    /// [`Owner::reserve_operation_output`]) that funds *everything* that call commits on this
    /// owner's stack — its task/attempt lifecycle rows as well as the eventual result. Naming the
    /// operation here is what lets the reliable channel take those bytes over from that one ceiling:
    /// without it a routine intermediate row (a task turning `Running`) would have to fit unrelated
    /// free headroom and could be refused — and latched as a storage fault — while the very ceiling
    /// reserved for it sat unused. The claim is consumed by exactly one publish, so a commit that
    /// turns out to produce no fact simply drops it.
    pub(super) fn claim_output(&mut self, operation: impl Into<String>) {
        self.pending_output_claim = Some(operation.into());
    }
}
