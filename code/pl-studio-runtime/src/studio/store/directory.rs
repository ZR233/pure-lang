//! Thread/Project 目录事实：内存目录 owner 提交的 delta、write-behind 落库
//! applier 与 SQLite 冷分页查询。
//!
//! 目录 mutation 一律"内存先行 + delta 异步落库"（见 design/19 §19.2）；本模块
//! 只承载已经由 owner 决定的事实，不做业务校验或状态转换。

use anyhow::{Result, bail};
use pl_protocol::{Thread, ThreadModeId};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::thread_record;
use crate::studio::store::StudioStore;
use crate::studio::store_support::non_empty_title;

/// 一次目录事实提交。
///
/// 由内存目录 owner（ProductEventBus 侧命令）在更新热集合同一临界区构建；
/// FIFO 队列保证 `thread_upserts` 的注册先于该 Thread 的首个 state commit 落库。
#[derive(Debug, Clone, Default)]
pub(in crate::studio) struct DirectoryDelta {
    pub(in crate::studio) thread_upserts: Vec<Thread>,
    pub(in crate::studio) thread_removals: Vec<ThreadRemoval>,
    pub(in crate::studio) project_upserts: Vec<ProjectDirectoryRecord>,
    pub(in crate::studio) project_removals: Vec<ProjectRemoval>,
}

impl DirectoryDelta {
    pub(in crate::studio) fn is_empty(&self) -> bool {
        self.thread_upserts.is_empty()
            && self.thread_removals.is_empty()
            && self.project_upserts.is_empty()
            && self.project_removals.is_empty()
    }

    /// 判断该 delta 是否仍携带指定 Thread owner 的目录事实。
    ///
    /// LRU 逐出在释放 Thread 热对象前使用它扩展 owner durability barrier，避免
    /// runtime revision 已耐久但同一 owner 的标题、归档或 Project 关闭事实仍在队列。
    pub(in crate::studio) fn touches_thread(&self, thread_id: &str) -> bool {
        self.thread_upserts
            .iter()
            .any(|thread| thread.id == thread_id)
            || self
                .thread_removals
                .iter()
                .any(|removal| removal.thread_ids.iter().any(|id| id == thread_id))
            || self
                .project_removals
                .iter()
                .any(|removal| removal.thread_ids.iter().any(|id| id == thread_id))
    }

    /// 注册一个新的 root Thread（`startNewThread` / 测试 seed 入口）。
    pub(in crate::studio) fn register_root_thread(
        project_id: &str,
        title: &str,
        mode: ThreadModeId,
    ) -> (Self, Thread) {
        let now = unix_seconds();
        let id = new_id("thread");
        let thread = Thread {
            root_thread_id: id.clone(),
            agent_path: id.clone(),
            id,
            project_id: project_id.to_string(),
            title: non_empty_title(title),
            mode: mode.clone(),
            parent_thread_id: None,
            role: crate::config::StudioRole::Planner.key().to_string(),
            status: pl_protocol::ThreadStatus::Idle,
            created_at: now,
            updated_at: now,
            archived: false,
        };
        (
            Self {
                thread_upserts: vec![thread.clone()],
                ..Default::default()
            },
            thread,
        )
    }

    /// 注册一个 durable child Thread（executor/reviewer/explorer spawn）。
    pub(in crate::studio) fn register_child_thread(spec: RegisteredChildThread) -> Self {
        let now = unix_seconds();
        Self {
            thread_upserts: vec![Thread {
                agent_path: spec.agent_path,
                id: spec.id,
                project_id: spec.project_id,
                title: non_empty_title(&spec.title),
                mode: spec.mode,
                root_thread_id: spec.root_thread_id,
                parent_thread_id: Some(spec.parent_thread_id),
                role: spec.role,
                status: pl_protocol::ThreadStatus::Idle,
                created_at: now,
                updated_at: now,
                archived: false,
            }],
            ..Default::default()
        }
    }

    /// 归档一棵 Thread 树（调用方已确定全部成员 id）。
    pub(in crate::studio) fn archive_threads(thread_ids: Vec<String>) -> Self {
        Self {
            thread_removals: vec![ThreadRemoval {
                thread_ids,
                archived_at: unix_seconds(),
            }],
            ..Default::default()
        }
    }

    pub(in crate::studio) fn upsert_project(record: ProjectDirectoryRecord) -> Self {
        Self {
            project_upserts: vec![record],
            ..Default::default()
        }
    }
}

