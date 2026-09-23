//! One idle transaction for host-owned state, instruction and tool changes.
use super::*;

/// A host-selected transaction. Content meanings and tool group membership remain outside core.
#[derive(Debug)]
pub struct IdleReconfiguration {
    pub expected_sequence: u64,
    pub application: extensions::ApplicationUpdate,
    pub context: Option<ReplaceContext>,
    pub model_update: Option<DeferredModelUpdate>,
    pub replace_tools: bool,
    pub remove_tools: Vec<String>,
    pub tools: Vec<crate::tool::opaque::Registration>,
}

impl Owner {
    pub(super) fn reconfigure(
        &mut self,
        update: IdleReconfiguration,
    ) -> Result<ThreadSnapshot, ThreadError> {
        self.tools.retain_candidates(&update.tools);
        if self.interrupt.is_closing() || self.state.lifecycle != ThreadLifecycle::Open {
            return Err(ThreadError::Closed);
        }
        if self.state.commit_sequence != update.expected_sequence {
            return Err(ThreadError::ContextConflict {
                expected: update.expected_sequence,
                actual: self.state.commit_sequence,
            });
        }
        if !self.active_inputs.is_empty()
            || self
                .state
                .turns
                .iter()
                .any(|turn| turn.state == TurnState::Running)
            || self
                .state
                .inputs
                .iter()
                .any(|input| input.state == input::InputState::Pending)
            || self
                .state
                .tasks
                .values()
                .any(|task| task.status == task::TaskStatus::Running)
            || !self.pending.is_empty()
            || !self.uncommitted_tools.is_empty()
        {
            return Err(ThreadError::InputRequiresIdle);
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
                .any(|record| matches!(record.state, permissions::PermissionState::Pending))
        {
            return Err(ThreadError::PendingInteraction);
        }
        let mut candidate = self.state.clone();
        let pending_model_update = update
            .model_update
            .map(|update| self.pending_model_update(update))
            .transpose()?;
        extensions::stage_extensions(&mut candidate, update.application.mutations)?;
        facts::stage_facts(&mut candidate, update.application.facts)?;
        if let Some(mut replacement) = update.context {
            if replacement.expected_revision != self.state.context.revision {
                return Err(ThreadError::ContextConflict {
                    expected: replacement.expected_revision,
                    actual: self.state.context.revision,
                });
            }
            candidate.context = self.state.context.clone();
            replacement.expected_revision = candidate.context.revision;
            replacement::stage_replacement(&mut candidate, replacement)?;
        }
        if update.replace_tools {
            self.tools.replace(update.tools)?;
        } else {
            self.tools.patch(&update.remove_tools, update.tools)?;
        }
        candidate.discovered_tools = self.tools.discovery();
        self.state = candidate;
        if let Some(pending_model_update) = pending_model_update {
            self.pending_model_update = pending_model_update;
        }
        self.retry_plan = None;
        self.publish();
        Ok(self.state.clone())
    }
}
