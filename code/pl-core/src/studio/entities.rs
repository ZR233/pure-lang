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
        pub created_at: i64,
        pub updated_at: i64,
        pub last_opened_at: Option<i64>,
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
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod message {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "messages")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub role: String,
        pub content: String,
        pub reasoning_content: Option<String>,
        pub metadata_json: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod tool_approval {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "tool_approvals")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub tool_call_id: String,
        pub tool_name: String,
        pub arguments_json: String,
        pub working_directory: Option<String>,
        pub decision: String,
        pub reason: Option<String>,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[allow(dead_code)]
pub mod agent {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agents")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub path: String,
        pub parent_path: Option<String>,
        pub role: String,
        pub task: String,
        pub status: String,
        pub summary: Option<String>,
        pub error: Option<String>,
        pub depth: i32,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_event {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub agent_id: String,
        pub path: String,
        pub parent_path: Option<String>,
        pub role: String,
        pub task: String,
        pub status: String,
        pub summary: Option<String>,
        pub error: Option<String>,
        pub reason: Option<String>,
        pub budget_limit_kind: Option<String>,
        pub budget_usage_json: Option<String>,
        pub depth: i32,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod trace_event {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "trace_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub sequence: i64,
        pub timestamp: i64,
        pub kind: String,
        pub payload_json: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod session_runtime_snapshot {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_runtime_snapshots")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub session_id: String,
        pub model: String,
        pub context_window: Option<i64>,
        pub latest_context_tokens: i64,
        pub prompt_tokens: i64,
        pub completion_tokens: i64,
        pub cached_prompt_tokens: i64,
        pub total_tokens: i64,
        pub currency: Option<String>,
        pub estimated_cost: Option<f64>,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod app_setting {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "app_settings")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub key: String,
        pub value: String,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