/// child Thread 注册事实；父 Thread 的目录事实由调用方（agent host）提供。
#[derive(Debug, Clone)]
pub(in crate::studio) struct RegisteredChildThread {
    pub(in crate::studio) id: String,
    pub(in crate::studio) parent_thread_id: String,
    pub(in crate::studio) agent_path: String,
    pub(in crate::studio) project_id: String,
    pub(in crate::studio) root_thread_id: String,
    pub(in crate::studio) mode: ThreadModeId,
    pub(in crate::studio) role: String,
    pub(in crate::studio) title: String,
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct ThreadRemoval {
    pub(in crate::studio) thread_ids: Vec<String>,
    pub(in crate::studio) archived_at: i64,
}

#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectRemoval {
    pub(in crate::studio) project_id: String,
    pub(in crate::studio) thread_ids: Vec<String>,
    pub(in crate::studio) closed_at: i64,
}

/// Project 目录行事实；插入时需要完整列，更新时只改目录列。
#[derive(Debug, Clone)]
pub(in crate::studio) struct ProjectDirectoryRecord {
    pub(in crate::studio) id: String,
    pub(in crate::studio) name: String,
    pub(in crate::studio) path: String,
    pub(in crate::studio) ssh_server_id: Option<String>,
    pub(in crate::studio) created_at: i64,
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) last_opened_at: Option<i64>,
    pub(in crate::studio) closed: bool,
}

/// 在 writer 已开启的事务中幂等应用一次目录 delta。
pub(in crate::studio) async fn apply_directory_delta(
    tx: &sea_orm::DatabaseTransaction,
    delta: &DirectoryDelta,
) -> Result<()> {
    for thread in &delta.thread_upserts {
        upsert_thread_directory_row(tx, thread).await?;
    }
    for removal in &delta.thread_removals {
        archive_thread_rows(tx, &removal.thread_ids, removal.archived_at).await?;
    }
    for project in &delta.project_upserts {
        upsert_project_directory_row(tx, project).await?;
    }
    for removal in &delta.project_removals {
        archive_thread_rows(tx, &removal.thread_ids, removal.closed_at).await?;
        close_project_row(tx, &removal.project_id, removal.closed_at).await?;
    }
    Ok(())
}

async fn upsert_thread_directory_row(
    tx: &sea_orm::DatabaseTransaction,
    thread: &Thread,
) -> Result<()> {
    use entities::thread;
    anyhow::ensure!(
        thread.agent_path == thread.id,
        "Thread {} directory identity must match its runtime identity",
        thread.id
    );
    let existing = thread::Entity::find_by_id(thread.id.clone())
        .one(tx)
        .await?;
    let Some(existing) = existing else {
        let model = thread::ActiveModel {
            id: Set(thread.id.clone()),
            project_id: Set(thread.project_id.clone()),
            title: Set(thread.title.clone()),
            mode: Set(thread.mode.label().to_string()),
            root_thread_id: Set(thread.root_thread_id.clone()),
            parent_thread_id: Set(thread.parent_thread_id.clone()),
            role: Set(thread.role.clone()),
            agent_path: Set(thread.agent_path.clone()),
            state_json: Set(serde_json::to_string(&pl_core::AgentState::idle())?),
            revision: Set(0),
            runtime_revision: Set(None),
            event_sequence: Set(0),
            metadata_json: Set("{}".to_string()),
            usage_json: Set(serde_json::to_string(
                &pl_protocol::InferenceTokenUsage::default(),
            )?),
            last_context_tokens: Set(None),
            trace_sequence: Set(0),
            created_at: Set(thread.created_at),
            updated_at: Set(thread.updated_at),
            archived: Set(i32::from(thread.archived)),
            ..Default::default()
        };
        model.insert(tx).await?;
        return Ok(());
    };
    // 身份列不可变；目录更新只触碰标题/模式/角色/归档与时间戳，
    // runtime 列由 state commit 拥有。
    if existing.project_id != thread.project_id
        || existing.root_thread_id != thread.root_thread_id
        || existing.parent_thread_id != thread.parent_thread_id
    {
        bail!(
            "Thread {} directory identity changed: persisted {:?} vs delta {:?}",
            thread.id,
            (
                existing.project_id.as_str(),
                existing.root_thread_id.as_str(),
                existing.parent_thread_id.as_deref()
            ),
            (
                thread.project_id.as_str(),
                thread.root_thread_id.as_str(),
                thread.parent_thread_id.as_deref()
            )
        );
    }
    let mut active: thread::ActiveModel = existing.into();
    active.title = Set(thread.title.clone());
    active.mode = Set(thread.mode.label().to_string());
    active.role = Set(thread.role.clone());
    active.updated_at = Set(thread.updated_at);
    active.archived = Set(i32::from(thread.archived));
    active.update(tx).await?;
    Ok(())
}

