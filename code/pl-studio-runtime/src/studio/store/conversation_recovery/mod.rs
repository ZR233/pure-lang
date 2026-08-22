use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use pl_core::MailboxDeliveryState;
use pl_protocol::{AgentWorkingState, ConversationRecoveryState};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::StudioStore;
use crate::studio::entity::{thread_input, thread_session_state};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConversationTurnInputs {
    pub(crate) messages: Vec<String>,
    pub(crate) hashes: Vec<String>,
}

impl StudioStore {
    pub(crate) async fn conversation_turn_inputs(
        &self,
        thread_id: &str,
        turn_ids: &[String],
    ) -> Result<BTreeMap<String, ConversationTurnInputs>> {
        let selected = turn_ids.iter().collect::<BTreeSet<_>>();
        let rows = thread_input::Entity::find()
            .filter(thread_input::Column::ThreadId.eq(thread_id.to_string()))
            .filter(thread_input::Column::StateKind.eq("consumed"))
            .order_by_asc(thread_input::Column::QueueOrdinal)
            .all(&self.db)
            .await?;
        let mut inputs = BTreeMap::<String, ConversationTurnInputs>::new();
        for row in rows {
            let state: MailboxDeliveryState = serde_json::from_str(&row.state_json)?;
            let turn_id = state
                .turn_id()
                .map(ToString::to_string)
                .ok_or_else(|| anyhow::anyhow!("consumed mailbox has no Turn identity"))?;
            if !selected.contains(&turn_id) {
                continue;
            }
            let entry = inputs
                .entry(turn_id)
                .or_insert_with(|| ConversationTurnInputs {
                    messages: Vec::new(),
                    hashes: Vec::new(),
                });
            entry
                .hashes
                .push(pl_core::canonical_content_hash(row.content.as_bytes()));
            entry.messages.push(row.content);
        }
        Ok(inputs)
    }

    pub(crate) async fn conversation_recovery_state(
        &self,
        thread_id: &str,
    ) -> Result<ConversationRecoveryState> {
        let Some(row) = thread_session_state::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(ConversationRecoveryState::default());
        };
        let actual_hash = pl_core::canonical_content_hash(row.state_json.as_bytes());
        if row.state_hash != actual_hash {
            bail!("Thread session state hash mismatch");
        }
        let state = serde_json::from_str::<AgentWorkingState>(&row.state_json)?;
        Ok(state.conversation_recovery)
    }
}
