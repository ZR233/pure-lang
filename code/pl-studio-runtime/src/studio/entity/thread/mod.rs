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
        /// Canonical serialized [`pl_core::AgentState`].
        pub state_json: String,
        /// SQLite generated discriminator derived from `state_json`.
        pub state_kind: String,
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
        pub attachments_json: String,
        pub metadata_json: String,
        pub presentation: String,
        pub state_json: String,
        pub state_kind: String,
        pub queue_ordinal: i64,
        pub queued_at: i64,
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

pub mod thread_submission {
    use super::*;

    /// 子代理向主代理汇报的 durable 阶段提交记录。
    ///
    /// 每次 `report_progress` 调用追加一行；主代理通过只读查询工具按 thread 全量
    /// 拉取，不依赖子代理 push。生命周期隶属于该 thread（主 agent 会话树），
    /// 子代理关闭后行保留，可继续查询。
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "thread_submissions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub thread_id: String,
        pub ordinal: i64,
        pub stage: String,
        pub summary: String,
        pub next_step: String,
        pub detail: Option<String>,
        pub revision: i64,
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
        pub state_json: String,
        pub state_kind: String,
        pub model_json: Option<String>,
        pub usage_json: String,
        pub metadata_json: Option<String>,
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
        /// Category-specific canonical [`pl_protocol::ThreadItemState`].
        pub state_json: String,
        /// SQLite generated discriminator derived from `state_json`.
        pub state_kind: String,
        pub created_at: i64,
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
