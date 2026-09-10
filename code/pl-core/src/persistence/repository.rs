use super::{SessionStoreError, SqliteSessionStore, sqlite};
use sea_orm::ConnectionTrait;
impl SqliteSessionStore {
    /// Lists durable session identities without activating any actors.
    ///
    /// # Errors
    /// Returns database or identity decoding errors.
    pub async fn session_ids(&self) -> Result<Vec<String>, SessionStoreError> {
        let rows = self
            .owner
            .shared
            .db
            .query_all_raw(sqlite::statement(
                "SELECT DISTINCT session_id FROM session_entries ORDER BY session_id",
                vec![],
            ))
            .await?;
        rows.into_iter()
            .map(|row| Ok(row.try_get("", "session_id")?))
            .collect()
    }

    /// Reads opaque records including unknown extension types, in stable creation order.
    ///
    /// # Errors
    /// Returns database or malformed envelope errors. This never rewrites payloads.
    pub async fn read_entries(
        &self,
        session_id: &str,
        type_id: Option<&str>,
    ) -> Result<Vec<crate::storage::SessionEntry>, SessionStoreError> {
        sqlite::entries(&self.owner.shared.db, session_id, type_id).await
    }
}
