use sea_orm::DatabaseConnection;

mod agent_framework;
pub(super) mod attachment;
mod error;
pub(in crate::studio) mod history;
mod interaction;
mod project;
mod session;
mod settings;
mod task;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
    history_db: DatabaseConnection,
    history_writer_db: DatabaseConnection,
}

pub(in crate::studio) use agent_framework::RecoverablePlan;
pub use error::StudioDatabaseError;
pub(in crate::studio) use session::AgentSessionSpec;
impl StudioStore {
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
    }

    pub(crate) fn history_database(&self) -> &DatabaseConnection {
        &self.history_db
    }

    pub(crate) fn history_writer_database(&self) -> &DatabaseConnection {
        &self.history_writer_db
    }

    #[cfg(test)]
    pub(crate) async fn execute_test_sql(&self, sql: &str) {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        self.db
            .execute_raw(Statement::from_string(
                DatabaseBackend::Sqlite,
                sql.to_string(),
            ))
            .await
            .unwrap();
    }
}

#[cfg(test)]
mod tests;
