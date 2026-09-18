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
        /// 远端项目引用 `~/.ssh/config` 的 Host 别名；本地项目为 NULL。
        pub ssh_alias: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_opened_at: Option<i64>,
        pub closed: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
