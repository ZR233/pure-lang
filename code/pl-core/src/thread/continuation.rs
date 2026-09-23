//! Owner-controlled interruption followed by a fresh Turn; admission receipts are not completions.
use super::*;

impl Owner {
    pub(super) fn continue_input(
        &mut self,
        input: input::ThreadInput,
        options: input::InputDriverOptions,
    ) -> Result<input::InputRecord, ThreadError> {
        // Duplicate submissions must not revive a manually paused driver or cancel a newer Turn.
        // Terminal identities stay resident even after their input body is pruned, so the receipt
        // is answered from bounded state instead of reviving a paused driver.
        if let Some(receipt) = input::duplicate_input_receipt(&self.state, &input)? {
            return Ok(receipt);
        }
        self.validate_continuation_admission()?;
        let record = self.accept_input(input)?;
        self.input_batch_through = Some(record.ordinal);
        self.request_continuation(options);
        Ok(record)
    }

    pub(super) fn continue_message(
        &mut self,
        message: inbox::ThreadMessage,
        options: input::InputDriverOptions,
    ) -> Result<u64, ThreadError> {
        // An already accepted identity is answered from its original receipt before any new
        // admission check or continuation action: a repeat is not new work, so it must not be
        // refused because the owner is closing, is waiting for an interaction or is under storage
        // pressure, and it must not interrupt the Turn that is currently running. Only a genuinely
        // new message takes the admission path, which is what warms the driver and interrupts.
        if let Some(sequence) = self.accepted_message_receipt(&message)? {
            return Ok(sequence);
        }
        self.validate_continuation_admission()?;
        let sequence = self.receive_message(message, Some(options))?;
        self.request_continuation(options);
        Ok(sequence)
    }

    fn validate_continuation_admission(&mut self) -> Result<(), ThreadError> {
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if self
            .state
            .interactions
            .values()
            .any(|record| record.state == interactions::InteractionState::Pending)
            || self
                .state
                .permissions
                .values()
                .any(|record| record.state == permissions::PermissionState::Pending)
        {
            return Err(ThreadError::PendingInteraction);
        }
        self.refresh_storage_pressure();
        if self.state.persistence.pressure_paused {
            return Err(ThreadError::StoragePressure);
        }
        if let Some(error) = &self.cold_error {
            return Err(ThreadError::Storage(error.clone()));
        }
        Ok(())
    }

    fn request_continuation(&mut self, options: input::InputDriverOptions) {
        self.input_driver.enable(options);
        if let Some(turn) = self
            .state
            .turns
            .iter()
            .rev()
            .find(|turn| turn.state == TurnState::Running)
        {
            let turn_id = turn.turn_id.clone();
            self.interrupted_turn = Some(turn_id.clone());
            // The receipt remains successful after admission even if cancellation cannot be committed.
            if let Err(error) = self.cancel_turn_tasks(&turn_id) {
                self.input_driver.fail(Arc::new(error));
                self.pause_inputs();
            }
            self.interrupt.interrupt();
        }
        self.publish_snapshot();
    }

    pub(super) fn cancel_turn_tasks(&mut self, turn_id: &str) -> Result<(), ThreadError> {
        let tasks: Vec<_> = self
            .state
            .tasks
            .values()
            .filter(|task| task.turn_id == turn_id && task.status == task::TaskStatus::Running)
            .map(|task| task.id.clone())
            .collect();
        for task in tasks {
            self.cancel_task(&task)?;
        }
        Ok(())
    }

    pub(super) async fn settle_turn_cancellation(&mut self, turn_id: &str) {
        while self
            .state
            .tasks
            .values()
            .any(|task| task.turn_id == turn_id && task.status == task::TaskStatus::Running)
        {
            if self
                .uncommitted_tools
                .values()
                .any(|completion| completion.call.turn_id == turn_id)
            {
                self.input_driver
                    .fail(Arc::new(ThreadError::PendingToolCommit));
                self.pause_inputs();
                break;
            }
            tokio::select! {
                Some(completion) = futures::StreamExt::next(&mut self.background), if !self.background.is_empty() => self.finish_background(completion),
                message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                    if let Some(message) = message { self.process_mailbox(message); }
                }
            }
        }
    }

    /// Wait in the owner loop for physical task completion, servicing the mailbox throughout.
    pub(super) fn continuation_ready(&mut self) -> bool {
        let Some(turn_id) = &self.interrupted_turn else {
            return true;
        };
        if self
            .state
            .turns
            .iter()
            .any(|turn| &turn.turn_id == turn_id && turn.state == TurnState::Running)
            || self
                .state
                .tasks
                .values()
                .any(|task| &task.turn_id == turn_id && task.status == task::TaskStatus::Running)
        {
            if self
                .uncommitted_tools
                .values()
                .any(|completion| &completion.call.turn_id == turn_id)
            {
                self.input_driver
                    .fail(Arc::new(ThreadError::PendingToolCommit));
                self.pause_inputs();
            }
            return false;
        }
        self.interrupted_turn = None;
        self.publish_snapshot();
        true
    }
}
