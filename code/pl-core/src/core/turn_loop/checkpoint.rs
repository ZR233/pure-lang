use pl_protocol::Result;

use crate::session::AgentSession;
use crate::trace::TraceRecorder;
use crate::turn::TurnOptions;

pub(super) async fn persist(
    options: &TurnOptions,
    session: &AgentSession,
    reason: crate::TurnCheckpointReason,
) -> Result<()> {
    let Some(checkpoint) = &options.checkpoint else {
        return Ok(());
    };
    let consumed_mail_ids = match &options.mailbox {
        Some(mailbox) => mailbox.pending_acknowledgements().await,
        None => Vec::new(),
    };
    checkpoint
        .checkpoint_mailbox(session.clone(), reason, consumed_mail_ids.clone())
        .await
        .map_err(|error| pl_protocol::PureError::MemoryError(error.to_string()))?;
    if let Some(mailbox) = &options.mailbox {
        mailbox.acknowledge(&consumed_mail_ids).await;
    }
    Ok(())
}

pub(super) async fn persist_pending_mail(
    options: &TurnOptions,
    session: &AgentSession,
) -> Result<()> {
    let Some(mailbox) = &options.mailbox else {
        return Ok(());
    };
    if mailbox.pending_acknowledgements().await.is_empty() {
        return Ok(());
    }
    persist(
        options,
        session,
        crate::TurnCheckpointReason::MailboxInputConsumed,
    )
    .await
}

pub(super) async fn drain_mailbox(
    options: &TurnOptions,
    session: &mut AgentSession,
    recorder: &mut TraceRecorder,
    turn_id: &str,
) -> Result<bool> {
    let Some(mailbox) = &options.mailbox else {
        return Ok(false);
    };
    let inputs = mailbox.drain().await;
    if inputs.is_empty() {
        return Ok(false);
    }
    for input in inputs {
        session.push_user_prompt_with_presentation(
            input.payload.message.clone(),
            input.payload.presentation,
        );
        recorder.user_text_item_with_id(
            turn_id,
            format!("{turn_id}-mail-{}", input.mail_id),
            input.payload.message,
            Vec::new(),
        );
    }
    persist(
        options,
        session,
        crate::TurnCheckpointReason::MailboxInputConsumed,
    )
    .await?;
    Ok(true)
}

pub(super) async fn finish_mailbox_window(
    options: &TurnOptions,
    session: &mut AgentSession,
    recorder: &mut TraceRecorder,
    turn_id: &str,
) -> Result<bool> {
    if drain_mailbox(options, session, recorder, turn_id).await? {
        return Ok(true);
    }
    persist(options, session, crate::TurnCheckpointReason::Terminal).await?;
    drain_mailbox(options, session, recorder, turn_id).await
}
