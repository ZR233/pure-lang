use anyhow::Result;
use pl_protocol::{AgentWorkingState, ConversationRecoveryState};

use crate::studio::StudioStore;
use crate::studio::store::object::load_object;

impl StudioStore {
    pub(crate) async fn conversation_recovery_state(
        &self,
        thread_id: &str,
    ) -> Result<ConversationRecoveryState> {
        let Some(state) = load_object::<AgentWorkingState>(&self.db, thread_id).await? else {
            return Ok(ConversationRecoveryState::default());
        };
        Ok(state.conversation_recovery)
    }
}
