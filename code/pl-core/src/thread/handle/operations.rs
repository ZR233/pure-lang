//! Handle operations submit bounded commands; only the owner mutates Thread state.
use super::*;

impl ThreadHandle {
    /// Returns whether both handles address the same physical owner incarnation.
    pub fn same_instance(&self, other: &Self) -> bool {
        self.commands.same_channel(&other.commands)
    }

    /// Queues the latest configuration for the next Turn without waiting for current execution.
    /// # Errors
    /// Rejects empty configuration identities and closed owners.
    pub async fn queue_model_update(
        &self,
        identity: String,
        factory: crate::model::ModelFactory,
        preparation: Option<context_preparation::ContextPreparer>,
    ) -> Result<(), ThreadError> {
        self.queue_model_update_with_extensions(identity, factory, preparation, Vec::new())
            .await
            .map(|_| ())
    }

    /// Atomically records host-owned application state and queues its matching model binding.
    /// The admitted Turn keeps its existing binding; the queued binding applies at the next Turn.
    ///
    /// # Errors
    /// Rejects stale extension revisions, empty configuration identities and closed owners without
    /// applying either the extension mutations or the pending model update.
    pub async fn queue_model_update_with_extensions(
        &self,
        identity: String,
        factory: crate::model::ModelFactory,
        preparation: Option<context_preparation::ContextPreparer>,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        self.queue_deferred_model_update(
            super::model_update::DeferredModelUpdate::new(identity, factory, preparation),
            mutations,
        )
        .await
    }

    /// Atomically records host-owned application state and queues an already frozen model update.
    ///
    /// # Errors
    /// Rejects stale extension revisions, invalid updates and closed owners without partial writes.
    pub async fn queue_deferred_model_update(
        &self,
        update: DeferredModelUpdate,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        self.send_deferred_model_update(update, Default::default(), mutations)
            .await
    }

    /// Queues one frozen model update only when its host-owned preconditions still match.
    ///
    /// # Errors
    /// Rejects stale journal or extension revisions before applying extension or pending-model
    /// state.
    pub async fn queue_deferred_model_update_if_current(
        &self,
        update: DeferredModelUpdate,
        precondition: DeferredModelUpdatePrecondition,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        self.send_deferred_model_update(update, precondition, mutations)
            .await
    }

