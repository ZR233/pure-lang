//! Canonical Thread, Turn, Item and input entities.

use sea_orm::entity::prelude::*;

pub mod thread {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "threads")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub project_id: String,
        pub title: String,
        pub mode: String,
        pub root_thread_id: String,
        pub parent_thread_id: Option<String>,
        pub role: String,
        #[sea_orm(unique)]
        pub agent_path: String,
        /// Product directory status and preparation error; runtime state belongs to Thread journal.
        pub state_json: String,
        /// SQLite generated discriminator derived from `state_json`.
        pub state_kind: String,
        /// Thread realtime notification revision exposed to subscribers.
        pub revision: i64,
        /// Product association receipt: `Some(1)` means an actor was registered.
        /// The actual runtime revision belongs exclusively to the core session database.
        pub runtime_revision: Option<i64>,
        pub event_sequence: i64,
        pub metadata_json: String,
        pub usage_json: String,
        pub last_context_tokens: Option<i64>,
        pub trace_sequence: i64,
        pub created_at: i64,
        pub updated_at: i64,
        pub archived: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::super::project::Entity",
            from = "Column::ProjectId",
            to = "super::super::project::Column::Id",
            on_delete = "Cascade"
        )]
        Project,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
