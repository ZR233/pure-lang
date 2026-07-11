use sea_orm::DatabaseConnection;

mod agent;
pub(super) mod attachment;
mod event_log;
mod interaction;
mod project;
mod projection;
mod runtime_usage;
mod session;
mod settings;
mod skill;
mod task;
mod turn;

pub use attachment::studio_attachment;
#[derive(Clone)]
pub struct StudioStore {
    db: DatabaseConnection,
}

#[cfg(test)]
impl StudioStore {
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
