use sea_orm::DatabaseConnection;

mod agent_framework;
pub(super) mod attachment;
mod error;
mod interaction;
mod project;
mod session;
mod settings;
mod task;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
}

pub use error::StudioDatabaseError;
pub(in crate::studio) use session::AgentSessionSpec;
impl StudioStore {
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    #[cfg(test)]
    pub(crate) async fn execute_test_sql(&self, sql: &str) {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        self.db
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                sql.to_string(),
            ))
            .await
            .unwrap();
    }
}

#[cfg(test)]
mod tests;
