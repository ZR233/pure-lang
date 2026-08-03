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
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod task_run {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "task_runs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub phase: String,
        pub plan: String,
        pub workspace_root: String,
        pub git_common_dir: String,
        pub branch: String,
        pub base_commit: String,
        pub expected_head: String,
        pub design_commit: Option<String>,
        pub status_message: Option<String>,
        pub stop_requested: i32,
        pub stop_requested_origin: Option<String>,
        pub stop_requested_reason: Option<String>,
        pub stop_requested_at: Option<i64>,
        pub task_generation: i64,
        pub terminal_generation: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod work_unit {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "work_units")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub title: String,
        pub status: String,
        pub owned_paths_json: String,
        pub base_commit: String,
        pub worktree_path: String,
        pub branch: String,
        pub worktree_disposition: String,
        pub attempt: i32,
        pub agent_id: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_outcome {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_outcomes")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub work_unit_id: Option<String>,
        pub agent_id: String,
        pub owner_path: String,
        pub initiated_by: String,
        pub requested_by_call_id: String,
        pub role: String,
        pub status: String,
        pub attempt: i32,
        pub summary: Option<String>,
        pub error: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod merge_record {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "merge_records")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub agent_id: String,
        pub status: String,
        pub expected_head: String,
        pub source_commit: String,
        pub conflict_files_json: String,
        pub resolution_summary: Option<String>,
        pub verification_json: Option<String>,
        pub attempt: i32,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod review_round {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "review_rounds")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub round: i32,
        pub scope: String,
        pub work_unit_id: Option<String>,
        pub completion_id: Option<String>,
        pub completion_revision: Option<i32>,
        pub reviewed_head: String,
        pub status: String,
        pub requested_by_call_id: String,
        pub reviewer_agent_id: Option<String>,
        pub summary: Option<String>,
        pub design_references_json: String,
        pub findings_json: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod work_completion {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "work_completions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub work_unit_id: String,
        pub executor_agent_id: String,
        pub revision: i32,
        pub kind: String,
        pub status: String,
        pub base_commit: String,
        pub head_commit: Option<String>,
        pub changed_files_json: String,
        pub verification_summary: String,
        pub worktree_path: String,
        pub branch: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod branch_lease {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "branch_leases")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub git_common_dir: String,
        pub branch: String,
        pub expected_head: String,
        pub acquired_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

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
