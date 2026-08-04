use anyhow::Result;
use pl_protocol::{ThreadItem, ThreadItemContent, ThreadItemStatus};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::StudioStore;
use crate::studio::entity::{item, thread, turn};

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

impl StudioStore {
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
