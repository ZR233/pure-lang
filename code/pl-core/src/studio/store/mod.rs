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
mod tests;
