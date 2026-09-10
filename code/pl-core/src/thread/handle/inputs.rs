//! Durable input admission and serial queue execution commands.
use super::*;

impl ThreadHandle {
    /// Enables actor-owned serial execution of queued inputs. Recovery leaves this disabled.
    /// Failures and interaction/step-limit stops pause execution until explicitly resumed.
    ///
    /// # Errors
    /// Returns a closed owner error without consuming queued input.
    pub async fn resume_inputs(
        &self,
        options: input::InputDriverOptions,
    ) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(crate::thread::mailbox::MailboxCommand::ResumeInputs(
                options, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Pauses future queue execution. Use `interrupt` separately to cancel the active Turn.
    ///
    /// # Errors
    /// Returns an error if the owner is no longer reachable.
    pub async fn pause_inputs(&self) -> Result<(), ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(crate::thread::mailbox::MailboxCommand::PauseInputs(reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)
    }

    /// Saves immutable input and returns its idempotent receipt without waiting for the model.
    ///
    /// # Errors
    /// Rejects conflicting identities, closed owners and cold-storage pressure.
    pub async fn submit_input(
        &self,
        input: input::ThreadInput,
    ) -> Result<input::InputRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(crate::thread::mailbox::MailboxCommand::Input(
                input, None, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Admits input and requests actor-owned queue execution in the same mailbox operation.
    /// The receipt proves admission, not execution; a racing close preserves the pending input.
    /// Repeating a consumed identity does not restart execution.
    ///
    /// # Errors
    /// Rejects input identity conflicts and admission failure before returning a receipt.
    pub async fn submit_input_and_run(
        &self,
        input: input::ThreadInput,
        options: input::InputDriverOptions,
    ) -> Result<input::InputRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(crate::thread::mailbox::MailboxCommand::Input(
                input,
                Some(options),
                reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Explicitly discards a pending input without pretending it was sent to a model.
    ///
    /// # Errors
    /// Rejects missing, consumed or currently preparing input identities.
    pub async fn discard_input(&self, id: String) -> Result<input::InputRecord, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(crate::thread::mailbox::MailboxCommand::DiscardInput(
                id, reply,
            ))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }

    /// Drives the oldest pending input with fresh execution identities after host assembly.
    /// Recovery itself never calls this method. None means the queue is empty.
    ///
    /// # Errors
    /// Returns execution or admission failure. Input stays pending until model-request admission.
    pub async fn run_next_input(
        &self,
        request: input::QueuedTurn,
    ) -> Result<Option<TurnCompletion>, ThreadError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::QueuedTurn(request, reply))
            .await
            .map_err(|_| ThreadError::Closed)?;
        response.await.map_err(|_| ThreadError::Closed)?
    }
}
