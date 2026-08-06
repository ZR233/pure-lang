use anyhow::Result;
use pl_core::canonical_content_hash;
use pl_protocol::{ThreadItem, ThreadItemContent, ThreadItemStatus};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::StudioStore;
use crate::studio::entity::{
    item, thread, thread_context_segment, thread_input, thread_session_state, turn,
};
use crate::studio::ids::unix_seconds;

pub(in crate::studio) struct RecoverablePlan {
    pub turn_id: String,
    pub item_id: String,
    pub content: String,
}

pub(in crate::studio) struct RecoverableTaskPlan {
    pub agent_id: String,
    pub thread_id: String,
    pub plan: RecoverablePlan,
}

pub(in crate::studio) struct ThreadRuntimeSeed {
    pub thread_revision: u64,
    pub runtime_revision: u64,
    pub event_sequence: u64,
}

impl StudioStore {
    pub(in crate::studio) async fn reset_agent_sessions_for_root(
        &self,
        root_thread_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let threads = thread::Entity::find()
                .filter(thread::Column::RootThreadId.eq(root_thread_id))
                .order_by_asc(thread::Column::CreatedAt)
                .order_by_asc(thread::Column::Id)
                .all(&tx)
                .await?;
            anyhow::ensure!(!threads.is_empty(), "Thread reset target not found");
            let now = unix_seconds();
            let state = pl_protocol::AgentWorkingState::default();
            let state_json = serde_json::to_string(&state)?;
            let state_hash = canonical_content_hash(state_json.as_bytes());
            for thread_row in threads {
                thread_context_segment::Entity::delete_many()
                    .filter(thread_context_segment::Column::ThreadId.eq(&thread_row.id))
                    .exec(&tx)
                    .await?;
                thread_session_state::Entity::delete_by_id(thread_row.id.clone())
                    .exec(&tx)
                    .await?;
                thread_session_state::ActiveModel {
                    thread_id: Set(thread_row.id.clone()),
                    revision: Set(0),
                    state_json: Set(state_json.clone()),
                    state_hash: Set(state_hash.clone()),
                    updated_at: Set(now),
                }
                .insert(&tx)
                .await?;

                let inputs = thread_input::Entity::find()
                    .filter(thread_input::Column::ThreadId.eq(&thread_row.id))
                    .filter(thread_input::Column::State.ne("consumed"))
                    .all(&tx)
                    .await?;
                for input in inputs {
                    let mut active = input.into_active_model();
                    active.state = Set("consumed".to_string());
                    active.consumed_at = Set(Some(now));
                    active.update(&tx).await?;
                }

                let active_turns = turn::Entity::find()
                    .filter(turn::Column::ThreadId.eq(&thread_row.id))
                    .filter(turn::Column::Status.is_in(["queued", "inProgress"]))
                    .all(&tx)
                    .await?;
                for turn in active_turns {
                    let mut active = turn.into_active_model();
                    active.status = Set("interrupted".to_string());
                    active.phase = Set(None);
                    active.reason = Set(Some("recovery context reset".to_string()));
                    active.failure_json = Set(None);
                    active.budget_limit_json = Set(None);
                    active.rollover_compacted = Set(0);
                    active.rollover_compaction_error = Set(None);
                    active.updated_at = Set(now);
                    active.completed_at = Set(Some(now));
                    active.update(&tx).await?;
                }

                let is_root = thread_row.id == root_thread_id;
                let mut active = thread_row.into_active_model();
                active.runtime_revision = Set(None);
                active.status = Set(if is_root { "idle" } else { "closed" }.to_string());
                active.last_context_tokens = Set(None);
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(in crate::studio) async fn thread_runtime_seed(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRuntimeSeed> {
        let row = thread::Entity::find_by_id(thread_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Thread runtime seed not found"))?;
        let event_sequence = u64::try_from(row.event_sequence)?;
        Ok(ThreadRuntimeSeed {
            thread_revision: u64::try_from(row.revision)?,
            runtime_revision: event_sequence.saturating_add(1).max(1),
            event_sequence: event_sequence.saturating_add(1).max(1),
        })
    }

    /// 返回每个活动 Task 根会话最新的完整 Plan，供启动时修复缺失的确认投影。
    pub(in crate::studio) async fn list_latest_task_plan_traces(
        &self,
    ) -> Result<Vec<RecoverableTaskPlan>> {
        let threads = thread::Entity::find()
            .filter(thread::Column::Mode.eq("task"))
            .filter(thread::Column::Archived.eq(0))
            .filter(thread::Column::ParentThreadId.is_null())
            .order_by_asc(thread::Column::Id)
            .all(&self.db)
            .await?;
        let mut plans = Vec::new();
        for thread in threads {
            let thread_id = thread.id;
            let history = item::Entity::find()
                .filter(item::Column::ThreadId.eq(thread_id.clone()))
                .filter(item::Column::ItemKind.eq("plan"))
                .filter(item::Column::Status.eq("completed"))
                .order_by_desc(item::Column::Ordinal)
                .all(&self.db)
                .await?;
            let plan = history.into_iter().find_map(|item| {
                let item: ThreadItem = match serde_json::from_str(&item.payload_json) {
                    Ok(item) => item,
                    Err(error) => {
                        tracing::warn!(
                            thread_id = %thread_id,
                            error_bytes = error.to_string().len(),
                            "skipping malformed Thread Item during plan recovery"
                        );
                        return None;
                    }
                };
                let ThreadItemContent::Plan { content } = item.content else {
                    return None;
                };
                if item.status != ThreadItemStatus::Completed || content.trim().is_empty() {
                    return None;
                }
                Some(RecoverablePlan {
                    turn_id: item.turn_id,
                    item_id: item.id,
                    content,
                })
            });
            let Some(plan) = plan else {
                continue;
            };
            plans.push(RecoverableTaskPlan {
                agent_id: thread_id.clone(),
                thread_id,
                plan,
            });
        }
        Ok(plans)
    }

    /// 查询 turn 的产品元数据；queue 行转为 running/terminal 时仍会保留该值。
    pub(in crate::studio) async fn agent_turn_metadata(
        &self,
        agent_id: &str,
        turn_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        turn::Entity::find_by_id(turn_id.to_string())
            .filter(turn::Column::ThreadId.eq(agent_id))
            .one(&self.db)
            .await?
            .map(|turn| {
                turn.metadata_json
                    .map(|metadata| serde_json::from_str(&metadata))
                    .transpose()
                    .map_err(Into::into)
            })
            .transpose()
            .map(Option::flatten)
    }
}
