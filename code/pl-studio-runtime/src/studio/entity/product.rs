//! Product-facing project, attachment, and interaction entities.

use sea_orm::entity::prelude::*;

pub mod project {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "projects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        pub path: String,
        pub ssh_server_id: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_opened_at: Option<i64>,
        pub closed: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::ssh_server::Entity",
            from = "Column::SshServerId",
            to = "super::ssh_server::Column::Id",
            on_delete = "Restrict"
        )]
        SshServer,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod ssh_server {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "ssh_servers")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        pub host: String,
        pub port: i32,
        pub username: String,
        pub auth_json: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