    async fn send_deferred_model_update(
        &self,
        update: DeferredModelUpdate,
        precondition: DeferredModelUpdatePrecondition,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::QueueModelUpdate(
                update,
                precondition,
                mutations,
                reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Starts one owner with a fresh, exclusively owned model session.
    ///
    /// # Errors
    /// Rejects an empty Thread identity before spawning the owner.
    pub fn start(id: String, model: DynModelSession) -> Result<Self, ThreadError> {
        Self::resume(id, model, None)
    }

    /// Starts or resumes one owner from a direct current-state checkpoint.
    pub fn resume(
        id: String,
        model: DynModelSession,
        checkpoint: Option<ThreadCheckpoint>,
    ) -> Result<Self, ThreadError> {
        Self::resume_with_model(id, Some(model), checkpoint)
    }

    /// Starts or resumes one owner without opening a physical model session.
    pub fn resume_without_model(
        id: String,
        checkpoint: Option<ThreadCheckpoint>,
    ) -> Result<Self, ThreadError> {
        Self::resume_with_model(id, None, checkpoint)
    }

    fn resume_with_model(
        id: String,
        model: Option<DynModelSession>,
        checkpoint: Option<ThreadCheckpoint>,
    ) -> Result<Self, ThreadError> {
        if id.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        let resumed = checkpoint.is_some();
        let (published, state) = match checkpoint {
            Some(checkpoint) => checkpoint.into_states(&id)?,
            None => (ThreadSnapshot::default(), ThreadSnapshot::default()),
        };
        let (commands, receiver) = mpsc::channel(64);
        let (mailbox, mailbox_receiver) = mpsc::channel(64);
        let interrupt = cancellation::InterruptHandle::default();
        let (publish, snapshots) = watch::channel(published.clone());
        let tools = crate::tool::opaque::ToolManager::with_discovery(&state.discovered_tools);
        let effect_window = Arc::new(EffectWindow::new());
        let thread_id = id.clone();
        let mut owner = Owner {
            model_identity: None,
            pending_model_update: None,
            pending_runtime_facts: Default::default(),
            context_preparation: None,
            model_progress: None,
            effect_window: effect_window.clone(),
            permission_leases: Default::default(),
            active_inputs: Vec::new(),
            input_batch_through: None,
            interrupted_turn: None,
            input_driver: if resumed {
                input::InputDriver::Paused
            } else {
                input::InputDriver::Dormant
            },
            interrupt: interrupt.clone(),
            mailbox: mailbox_receiver,
            task_commands: mailbox.downgrade(),
            id,
            model,
            state,
            publish,
            tools,
            pending: Default::default(),
            task_tokens: Default::default(),
            background: Default::default(),
            uncommitted_tools: Default::default(),
            capacity: Default::default(),
            pending_effects: Default::default(),
            cold: None,
            cold_error: None,
            resources: None,
            retry_plan: None,
            published,
        };
        owner.publish();
        owner.publish_snapshot();
        tokio::spawn(owner.run(receiver));
        Ok(Self {
            thread_id,
            _lifetime: Arc::new(HandleLifetime(interrupt.clone())),
            commands,
            mailbox,
            interrupt,
            snapshots,
            effect_window,
        })
    }

    /// Closes only when the owner atomically observes no active or pending work.
    ///
    /// # Errors
    /// Returns actual resource or persistence failures; a failed close remains retryable.
    pub async fn close_if_idle(&self) -> Result<bool, ThreadError> {
        if self.snapshot().lifecycle == ThreadLifecycle::Closed {
            return Ok(true);
        }
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::BeginIdleClose(reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        if !response.await.map_err(|_| ThreadError::Closed)? {
            return Ok(false);
        }
        self.close().await?;
        Ok(true)
    }

    /// Atomically replaces host state, instructions and selected tools at an idle boundary.
    ///
    /// # Errors
    /// Rejects stale snapshots, active work, invalid context and invalid tool registrations.
    pub async fn reconfigure(
        &self,
        mut update: IdleReconfiguration,
    ) -> Result<ThreadSnapshot, ThreadError> {
        let batch = crate::tool::opaque::RegistrationBatch::new(std::mem::take(&mut update.tools));
        let (reply, response) = oneshot::channel();
        let result = match self
            .mailbox
            .send(super::mailbox::MailboxCommand::Reconfigure(
                update,
                batch.clone(),
                reply,
            ))
            .await
        {
            Ok(()) => response.await.unwrap_or(Err(ThreadError::Closed)),
            Err(_) => Err(ThreadError::Closed),
        };
        finish_tool_transfer(batch, result).await
    }

    /// Stops the current Turn after atomically checking its expected identity and pauses queued execution.
    /// Passing no identity stops whichever Turn the owner is currently executing.
    ///
    /// # Errors
    /// Returns `InvalidIdentity` when another Turn is active, or `Closed` when the owner has exited.
    pub async fn interrupt_turn(&self, expected: Option<String>) -> Result<bool, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::InterruptTurn { expected, reply })
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Cancels only the currently active execution generation without closing the Thread.
    /// Returns false when there is no active model/tool/Turn command.
    pub fn interrupt(&self) -> bool {
        self.interrupt.interrupt()
    }

    /// Replaces the model session at a serial execution boundary while preserving historical context.
    /// The supplied factory is invoked only after the old session closes successfully.
    ///
    /// # Errors
    /// Returns resource cleanup or factory failure. Failed construction leaves model execution unavailable
    /// until a later successful replacement; tool tasks and saved history remain owned by this Thread.
    pub async fn replace_model(
        &self,
        factory: crate::model::ModelFactory,
    ) -> Result<(), ThreadError> {
        self.replace_model_with_preparation(factory, None).await
    }

    /// Atomically replaces the model binding and its optional context policy.
    /// # Errors
    /// Retains existing model cleanup failure and rejects replacement during pending execution.
    pub async fn replace_model_with_preparation(
        &self,
        factory: crate::model::ModelFactory,
        preparation: Option<context_preparation::ContextPreparer>,
    ) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::ReplaceModel(factory, preparation, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Installs a policy before admitting new work; running requests keep their frozen policy.
    /// # Errors
    /// Rejects a closed owner.
    pub async fn set_context_preparation(
        &self,
        preparation: Option<context_preparation::ContextPreparer>,
    ) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::ContextPreparation(preparation, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Installs the resource service lease captured by subsequent prepared model calls.
    ///
    /// # Errors
    /// Rejects a closed or closing owner.
    pub async fn set_resources(
        &self,
        resources: crate::context::ResourceAccess,
    ) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Resources(resources, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Commits application state and its model-visible facts in one atomic checkpoint.
    ///
    /// # Errors
    /// Rejects version conflicts or invalid facts without applying either half of the update.
    pub async fn update_application(
        &self,
        update: extensions::ApplicationUpdate,
    ) -> Result<ThreadSnapshot, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Application(update, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Atomically mutates producer-owned opaque records without projecting them into model context.
    ///
    /// # Errors
    /// Rejects stale revisions, invalid identities, overflow or a closed Thread without partial writes.
    pub async fn mutate_extensions(
        &self,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<std::collections::BTreeMap<String, extensions::ExtensionRecord>, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Extensions(mutations, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Accepts a message idempotently; context is delivered only with a later model admission.
    ///
    /// # Errors
    /// Rejects changed content under an existing ID, invalid identities or a closed owner.
    pub async fn send_message(&self, message: inbox::ThreadMessage) -> Result<u64, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::Message(
                message, None, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Delivers a runtime message and durably requests execution if it remains unconsumed.
    /// A message consumed by an already active Turn does not trigger a second invocation.
    ///
    /// # Errors
    /// Rejects conflicting identities or a closed owner before message acceptance.
    pub async fn send_message_and_resume(
        &self,
        message: inbox::ThreadMessage,
        options: input::InputDriverOptions,
    ) -> Result<u64, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::Message(
                message,
                Some(options),
                reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Accepts a parent-authored message, interrupts active work and continues after cleanup.
    /// Duplicate identities return the original receipt without restarting execution.
    ///
    /// # Errors
    /// Rejects conflicting identities, pending interactions and unavailable admission.
    pub async fn send_message_and_continue(
        &self,
        message: inbox::ThreadMessage,
        options: input::InputDriverOptions,
    ) -> Result<u64, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::ContinueMessage(
                message, options, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Waits for a committed task result; cancelling this wait does not cancel the task.
    ///
    /// # Errors
    /// Rejects unknown tasks, missing terminal deliveries, cancelled waits or a lost owner.
    pub async fn wait_task(
        &self,
        id: &str,
        cancellation: CancellationToken,
    ) -> Result<ToolDelivery, ThreadError> {
        let mut snapshots = self.snapshots.clone();
        loop {
            if cancellation.is_cancelled() {
                return Err(ThreadError::Cancelled);
            }
            {
                let snapshot = snapshots.borrow_and_update();
                let task = snapshot
                    .tasks
                    .get(id)
                    .or_else(|| {
                        snapshot
                            .terminal_tasks
                            .iter()
                            .find(|record| record.id == id)
                    })
                    .ok_or(ThreadError::InvalidIdentity)?;
                if snapshot.pending_tool_commits.contains(&task.call_id) {
                    return Err(ThreadError::PendingToolCommit);
                }
                if task.status != task::TaskStatus::Running {
                    // A settled result is committed history: read the resident queue while the result
                    // is still owed to model context, otherwise the exact committed delivery from the
                    // bounded live effect window. Older results are the host's calls reader.
                    let call_id = task.call_id.as_str();
                    if let Some(delivery) = snapshot
                        .deliveries
                        .iter()
                        .find(|delivery| delivery.call_id == call_id)
                    {
                        return Ok(delivery.clone());
                    }
                    return recent_effect_fact(&self.effect_window, |effect| {
                        effect
                            .deliveries
                            .iter()
                            .rev()
                            .find(|delivery| delivery.call_id == call_id)
                            .cloned()
                    })
                    .ok_or(ThreadError::InvalidOutput);
                }
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(ThreadError::Cancelled),
                changed = snapshots.changed() => changed.map_err(|_| ThreadError::Closed)?,
            }
        }
    }

    /// Retries only result commits retained after execution, without repeating tool side effects.
    ///
    /// # Errors
    /// Returns the original commit boundary failure if the state still cannot accept the result.
    pub async fn retry_tool_commits(&self) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::RetryToolCommits(reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Requests cancellation of this owner's task without waiting for physical completion.
    ///
    /// # Errors
    /// Rejects unknown task identities or an unavailable owner.
    pub async fn cancel_task(
        &self,
        id: String,
    ) -> Result<task::TaskCancellationReceipt, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::CancelTask(id, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Resolves one live execution permission without decoding its prompt payload.
    ///
    /// # Errors
    /// Rejects new decisions for stale revisions, expired tasks and replaced policies.
    /// An identical terminal decision only returns its saved receipt; it never grants a new execution lease.
    pub async fn resolve_execution_permission(
        &self,
        resolution: permissions::PermissionResolution,
    ) -> Result<permissions::PermissionRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(mailbox::MailboxCommand::ResolvePermission(
                resolution, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Atomically selects queue or current-Turn delivery and records the immutable input.
    ///
    /// # Errors
    /// Rejects incompatible routing policy, duplicate identities with different content and storage pressure.
    pub async fn submit_input_with_policy(
        &self,
        submission: input::InputSubmission,
    ) -> Result<input::InputRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(mailbox::MailboxCommand::SubmitInput(submission, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Records an opaque host interaction idempotently without parsing its contents.
    ///
    /// # Errors
    /// Rejects identity conflicts or a closed owner.
    pub async fn request_interaction(
        &self,
        request: interactions::InteractionRequest,
    ) -> Result<interactions::InteractionRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::RequestInteraction(request, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Resolves an interaction and commits its actual context in the same checkpoint.
    ///
    /// # Errors
    /// Rejects stale or terminal interactions, invalid context and pending tool batches.
    pub async fn resolve_interaction(
        &self,
        resolution: interactions::InteractionResolution,
    ) -> Result<interactions::InteractionRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::ResolveInteraction(resolution, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Cancels a pending interaction and atomically appends its cancellation context.
    /// Repeating the same successful command returns the original terminal record.
    ///
    /// # Errors
    /// Rejects stale revisions, answered interactions, pending tool batches or a closed owner.
    pub async fn cancel_interaction(
        &self,
        cancellation: interactions::InteractionCancellation,
    ) -> Result<interactions::InteractionRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::CancelInteraction(cancellation, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Sets admission limits for subsequent prepared calls.
    ///
    /// # Errors
    /// Returns an unavailable or closing owner.
    pub async fn set_capacity(&self, capacity: ContextCapacity) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Capacity(capacity, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Reveals deferred declarations without changing their frozen content or rebuilding instances.
    ///
    /// # Errors
    /// Rejects unknown IDs atomically or a closed owner.
    pub async fn reveal_tools(&self, ids: Vec<String>) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Reveal(ids, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Replaces this Thread's registry atomically. Pending calls keep their frozen executors.
    ///
    /// # Errors
    /// Rejects duplicate registrations or a closed owner.
    pub async fn register_tools(
        &self,
        tools: Vec<crate::tool::opaque::Registration>,
    ) -> Result<(), ThreadError> {
        self.transfer_tools(None, tools).await
    }

    /// Replaces the registry only if producer extensions still match the preparation snapshot.
    /// Rejected candidates are closed before returning; pending calls retain frozen executors.
    ///
    /// # Errors
    /// Returns `ExtensionSequenceConflict` without changing the registry when preparation is stale.
    /// Also returns registration/owner errors, or `RejectedTools` retaining failed cleanup for retry.
    pub async fn register_tools_if_extensions(
        &self,
        expected_extension_sequence: u64,
        tools: Vec<crate::tool::opaque::Registration>,
    ) -> Result<(), ThreadError> {
        self.transfer_tools(Some(expected_extension_sequence), tools)
            .await
    }

    async fn transfer_tools(
        &self,
        expected: Option<u64>,
        tools: Vec<crate::tool::opaque::Registration>,
    ) -> Result<(), ThreadError> {
        let batch = crate::tool::opaque::RegistrationBatch::new(tools);
        let (reply, response) = oneshot::channel();
        let result = match self
            .mailbox
            .send(super::mailbox::MailboxCommand::Register(
                expected,
                batch.clone(),
                reply,
            ))
            .await
        {
            Ok(()) => response.await.unwrap_or(Err(ThreadError::Closed)),
            Err(_) => Err(ThreadError::Closed),
        };
        finish_tool_transfer(batch, result).await
    }

    /// Executes one admitted call, attaching its result to the original call and tool role.
    ///
    /// # Errors
    /// Rejects unknown or completed calls, cancellation, and tool failures.
    pub async fn execute_tool(
        &self,
        call_id: String,
        cancellation: CancellationToken,
    ) -> Result<ToolDispatch, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Execute(call_id, cancellation, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Queues the latest facts per source for the next model preparation boundary.
    /// Acceptance never mutates an already admitted model input or waits for the Turn to end.
    /// Pending facts are ephemeral; the host must resupply them after cold activation.
    ///
    /// # Errors
    /// Rejects empty or duplicate source identities and a closing owner.
    pub async fn queue_runtime_facts(&self, facts: Vec<RuntimeFact>) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(super::mailbox::MailboxCommand::QueueRuntimeFacts(
                facts, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Atomically updates only the supplied runtime fact sources, preserving all others.
    /// Empty content clears that source; an empty patch changes no source.
    ///
    /// # Errors
    /// Rejects duplicate or empty sources, pending tool calls, revision overflow and a closed owner.
    pub async fn patch_runtime_facts(
        &self,
        facts: Vec<RuntimeFact>,
    ) -> Result<ContextSnapshot, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::PatchRuntimeFacts(facts, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Supplies the complete current runtime fact set. Unchanged content is not appended again.
    /// Omitted known sources receive an invalidation record with runtime attribution.
    ///
    /// # Errors
    /// Rejects duplicate or empty source identities, revision overflow and a closed owner.
    pub async fn update_facts(
        &self,
        facts: Vec<RuntimeFact>,
    ) -> Result<ContextSnapshot, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::UpdateFacts(facts, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Replaces context after a successful host transformation, retaining both versions.
    /// Tool executors cannot invoke this operation through their restricted CallContext.
    ///
    /// # Errors
    /// Rejects stale revisions, duplicate records, pending tool calls or a closed owner.
    pub async fn replace_context(
        &self,
        replacement: ReplaceContext,
    ) -> Result<ContextSnapshot, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::ReplaceContext(replacement, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Attaches a store once and admits retained commits without waiting for database IO.
    ///
    /// # Errors
    /// Rejects replacement of an attached store or a closed owner.
    pub async fn attach_storage(&self, store: cold::ColdStoreHandle) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::AttachCold(store, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Retries pending admissions and waits for durability without rolling back runtime facts.
    ///
    /// A released Thread already satisfies this barrier: closing only confirms after the admitted
    /// watermark covered the final commit, so live observation can finish projecting a closed
    /// Thread without a live owner.
    ///
    /// # Errors
    /// Returns storage failure, or an unavailable owner that never confirmed a durable close.
    pub async fn flush(&self) -> Result<(), ThreadError> {
        if self.durably_released() {
            return Ok(());
        }
        let (reply, response) = oneshot::channel();
        if self.commands.send(Command::Flush(reply)).await.is_err() {
            return self.settled_flush();
        }
        match response.await {
            Ok(result) => result,
            Err(_) => self.settled_flush(),
        }
    }

    /// Reports whether the owner was released after its admitted effects became durable.
    fn durably_released(&self) -> bool {
        let snapshot = self.snapshot();
        snapshot.lifecycle == ThreadLifecycle::Closed
            && (!snapshot.persistence.attached
                || snapshot.persistence.durable_sequence >= snapshot.commit_sequence)
    }

    fn settled_flush(&self) -> Result<(), ThreadError> {
        if self.durably_released() {
            Ok(())
        } else {
            Err(ThreadError::Closed)
        }
    }

    /// Subscribes to immutable live snapshots without granting mutation or lifecycle authority.
    pub fn subscribe(&self) -> ThreadSubscription {
        ThreadSubscription {
            snapshots: self.snapshots.clone(),
            initial: true,
        }
    }

    /// Reads the transient live effect window after a known sequence.
    ///
    /// The window retains only commits that are not durable yet: once a fixed durable watermark is
    /// confirmed the covered batches are released, so a reader behind [`Self::effect_window_start`]
    /// must resynchronize from durable history instead of expecting the live body.
    ///
    /// # Errors
    /// Rejects a future watermark. Closing execution does not revoke read access.
    pub async fn effect_page(
        &self,
        after: u64,
        limit: std::num::NonZeroUsize,
    ) -> Result<Vec<Arc<ThreadEffectBatch>>, ThreadError> {
        if after > self.snapshot().commit_sequence {
            return Err(ThreadError::InvalidContext);
        }
        // The gap check and the copy share one lock, so a durable release running right now cannot
        // turn a valid request into a silently empty page.
        match self.effect_window.page_after(after, limit.get()) {
            WindowPage::Page(page) => Ok(page),
            WindowPage::Gap => Err(ThreadError::InvalidContext),
        }
    }

    /// Returns every commit still retained by the transient live effect window.
    ///
    /// Durable commits have already been released to history/calls, so this is not a history
    /// reader; it exists for the transient write batches the owner has not handed off yet.
    ///
    /// # Errors
    /// This in-memory read currently cannot fail.
    pub async fn effects(&self) -> Result<Vec<Arc<ThreadEffectBatch>>, ThreadError> {
        Ok(self.effect_window.retained())
    }

    /// Captures current owner state after the caller fixed and flushed the matching history fence.
    pub fn checkpoint(&self, history_fence: u64) -> Result<ThreadCheckpoint, ThreadError> {
        let state = self.snapshot();
        if history_fence > state.commit_sequence {
            return Err(ThreadError::InvalidContext);
        }
        Ok(ThreadCheckpoint::capture(
            self.thread_id.clone(),
            history_fence,
            state,
        ))
    }

    /// Returns the first effect sequence the live window can still serve.
    ///
    /// Every released commit below it is durable, so a consumer that falls behind this frontier
    /// resynchronizes from durable history/calls before it continues consuming effects.
    pub fn effect_window_start(&self) -> Option<u64> {
        self.effect_window.start()
    }

    /// Returns the last atomic commit without waiting for an in-flight model operation.
    pub fn snapshot(&self) -> ThreadSnapshot {
        self.snapshots.borrow().clone()
    }

    /// Runs a bounded model/tool loop without interleaving another submitted Turn.
    ///
    /// # Errors
    /// Returns model/admission/cancellation errors; committed tool failures remain available to the next step.
    pub async fn run_turn(&self, input: TurnInput) -> Result<TurnCompletion, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Turn(input, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Retries the latest failed attempt with the exact admitted context and frozen executors.
    ///
    /// # Errors
    /// Rejects changed context, revoked/changed tool declarations, completed source attempts and reused IDs.
    pub async fn retry_attempt(
        &self,
        source: String,
        attempt: String,
        cancellation: CancellationToken,
    ) -> Result<ModelStepOutput, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Retry {
                source,
                attempt,
                cancellation,
                reply,
            })
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Submits a model step to the serial owner.
    ///
    /// # Errors
    /// Returns admission, model or output-validation failures. Admitted failures remain in history.
    pub async fn step(&self, input: StepInput) -> Result<ModelStepOutput, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Step(input, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Drains accepted commands and closes the model. A failed close keeps the owner available.
    ///
    /// # Errors
    /// Returns model close failure or an unavailable owner.
    pub async fn close(&self) -> Result<(), ThreadError> {
        self.interrupt.begin_close();
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Close(reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }
}

async fn finish_tool_transfer<T>(
    batch: crate::tool::opaque::RegistrationBatch,
    result: Result<T, ThreadError>,
) -> Result<T, ThreadError> {
    if let Some(tools) = batch.take() {
        let rejection = result.err().unwrap_or(ThreadError::Closed);
        return match crate::tool::opaque::close_rejected(tools).await {
            Ok(()) => Err(rejection),
            Err(error) => Err(ThreadError::RejectedTools(Box::new(
                error.with_rejection(rejection),
            ))),
        };
    }
    result
}
