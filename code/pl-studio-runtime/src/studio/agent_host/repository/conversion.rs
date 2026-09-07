use super::labels::{agent_state_kind, thread_mode_from_label, thread_status};
use super::store_error;
use crate::PureError;
use crate::studio::entity::thread;
use pl_core::AgentState;
use pl_core::Thread as ThreadRecord;

impl TryFrom<thread::Model> for ThreadRecord {
    type Error = PureError;

    fn try_from(model: thread::Model) -> Result<Self, Self::Error> {
        let state: AgentState = serde_json::from_str(&model.state_json)?;
        if agent_state_kind(&state) != model.state_kind {
            return Err(store_error(format!(
                "Agent state discriminator mismatch: JSON is {}, generated column is {}",
                agent_state_kind(&state),
                model.state_kind
            )));
        }
        Ok(Self {
            id: model.id,
            project_id: model.project_id,
            title: model.title,
            mode: thread_mode_from_label(&model.mode)?,
            root_thread_id: model.root_thread_id,
            parent_thread_id: model.parent_thread_id,
            role: model.role,
            agent_path: model.agent_path,
            status: thread_status(&state),
            created_at: model.created_at,
            updated_at: model.updated_at,
            archived: model.archived != 0,
        })
    }
}
