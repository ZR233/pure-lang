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
        pub status: String,
        /// Thread realtime notification revision exposed to subscribers.
        pub revision: i64,
        /// ThreadActor compare-and-swap revision. `None` means the durable Thread exists but its
        /// actor has not completed registration yet.
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

pub mod thread_input {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "thread_inputs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub thread_id: String,
        #[sea_orm(unique)]
        pub mail_id: String,
        pub turn_id: String,
        pub content: String,
        pub metadata_json: String,
        pub presentation: String,
        pub state: String,
        pub claimed_turn_id: Option<String>,
        pub checkpoint_seq: Option<i64>,
        pub queue_ordinal: i64,
        pub queued_at: i64,
        pub claimed_at: Option<i64>,
        pub consumed_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::thread::Entity",
            from = "Column::ThreadId",
            to = "super::thread::Column::Id",
            on_delete = "Cascade"
        )]
        Thread,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod turn {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "turns")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub thread_id: String,
        pub ordinal: i64,
        pub revision: i64,
        pub status: String,
        pub phase: Option<String>,
        pub reason: Option<String>,
        pub model_json: Option<String>,
        pub usage_json: String,
        pub failure_json: Option<String>,
        pub budget_limit_json: Option<String>,
        pub rollover_compacted: i32,
        pub rollover_compaction_error: Option<String>,
        pub metadata_json: Option<String>,
        pub started_at: Option<i64>,
        pub updated_at: i64,
        pub completed_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::thread::Entity",
            from = "Column::ThreadId",
            to = "super::thread::Column::Id",
            on_delete = "Cascade"
        )]
        Thread,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod item {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "items")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub thread_id: String,
        pub turn_id: String,
        pub ordinal: i64,
        pub revision: i64,
        pub item_kind: String,
        pub status: String,
        pub payload_json: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub completed_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::thread::Entity",
            from = "Column::ThreadId",
            to = "super::thread::Column::Id",
            on_delete = "Cascade"
        )]
        Thread,
        #[sea_orm(
            belongs_to = "super::turn::Entity",
            from = "Column::TurnId",
            to = "super::turn::Column::Id",
            on_delete = "Cascade"
        )]
        Turn,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod thread_context_segment {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "thread_context_segments")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub thread_id: String,
        pub ordinal: i64,
        pub revision: i64,
        pub kind: String,
        pub payload_json: String,
        pub payload_hash: String,
        pub resulting_hash: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::thread::Entity",
            from = "Column::ThreadId",
            to = "super::thread::Column::Id",
            on_delete = "Cascade"
        )]
        Thread,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod thread_session_state {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "thread_session_state")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub thread_id: String,
        pub revision: i64,
        pub state_json: String,
        pub state_hash: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::thread::Entity",
            from = "Column::ThreadId",
            to = "super::thread::Column::Id",
            on_delete = "Cascade"
        )]
        Thread,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
