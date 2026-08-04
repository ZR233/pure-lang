use anyhow::Result;
use pl_protocol::ThreadTurnPage;

use crate::studio::StudioRuntime;

impl StudioRuntime {
    pub async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        self.store.list_thread_turns(thread_id, cursor, limit).await
    }
}