async fn archive_thread_rows(
    tx: &sea_orm::DatabaseTransaction,
    thread_ids: &[String],
    archived_at: i64,
) -> Result<()> {
    use entities::thread;
    for thread_id in thread_ids {
        if let Some(existing) = thread::Entity::find_by_id(thread_id.clone())
            .one(tx)
            .await?
        {
            let mut active: thread::ActiveModel = existing.into();
            active.archived = Set(1);
            active.updated_at = Set(archived_at);
            active.update(tx).await?;
        }
    }
    Ok(())
}

async fn upsert_project_directory_row(
    tx: &sea_orm::DatabaseTransaction,
    record: &ProjectDirectoryRecord,
) -> Result<()> {
    use entities::project;
    let existing = project::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let Some(existing) = existing else {
        project::ActiveModel {
            id: Set(record.id.clone()),
            name: Set(record.name.clone()),
            path: Set(record.path.clone()),
            ssh_server_id: Set(record.ssh_server_id.clone()),
            created_at: Set(record.created_at),
            updated_at: Set(record.updated_at),
            last_opened_at: Set(record.last_opened_at),
            closed: Set(i32::from(record.closed)),
        }
        .insert(tx)
        .await?;
        return Ok(());
    };
    if existing.path != record.path || existing.ssh_server_id != record.ssh_server_id {
        bail!(
            "Project {} directory identity changed: persisted path {}, delta path {}",
            record.id,
            existing.path,
            record.path
        );
    }
    let mut active: project::ActiveModel = existing.into();
    active.name = Set(record.name.clone());
    active.updated_at = Set(record.updated_at);
    active.last_opened_at = Set(record.last_opened_at);
    active.closed = Set(i32::from(record.closed));
    active.update(tx).await?;
    Ok(())
}

async fn close_project_row(
    tx: &sea_orm::DatabaseTransaction,
    project_id: &str,
    closed_at: i64,
) -> Result<()> {
    use entities::project;
    if let Some(existing) = project::Entity::find_by_id(project_id.to_string())
        .one(tx)
        .await?
    {
        let mut active: project::ActiveModel = existing.into();
        active.closed = Set(1);
        active.updated_at = Set(closed_at);
        active.update(tx).await?;
    }
    Ok(())
}

/// Thread 目录 keyset 分页游标：`v1:{updated_at}:{id}`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::studio) struct ThreadDirectoryCursor {
    pub(in crate::studio) updated_at: i64,
    pub(in crate::studio) id: String,
}

impl ThreadDirectoryCursor {
    pub(in crate::studio) fn encode(&self) -> String {
        format!("v1:{}:{}", self.updated_at, self.id)
    }

    pub(in crate::studio) fn decode(raw: &str) -> Option<Self> {
        let rest = raw.strip_prefix("v1:")?;
        let (updated_at, id) = rest.split_once(':')?;
        Some(Self {
            updated_at: updated_at.parse().ok()?,
            id: id.to_string(),
        })
    }
}

impl StudioStore {
    /// 未归档 Thread 的冷分页：按 `(updated_at, id)` 倒序 keyset，
    /// cursor 为闭区间锚点（下一页取严格小于该键的条目）。
    pub(in crate::studio) async fn list_thread_directory_page(
        &self,
        cursor: Option<&ThreadDirectoryCursor>,
        limit: usize,
    ) -> Result<Vec<Thread>> {
        use entities::thread;
        let mut query = thread::Entity::find()
            .filter(thread::Column::Archived.eq(0))
            .order_by_desc(thread::Column::UpdatedAt)
            .order_by_desc(thread::Column::Id);
        if let Some(cursor) = cursor {
            query = query.filter(
                Condition::any()
                    .add(thread::Column::UpdatedAt.lt(cursor.updated_at))
                    .add(
                        Condition::all()
                            .add(thread::Column::UpdatedAt.eq(cursor.updated_at))
                            .add(thread::Column::Id.lt(cursor.id.clone())),
                    ),
            );
        }
        let models = query.limit(u64::try_from(limit)?).all(&self.db).await?;
        models
            .into_iter()
            .map(|model| thread_record(model).map(Thread::from))
            .collect()
    }
}
