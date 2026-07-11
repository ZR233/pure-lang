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
        pub delivery_json: Option<String>,
        pub review_json: Option<String>,
        pub terminal_observed: i32,
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
        pub head_commit: String,
        pub status: String,
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
        pub reason: Option<String>,
        pub budget_limit_kind: Option<String>,
        pub budget_usage_json: Option<String>,
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
        pub sequence: i64,
        pub kind: String,
        pub agent_id: Option<String>,
        pub path: Option<String>,
        pub parent_path: Option<String>,
        pub payload_json: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod studio_message {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "studio_messages")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub turn_id: String,
        pub role: String,
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub completed_at: Option<i64>,
        pub error: Option<String>,
        pub metadata_json: String,
        pub sequence: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod message_part {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "message_parts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub message_id: String,
        pub session_id: String,
        pub turn_id: String,
        pub part_type: String,
        pub part_order: i64,
        pub revision: i64,
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub completed_at: Option<i64>,
        pub error: Option<String>,
        pub text_channel: Option<String>,
        pub activity_group_id: Option<String>,
        pub text: String,
        pub attachments_json: String,
        pub tool_json: Option<String>,
        pub agent_json: Option<String>,
        pub inference_json: Option<String>,
        pub plan_json: Option<String>,
        pub file_json: Option<String>,
        pub usage_json: Option<String>,
        pub synthetic: i32,
        pub ignored: i32,
        pub sequence: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod studio_event {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "studio_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub project_id: Option<String>,
        pub session_id: Option<String>,
        pub turn_id: Option<String>,
        pub sequence: i64,
        pub created_at: i64,
        pub kind: String,
        pub payload_json: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod turn {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "turns")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub status: String,
        pub reason: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub completed_at: Option<i64>,
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

pub mod session_skill {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session_skills")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub skill_name: String,
        pub skill_name_key: String,
        pub source: String,
        pub path: String,
        pub first_turn_id: String,
        pub last_turn_id: String,
        pub last_tool_call_id: String,
        pub activated_at: i64,
        pub updated_at: i64,
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
        pub estimated_costs_json: String,
        pub has_unpriced_usage: i32,
        pub updated_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_runtime_event {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_runtime_events")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub inference_id: String,
        pub agent_id: String,
        pub path: String,
        pub parent_path: Option<String>,
        pub role: String,
        pub model: String,
        pub context_window: Option<i64>,
        pub prompt_tokens: i64,
        pub completion_tokens: i64,
        pub cached_prompt_tokens: i64,
        pub total_tokens: i64,
        pub estimated_costs_json: String,
        pub has_unpriced_usage: i32,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod agent_runtime_snapshot {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "agent_runtime_snapshots")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub session_id: String,
        pub agent_id: String,
        pub path: String,
        pub parent_path: Option<String>,
        pub role: String,
        pub model: String,
        pub context_window: Option<i64>,
        pub latest_context_tokens: i64,
        pub prompt_tokens: i64,
        pub completion_tokens: i64,
        pub cached_prompt_tokens: i64,
        pub total_tokens: i64,
        pub estimated_costs_json: String,
        pub has_unpriced_usage: i32,
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
