use sea_orm::DatabaseConnection;

mod agent_framework;
pub(super) mod attachment;
mod error;
pub(in crate::studio) mod history;
mod interaction;
mod project;
mod settings;
mod task;
mod thread;

#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
}

pub(in crate::studio) use agent_framework::RecoverablePlan;
pub use error::StudioDatabaseError;
pub(in crate::studio) use thread::ChildThreadSpec;
impl StudioStore {
    pub(crate) fn database(&self) -> &DatabaseConnection {
        &self.db
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
