//! Task lifecycle, allocation, review, merge, and branch-lease entities.

use sea_orm::entity::prelude::*;

pub mod task_run {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "task_runs")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub root_thread_id: String,
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
        pub terminal_failure_id: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "crate::studio::entity::thread::Entity",
            from = "Column::RootThreadId",
            to = "crate::studio::entity::thread::Column::Id",
            on_delete = "Cascade"
        )]
        RootThread,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod task_failure {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "task_failures")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub task_run_id: String,
        pub source_thread_id: String,
        pub source_turn_id: String,
        pub source_agent_id: String,
        pub source_role: String,
        pub work_unit_id: Option<String>,
        pub review_round_id: Option<String>,
        pub disposition: String,
        pub failure_json: String,
        pub resolved_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
    }

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
        pub scope_hints_json: String,
        pub base_commit: String,
        pub worktree_path: String,
        pub branch: String,
        pub worktree_disposition: String,
        pub attempt: i32,
        pub executor_thread_id: Option<String>,
        pub requested_by_call_id: String,
        pub execution_status: String,
        pub execution_summary: Option<String>,
        pub execution_error: Option<String>,
        pub budget_limit_json: Option<String>,
        pub budget_slice_count: i32,
        pub continuation_state: String,
        pub continuation_source_turn_id: Option<String>,
        pub continuation_revision: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
    }

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
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
        #[sea_orm(
            belongs_to = "super::work_unit::Entity",
            from = "Column::WorkUnitId",
            to = "super::work_unit::Column::Id",
            on_delete = "Cascade"
        )]
        WorkUnit,
    }

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
        pub reviewer_thread_id: Option<String>,
        pub reviewer_status: String,
        pub reviewer_error: Option<String>,
        pub summary: Option<String>,
        pub design_references_json: String,
        pub findings_json: String,
        pub created_at: i64,
        pub updated_at: i64,
        // Keep this field last so fresh v7 DDL matches SQLite's v6 -> v7 ALTER TABLE result.
        pub file_reviews_json: Option<String>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
        #[sea_orm(
            belongs_to = "super::work_unit::Entity",
            from = "Column::WorkUnitId",
            to = "super::work_unit::Column::Id",
            on_delete = "Cascade"
        )]
        WorkUnit,
        #[sea_orm(
            belongs_to = "super::work_completion::Entity",
            from = "Column::CompletionId",
            to = "super::work_completion::Column::Id",
            on_delete = "Cascade"
        )]
        Completion,
    }

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
        pub work_unit_id: String,
        pub completion_id: String,
        pub completion_revision: i32,
        pub executor_agent_id: String,
        pub expected_previous_head: String,
        pub resulting_head: String,
        pub delivery_head: String,
        pub method: String,
        pub summary: String,
        pub cleanup_status: String,
        pub cleanup_detail: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
        #[sea_orm(
            belongs_to = "super::work_unit::Entity",
            from = "Column::WorkUnitId",
            to = "super::work_unit::Column::Id",
            on_delete = "Cascade"
        )]
        WorkUnit,
        #[sea_orm(
            belongs_to = "super::work_completion::Entity",
            from = "Column::CompletionId",
            to = "super::work_completion::Column::Id",
            on_delete = "Cascade"
        )]
        Completion,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod branch_lease {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "branch_leases")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(unique)]
        pub task_run_id: String,
        pub git_common_dir: String,
        pub branch: String,
        pub expected_head: String,
        pub acquired_at: i64,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::task_run::Entity",
            from = "Column::TaskRunId",
            to = "super::task_run::Column::Id",
            on_delete = "Cascade"
        )]
        TaskRun,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
