//! Complete session-history entities.

use sea_orm::entity::prelude::*;

pub mod session_history_turn {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_history_turns")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false, unique_key = "session_turn_id")]
        pub session_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub turn_sequence: i64,
        #[sea_orm(unique_key = "session_turn_id")]
        pub turn_id: String,
        pub status: String,
        pub model_json: Option<String>,
        pub error_json: Option<String>,
        pub started_at: i64,
        pub completed_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod session_history_item {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_history_items")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false, unique_key = "session_item_id")]
        pub session_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub sequence: i64,
        #[sea_orm(unique_key = "session_item_id")]
        pub item_id: String,
        pub turn_id: String,
        pub item_kind: String,
        pub payload_json: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod session_history_checkpoint {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_history_checkpoints")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub session_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub revision: i64,
        pub through_sequence: i64,
        pub context_json: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
