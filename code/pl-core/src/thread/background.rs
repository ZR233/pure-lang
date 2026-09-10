//! Background ownership and immutable result delivery through the Thread inbox.
use super::*;

impl Owner {
    pub(super) fn acknowledge_task(
        &mut self,
        call_id: &str,
    ) -> Result<task::TaskRecord, ThreadError> {
        let mut task = self
            .state
            .tasks
            .get(&format!("task:{call_id}"))
            .cloned()
            .ok_or(ThreadError::InvalidIdentity)?;
        if task.acknowledgement.is_some() || task.status != task::TaskStatus::Running {
            return Err(ThreadError::InvalidIdentity);
        }
        let revision = self
            .state
            .context
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        let id = unique_id(
            "task-receipt",
            self.state
                .context
                .records
                .iter()
                .map(|record| record.id.as_str()),
        )?;
        let content = vec![ContextContent::Text {
            text: Arc::from(format!(
                "Task {} is running. Its result will be delivered when execution finishes.",
                task.id
            )),
        }];
        let mut records = self.state.context.records.to_vec();
        records.push(ContextRecord {
            id: id.clone(),
            turn_id: Some(task.turn_id.clone()),
            source: ContextSource::ToolResult {
                call_id: task.call_id.clone(),
                tool_id: task.tool_id.clone(),
            },
            content,
            tool_calls: Vec::new(),
        });
        let context = ContextSnapshot {
            revision,
            records: records.into(),
        };
        context.pending_calls()?;
        task.revision = task
            .revision
            .checked_add(1)
            .ok_or(ThreadError::RevisionExhausted)?;
        task.acknowledgement = Some(task::TaskAcknowledgement {
            record_id: id,
            context_revision: revision,
        });
        self.state.context = context;
        task::record_change(&mut self.state, task.clone());
        self.publish();
        Ok(task)
    }

    pub(super) fn finish_background(
        &mut self,
        completion: super::tool_execution::ToolExecutionCompletion,
    ) {
        if let Err(error) = self.commit_tool_execution(completion)
            && !matches!(error, ThreadError::Tool(_) | ThreadError::Cancelled)
        {
            tracing::error!(thread = self.id, %error, "background result commit failed");
        }
    }

    pub(super) async fn drain_background(&mut self) {
        while !self.background.is_empty() {
            tokio::select! {
                Some(completion) = futures::StreamExt::next(&mut self.background) => self.finish_background(completion),
                message = self.mailbox.recv(), if !self.mailbox.is_closed() || !self.mailbox.is_empty() => {
                    if let Some(message) = message { self.process_mailbox(message); }
                },
            }
        }
    }
}

pub(super) fn append_result_message(
    state: &mut ThreadSnapshot,
    task: &task::TaskRecord,
    output: &crate::tool::ToolOutput,
    context: Vec<ContextContent>,
) -> Result<String, ThreadError> {
    let sequence = state
        .inbox
        .last()
        .map_or(0, |record| record.sequence)
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    let id = unique_id(
        "task-result",
        state.inbox.iter().map(|record| record.message.id.as_str()),
    )?;
    let message = inbox::ThreadMessage {
        id: id.clone(),
        source_id: task.id.clone(),
        payload: output.payload().clone(),
        context,
    };
    let mut inbox = state.inbox.to_vec();
    inbox.push(inbox::InboxRecord { sequence, message });
    state.inbox = inbox.into();
    Ok(id)
}

pub(super) fn result_context(
    task: &task::TaskRecord,
    status: task::TaskStatus,
    mut content: Vec<ContextContent>,
) -> Vec<ContextContent> {
    content.insert(
        0,
        ContextContent::Text {
            text: Arc::from(format!(
                "Task {} for tool {} finished: {status:?}.",
                task.id, task.tool_id
            )),
        },
    );
    content
}

pub(super) fn unique_id<'a>(
    prefix: &str,
    existing: impl Iterator<Item = &'a str>,
) -> Result<String, ThreadError> {
    let existing = existing.collect::<std::collections::BTreeSet<_>>();
    (0..=existing.len())
        .map(|index| format!("{prefix}:{index}"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .ok_or(ThreadError::InvalidIdentity)
}
