//! Product-facing project, session, attachment, and interaction entities.

use sea_orm::entity::prelude::*;

pub mod project {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        #[sea_orm(unique)]
        pub path: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_opened_at: Option<i64>,
        pub closed: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod session {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "sessions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub project_id: String,
        pub title: String,
        pub mode: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub archived: i32,
        pub visibility: String,
        pub instruction_snapshot_json: Option<String>,
        pub parent_session_id: Option<String>,
        pub root_session_id: String,
        pub session_kind: String,
        pub owner_agent_id: String,
        pub owner_role: String,
        pub agent_status: String,
        pub agent_summary: Option<String>,
        pub agent_error: Option<String>,
        pub agent_updated_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::project::Entity",
            from = "Column::ProjectId",
            to = "super::project::Column::Id",
            on_delete = "Cascade"
        )]
        Project,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod attachment {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "attachments")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub message_id: Option<String>,
        pub media_type: String,
        pub filename: Option<String>,
        pub storage_path: String,
        pub byte_size: i64,
        pub width: Option<i64>,
        pub height: Option<i64>,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::session::Entity",
            from = "Column::SessionId",
            to = "super::session::Column::Id",
            on_delete = "Cascade"
        )]
        Session,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod interaction {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "interactions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub turn_id: String,
        pub item_id: Option<String>,
        pub tool_id: Option<String>,
        pub agent_path: Option<String>,
        pub kind: String,
        pub status: String,
        pub payload_json: String,
        pub resolution_json: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub resolved_at: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::session::Entity",
            from = "Column::SessionId",
            to = "super::session::Column::Id",
            on_delete = "Cascade"
        )]
        Session,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
