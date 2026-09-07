use crate::studio::store::StudioStore;
use anyhow::Result;
use pl_core::ThreadTurnPage;

impl StudioStore {
    pub(crate) async fn list_thread_turns(
        &self,
        thread_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ThreadTurnPage> {
        Ok(self.sessions().list_turns(thread_id, cursor, limit).await?)
    }
}
