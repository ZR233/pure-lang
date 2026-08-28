//! Versioned bounded Studio objects.

use sea_orm::entity::prelude::*;

pub mod studio_object {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "studio_objects")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub owner_kind: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub owner_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub object_kind: String,
        pub revision: i64,
        pub schema_version: i64,
        pub payload_json: String,
        pub payload_hash: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
