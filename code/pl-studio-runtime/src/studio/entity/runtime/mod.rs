//! Agent runtime state and rebuildable projection entities.

use sea_orm::entity::prelude::*;

pub mod agent_runtime_state {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_runtime_states")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub agent_id: String,
        pub revision: i64,
        pub snapshot_json: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_runtime_session {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_runtime_sessions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub agent_id: String,
        #[sea_orm(unique)]
        pub session_id: String,
        pub metadata_json: String,
        pub context_json: String,
        pub usage_json: String,
        pub last_context_tokens: Option<i64>,
        pub trace_sequence: i64,
        pub session_event_sequence: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_pending_input {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_pending_inputs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub agent_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub queue_position: i64,
        pub input_json: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_active_input {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_active_inputs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub agent_id: String,
        pub input_json: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::agent_runtime_state::Entity",
            from = "Column::AgentId",
            to = "super::agent_runtime_state::Column::AgentId",
            on_delete = "Cascade"
        )]
        State,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_turn {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_turns")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub agent_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub turn_id: String,
        pub session_id: String,
        pub status: String,
        pub reason: Option<String>,
        pub usage_json: String,
        pub metadata_json: Option<String>,
        pub started_at: Option<i64>,
        pub finished_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod session_view_snapshot {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_view_snapshots")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub session_id: String,
        pub through_sequence: i64,
        pub snapshot_json: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
